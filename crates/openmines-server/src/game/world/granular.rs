use crate::game::{
    BroadcastEffect, BroadcastQueue, GranularWakeQueue, ScheduleConfigResource, WorldResource,
};
use crate::world::WorldProvider;
use crate::world::cells::{CellDefs, cell_type};
use bevy_ecs::prelude::*;
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant};

const GRANULAR_SCAN_RADIUS: i32 = 16;
const GRANULAR_CHUNK_SIZE: i32 = 32;
const GRANULAR_CACHE_X_PAD: i32 = 1;
const GRANULAR_CACHE_Y_LOOKAHEAD: i32 = 3;
const GRANULAR_REGION_RESEED_EVERY: Duration = Duration::from_secs(5);
const GRANULAR_CANDIDATE_BUDGET: usize = 1_024;
const GRANULAR_PARALLEL_CANDIDATE_MIN: usize = 256;
const GRANULAR_PARALLEL_BATCH_MIN: usize = 64;

/// Проходима ли клетка для падающего блока. База — `is_empty` (≡ JS
/// `!BlockStats[id].solid`): песок течёт сквозь фон/обычную дорогу(35)/ворота(30).
/// НО плюс C#-блоклист `TrueEmpty`: `cell ∉ {0,36,37,39}` — инфраструктура зданий
/// (NOTHING, ЗОЛОТАЯ ДОРОГА 36, ДВЕРЬ 37, ПОЛИМЕР 39) НЕ проходима, иначе песок
/// затирает её («жрёт золотую дорогу»). Чистый JS-geophys это не ловит (там 36
/// не-solid), но C# `Physics` именно так и защищает паки. OOB = непроходимо.
fn is_passable(world: &crate::world::World, x: i32, y: i32) -> bool {
    use crate::world::cells::cell_type as ct;
    if !world.valid_coord(x, y) {
        return false;
    }
    let cell = world.get_cell_typed(x, y);
    world.is_empty(x, y)
        && !matches!(
            cell.0,
            ct::NOTHING | ct::GOLDEN_ROAD | ct::BUILDING_DOOR | ct::POLYMER_ROAD
        )
}

fn cell_is_granular(cell_defs: &CellDefs, cell: crate::world::CellType) -> bool {
    cell_defs.get_typed(cell).is_sand() || cell.is_boulder()
}

struct GranularScanCache {
    min_x: i32,
    min_y: i32,
    width: usize,
    height: usize,
    cells: Vec<Option<crate::world::CellType>>,
}

impl GranularScanCache {
    fn new(world: &crate::world::World, player_x: i32, player_y: i32) -> Self {
        let min_x = player_x - GRANULAR_SCAN_RADIUS - GRANULAR_CACHE_X_PAD;
        let max_x = player_x + GRANULAR_SCAN_RADIUS + GRANULAR_CACHE_X_PAD;
        let min_y = player_y - GRANULAR_SCAN_RADIUS;
        let max_y = player_y + GRANULAR_SCAN_RADIUS + GRANULAR_CACHE_Y_LOOKAHEAD;
        let width = usize::try_from(max_x - min_x + 1).expect("granular cache width is positive");
        let height = usize::try_from(max_y - min_y + 1).expect("granular cache height is positive");
        let cells = world.snapshot_cells_rect(min_x, min_y, width, height);
        Self {
            min_x,
            min_y,
            width,
            height,
            cells,
        }
    }

    fn get(&self, x: i32, y: i32) -> Option<crate::world::CellType> {
        let rx = x.checked_sub(self.min_x)?;
        let ry = y.checked_sub(self.min_y)?;
        let rx = usize::try_from(rx).ok()?;
        let ry = usize::try_from(ry).ok()?;
        if rx >= self.width || ry >= self.height {
            return None;
        }
        self.cells[ry * self.width + rx]
    }

    fn for_candidates(world: &crate::world::World, candidates: &[(i32, i32)]) -> Self {
        let (first_x, first_y) = candidates[0];
        let mut min_x = first_x - GRANULAR_CACHE_X_PAD;
        let mut max_x = first_x + GRANULAR_CACHE_X_PAD;
        let mut min_y = first_y;
        let mut max_y = first_y + GRANULAR_CACHE_Y_LOOKAHEAD;
        for &(x, y) in candidates.iter().skip(1) {
            min_x = min_x.min(x - GRANULAR_CACHE_X_PAD);
            max_x = max_x.max(x + GRANULAR_CACHE_X_PAD);
            min_y = min_y.min(y);
            max_y = max_y.max(y + GRANULAR_CACHE_Y_LOOKAHEAD);
        }
        let width =
            usize::try_from(max_x - min_x + 1).expect("granular candidate cache width is positive");
        let height = usize::try_from(max_y - min_y + 1)
            .expect("granular candidate cache height is positive");
        let cells = world.snapshot_cells_rect(min_x, min_y, width, height);
        Self {
            min_x,
            min_y,
            width,
            height,
            cells,
        }
    }
}

fn is_passable_cached(cache: &GranularScanCache, cell_defs: &CellDefs, x: i32, y: i32) -> bool {
    use crate::world::cells::cell_type as ct;
    let Some(cell) = cache.get(x, y) else {
        return false;
    };
    cell_defs.get_typed(cell).cell_is_empty()
        && !matches!(
            cell.0,
            ct::NOTHING | ct::GOLDEN_ROAD | ct::BUILDING_DOOR | ct::POLYMER_ROAD
        )
}

fn down_free_cached(cache: &GranularScanCache, cell_defs: &CellDefs, x: i32, y: i32) -> u8 {
    if is_passable_cached(cache, cell_defs, x, y + 1) {
        if cache
            .get(x, y + 2)
            .is_some_and(|cell| cell_is_granular(cell_defs, cell))
        {
            if !is_passable_cached(cache, cell_defs, x, y + 3) {
                return 1;
            }
            return 2;
        }
        return 1;
    }
    if cache
        .get(x, y + 1)
        .is_some_and(|cell| cell_is_granular(cell_defs, cell))
    {
        return 0;
    }
    2
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct GranularChunkKey {
    x: i32,
    y: i32,
}

impl GranularChunkKey {
    const fn for_cell(x: i32, y: i32) -> Self {
        Self {
            x: x.div_euclid(GRANULAR_CHUNK_SIZE),
            y: y.div_euclid(GRANULAR_CHUNK_SIZE),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct GranularIntent {
    src_x: i32,
    src_y: i32,
    dest_x: i32,
    dest_y: i32,
    cell: crate::world::CellType,
}

impl GranularIntent {
    const fn stable_key(self) -> (i32, i32, i32, i32, u8) {
        (
            self.src_x,
            self.src_y,
            self.dest_x,
            self.dest_y,
            self.cell.0,
        )
    }

    const fn source_chunk(self) -> GranularChunkKey {
        GranularChunkKey::for_cell(self.src_x, self.src_y)
    }

    const fn crosses_chunk_boundary(self) -> bool {
        self.source_chunk().x != GranularChunkKey::for_cell(self.dest_x, self.dest_y).x
            || self.source_chunk().y != GranularChunkKey::for_cell(self.dest_x, self.dest_y).y
    }
}

struct GranularCandidateBatch {
    owner: GranularChunkKey,
    candidates: Vec<(i32, i32)>,
    snapshot: GranularScanCache,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct GranularAnalyzeProfile {
    skipped_oob: usize,
    skipped_non_granular: usize,
    blocked: usize,
    falling_cells: usize,
}

impl GranularAnalyzeProfile {
    const fn merge(&mut self, other: Self) {
        self.skipped_oob += other.skipped_oob;
        self.skipped_non_granular += other.skipped_non_granular;
        self.blocked += other.blocked;
        self.falling_cells += other.falling_cells;
    }
}

struct GranularAnalyzeResult {
    intent: Option<GranularIntent>,
    profile: GranularAnalyzeProfile,
}

struct GranularAnalyzeSummary {
    intents: Vec<GranularIntent>,
    profile: GranularAnalyzeProfile,
    batches: usize,
}

struct GranularApplyResult {
    applied: usize,
    granular_activated: usize,
    effects: Vec<BroadcastEffect>,
}

const fn granular_slide_order(x: i32, y: i32) -> (i32, i32) {
    if (x ^ y) & 1 == 0 {
        (x + 1, x - 1)
    } else {
        (x - 1, x + 1)
    }
}

fn analyze_granular_candidate(
    world: &crate::world::World,
    cell_defs: &CellDefs,
    snapshot: &GranularScanCache,
    sx: i32,
    sy: i32,
) -> GranularAnalyzeResult {
    let mut profile = GranularAnalyzeProfile::default();
    if !world.valid_coord(sx, sy) {
        profile.skipped_oob += 1;
        return GranularAnalyzeResult {
            intent: None,
            profile,
        };
    }
    let cell = snapshot
        .get(sx, sy)
        .expect("valid granular candidate must exist in its chunk snapshot");
    let is_s = cell_defs.get_typed(cell).is_sand();
    let is_b = cell.is_boulder();
    if !is_s && !is_b {
        profile.skipped_non_granular += 1;
        return GranularAnalyzeResult {
            intent: None,
            profile,
        };
    }
    profile.falling_cells += 1;

    let df = down_free_cached(snapshot, cell_defs, sx, sy);
    if df == 1 {
        return GranularAnalyzeResult {
            intent: Some(GranularIntent {
                src_x: sx,
                src_y: sy,
                dest_x: sx,
                dest_y: sy + 1,
                cell,
            }),
            profile,
        };
    }
    if df != 0 {
        profile.blocked += 1;
        return GranularAnalyzeResult {
            intent: None,
            profile,
        };
    }

    let (first_x, second_x) = granular_slide_order(sx, sy);
    let can_slide = |tx: i32| {
        is_passable_cached(snapshot, cell_defs, tx, sy + 1)
            && (is_s || is_passable_cached(snapshot, cell_defs, tx, sy))
    };
    let dest_x = if can_slide(first_x) {
        Some(first_x)
    } else if can_slide(second_x) {
        Some(second_x)
    } else {
        None
    };
    let Some(dest_x) = dest_x else {
        profile.blocked += 1;
        return GranularAnalyzeResult {
            intent: None,
            profile,
        };
    };
    GranularAnalyzeResult {
        intent: Some(GranularIntent {
            src_x: sx,
            src_y: sy,
            dest_x,
            dest_y: sy + 1,
            cell,
        }),
        profile,
    }
}

fn build_granular_candidate_batches(
    world: &crate::world::World,
    candidates: Vec<(i32, i32)>,
) -> Vec<GranularCandidateBatch> {
    let mut grouped = BTreeMap::<GranularChunkKey, Vec<(i32, i32)>>::new();
    for candidate @ (x, y) in candidates {
        grouped
            .entry(GranularChunkKey::for_cell(x, y))
            .or_default()
            .push(candidate);
    }
    grouped
        .into_iter()
        .map(|(owner, candidates)| {
            let snapshot = GranularScanCache::for_candidates(world, &candidates);
            GranularCandidateBatch {
                owner,
                candidates,
                snapshot,
            }
        })
        .collect()
}

fn analyze_granular_batch(
    world: &crate::world::World,
    cell_defs: &CellDefs,
    batch: &GranularCandidateBatch,
    parallel: bool,
) -> GranularAnalyzeSummary {
    let analyze = |&(sx, sy): &(i32, i32)| {
        analyze_granular_candidate(world, cell_defs, &batch.snapshot, sx, sy)
    };
    let analyzed: Vec<_> = if parallel && batch.candidates.len() >= GRANULAR_PARALLEL_BATCH_MIN {
        batch.candidates.par_iter().map(analyze).collect()
    } else {
        batch.candidates.iter().map(analyze).collect()
    };
    let mut profile = GranularAnalyzeProfile::default();
    let mut intents = Vec::new();
    for result in analyzed {
        profile.merge(result.profile);
        if let Some(intent) = result.intent {
            debug_assert_eq!(intent.source_chunk(), batch.owner);
            intents.push(intent);
        }
    }
    GranularAnalyzeSummary {
        intents,
        profile,
        batches: 1,
    }
}

fn analyze_granular_candidates(
    world: &crate::world::World,
    cell_defs: &CellDefs,
    candidates: Vec<(i32, i32)>,
    parallel: bool,
) -> GranularAnalyzeSummary {
    let batches = build_granular_candidate_batches(world, candidates);
    let analyzed: Vec<_> = if parallel && batches.len() > 1 {
        batches
            .par_iter()
            .map(|batch| analyze_granular_batch(world, cell_defs, batch, true))
            .collect()
    } else {
        batches
            .iter()
            .map(|batch| analyze_granular_batch(world, cell_defs, batch, parallel))
            .collect()
    };
    let mut summary = GranularAnalyzeSummary {
        intents: Vec::new(),
        profile: GranularAnalyzeProfile::default(),
        batches: analyzed.len(),
    };
    for batch in analyzed {
        summary.profile.merge(batch.profile);
        summary.intents.extend(batch.intents);
    }
    summary
}

fn canonicalize_granular_intents(intents: &mut Vec<GranularIntent>) {
    intents.sort_unstable_by_key(|intent| intent.stable_key());
    intents.dedup_by(|a, b| a.src_x == b.src_x && a.src_y == b.src_y);
}

fn apply_granular_intents(
    world: &crate::world::World,
    cell_defs: &CellDefs,
    physics_state: &mut GranularPhysicsState,
    intents: Vec<GranularIntent>,
) -> GranularApplyResult {
    let mut applied = 0usize;
    let mut granular_activated = 0usize;
    let mut effects = Vec::with_capacity(intents.len().saturating_mul(2));
    for intent in intents {
        if is_passable(world, intent.dest_x, intent.dest_y) {
            applied += 1;
            // 1:1 C# `World.MoveCell` (Physics): durability ПЕРЕНОСИТСЯ. `set_cell`
            // сбрасывает её на дефолт типа → без переноса недокопанный валун «лечится»
            // до полной при падении. Читаем durability ДО очистки источника.
            let dur = world.get_durability(intent.src_x, intent.src_y);
            world.set_cell_typed(
                intent.src_x,
                intent.src_y,
                crate::world::CellType(cell_type::EMPTY),
            );
            world.write_world_cell(
                intent.dest_x,
                intent.dest_y,
                crate::world::WorldCell {
                    cell_type: intent.cell,
                    durability: dur,
                },
            );

            effects.push(BroadcastEffect::CellUpdate(
                (intent.src_x, intent.src_y).into(),
            ));
            effects.push(BroadcastEffect::CellUpdate(
                (intent.dest_x, intent.dest_y).into(),
            ));
            granular_activated += physics_state.activate_granular_neighborhood(
                world,
                cell_defs,
                intent.src_x,
                intent.src_y,
            );
            granular_activated += physics_state.activate_granular_neighborhood(
                world,
                cell_defs,
                intent.dest_x,
                intent.dest_y,
            );
        } else {
            granular_activated += physics_state.activate_granular_neighborhood(
                world,
                cell_defs,
                intent.src_x,
                intent.src_y,
            );
        }
    }
    GranularApplyResult {
        applied,
        granular_activated,
        effects,
    }
}

#[derive(Default)]
struct GranularPhysicsState {
    active_cells: HashSet<(i32, i32)>,
    region_seeded_at: HashMap<(i32, i32), Instant>,
}

impl GranularPhysicsState {
    const fn active_region_key(x: i32, y: i32) -> (i32, i32) {
        (x.div_euclid(32), y.div_euclid(32))
    }

    fn seed_active_region(
        &mut self,
        world: &crate::world::World,
        cell_defs: &CellDefs,
        player_x: i32,
        player_y: i32,
        now: Instant,
    ) -> (usize, bool) {
        let key = Self::active_region_key(player_x, player_y);
        if self
            .region_seeded_at
            .get(&key)
            .is_some_and(|seeded_at| now.duration_since(*seeded_at) < GRANULAR_REGION_RESEED_EVERY)
        {
            return (0, false);
        }
        self.region_seeded_at.insert(key, now);
        let cache = GranularScanCache::new(world, player_x, player_y);
        let mut scanned = 0usize;
        for dy in (-GRANULAR_SCAN_RADIUS..=GRANULAR_SCAN_RADIUS).rev() {
            for dx in -GRANULAR_SCAN_RADIUS..=GRANULAR_SCAN_RADIUS {
                scanned += 1;
                let sx = player_x + dx;
                let sy = player_y + dy;
                let Some(cell) = cache.get(sx, sy) else {
                    continue;
                };
                if cell_is_granular(cell_defs, cell) {
                    self.active_cells.insert((sx, sy));
                }
            }
        }
        (scanned, true)
    }

    fn activate_granular_neighborhood(
        &mut self,
        world: &crate::world::World,
        cell_defs: &CellDefs,
        x: i32,
        y: i32,
    ) -> usize {
        let mut activated = 0usize;
        for dy in -2..=1 {
            for dx in -1..=1 {
                let ax = x + dx;
                let ay = y + dy;
                if !world.valid_coord(ax, ay) {
                    continue;
                }
                let cell = world.get_cell_typed(ax, ay);
                if cell_is_granular(cell_defs, cell) {
                    activated += usize::from(self.active_cells.insert((ax, ay)));
                }
            }
        }
        activated
    }

    fn take_candidates(&mut self, limit: usize) -> (Vec<(i32, i32)>, usize) {
        let mut candidates: Vec<_> = self.active_cells.drain().collect();
        candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if candidates.len() <= limit {
            return (candidates, 0);
        }
        let deferred = candidates.split_off(limit);
        let deferred_len = deferred.len();
        self.active_cells.extend(deferred);
        (candidates, deferred_len)
    }
}

fn activate_woken_granular_cells(
    wake_q: &GranularWakeQueue,
    physics_state: &mut GranularPhysicsState,
    world: &crate::world::World,
    cell_defs: &CellDefs,
) -> (usize, usize, usize, usize, usize) {
    let (mut wake_points, mut region_seeds) = wake_q.take();
    let received = wake_points.len();
    wake_points.sort_unstable_by_key(|pos| {
        let (x, y): (i32, i32) = (*pos).into();
        (y, x)
    });
    wake_points.dedup();
    let unique = wake_points.len();
    let activated = wake_points
        .into_iter()
        .map(|pos| {
            let (x, y): (i32, i32) = pos.into();
            physics_state.activate_granular_neighborhood(world, cell_defs, x, y)
        })
        .sum();
    let region_seeds_received = region_seeds.len();
    region_seeds.sort_unstable_by_key(|pos| {
        let (x, y): (i32, i32) = (*pos).into();
        (y, x)
    });
    region_seeds.dedup();
    let mut region_cells_scanned = 0usize;
    for pos in region_seeds {
        let (x, y): (i32, i32) = pos.into();
        let (scanned, _) = physics_state.seed_active_region(world, cell_defs, x, y, Instant::now());
        region_cells_scanned += scanned;
    }
    (
        received,
        unique,
        activated,
        region_seeds_received,
        region_cells_scanned,
    )
}

#[allow(clippy::needless_pass_by_value)]
fn granular_physics_system(
    world_res: Res<WorldResource>,
    schedule_cfg: Res<ScheduleConfigResource>,
    wake_q: Res<GranularWakeQueue>,
    mut bcast_q: ResMut<BroadcastQueue>,
    mut physics_state: Local<GranularPhysicsState>,
) {
    let started_at = Instant::now();
    let world = &world_res.0;
    let cell_defs = world.cell_defs();
    let scan_t0 = Instant::now();
    let (
        wake_points_len,
        wake_points_unique,
        wake_granular_activated,
        region_seeds_received,
        region_cells_scanned,
    ) = activate_woken_granular_cells(&wake_q, &mut physics_state, world, &cell_defs);

    let frontier_before = physics_state.active_cells.len();
    let (candidates, candidates_deferred) =
        physics_state.take_candidates(GRANULAR_CANDIDATE_BUDGET);
    let candidates_processed = candidates.len();
    let parallel_analysis = candidates.len() >= GRANULAR_PARALLEL_CANDIDATE_MIN;
    let GranularAnalyzeSummary {
        mut intents,
        profile: analyze_profile,
        batches: candidate_batches,
    } = analyze_granular_candidates(world, &cell_defs, candidates, parallel_analysis);
    let falling_cells = analyze_profile.falling_cells;
    let candidates_skipped_oob = analyze_profile.skipped_oob;
    let candidates_skipped_non_granular = analyze_profile.skipped_non_granular;
    let candidates_blocked = analyze_profile.blocked;
    let frontier_after_scan = physics_state.active_cells.len();
    let scan_time = scan_t0.elapsed();

    // Stable order preserves the legacy sequential conflict winner regardless of Rayon order.
    let dedup_t0 = Instant::now();
    canonicalize_granular_intents(&mut intents);
    let cross_chunk_intents = intents
        .iter()
        .filter(|intent| intent.crosses_chunk_boundary())
        .count();
    let dedup_time = dedup_t0.elapsed();

    let apply_t0 = Instant::now();
    let apply_result = apply_granular_intents(world, &cell_defs, &mut physics_state, intents);
    let applied = apply_result.applied;
    let apply_granular_activated = apply_result.granular_activated;
    bcast_q.0.extend(apply_result.effects);
    let frontier_after_apply = physics_state.active_cells.len();
    wake_q.set_active(frontier_after_apply > 0);
    let apply_time = apply_t0.elapsed();
    let total = started_at.elapsed();
    let threshold = Duration::from_millis(schedule_cfg.0.schedule_warn_threshold_ms)
        .min(Duration::from_millis(schedule_cfg.0.game_loop_tick_rate_ms));
    if total > threshold {
        tracing::warn!(
            target: "tickprof",
            wake_points = wake_points_len,
            wake_points_unique,
            wake_granular_activated,
            region_seeds_received,
            region_cells_scanned,
            frontier_before,
            frontier_after_scan,
            frontier_after_apply,
            candidates_processed,
            candidates_deferred,
            candidate_batches,
            candidate_budget = GRANULAR_CANDIDATE_BUDGET,
            parallel_analysis,
            cross_chunk_intents,
            candidates_skipped_oob,
            candidates_skipped_non_granular,
            candidates_blocked,
            falling_cells,
            applied,
            apply_granular_activated,
            scan_time = ?scan_time,
            dedup_time = ?dedup_time,
            apply_time = ?apply_time,
            total = ?total,
            threshold = ?threshold,
            "SLOW granular physics system"
        );
    }
}

pub fn add_granular_physics_system(schedule: &mut Schedule) {
    schedule.add_systems(granular_physics_system);
}

#[cfg(test)]
mod physics_repro {
    //! Изолированный прогон cell-мутирующих систем (granular/alive) без сети:
    //! реальный `World`, игрок-entity, форс таймеров → проверяем (1) двигает ли
    //! физика клетки вообще, (2) не плодит ли НЕВАЛИДНЫЕ байты (порча карты).
    use crate::game::player::{PlayerConnection, PlayerPosition};
    use crate::game::{BroadcastEffect, BroadcastQueue, WorldResource, alive};
    use crate::world::cells::{CellDefs, cell_type};
    use crate::world::{World, WorldProvider};
    use bevy_ecs::prelude::*;
    use std::sync::Arc;

    fn spawn_connected_test_player(w: &mut bevy_ecs::world::World, x: i32, y: i32) {
        w.spawn((
            PlayerPosition { x, y, dir: 0 },
            PlayerConnection {
                session_id: crate::game::SessionId::new(1),
            },
        ));
    }

    #[test]
    fn granular_chunked_parallel_analysis_matches_sequential_digest() {
        const SAND: u8 = 100;
        let dir = std::env::temp_dir().join(format!("phys_parallel_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
        let world = Arc::new(World::new("phys_parallel", 4, 4, cd, &dir).unwrap());
        for y in 48..76 {
            for x in 16..112 {
                world.set_cell(x, y, cell_type::EMPTY);
            }
        }
        world.set_cell(31, 54, SAND);
        world.set_cell(54, 54, SAND);
        world.set_cell(58, 54, SAND);
        world.set_cell(62, 54, SAND);
        world.set_cell(58, 55, SAND);
        world.set_cell(62, 55, 107);
        world.set_cell(96, 54, SAND);
        world.set_cell(54, 31, SAND);
        world.set_cell(54, 32, cell_type::EMPTY);

        let candidates = vec![
            (96, 54),
            (54, 54),
            (54, 31),
            (31, 54),
            (58, 54),
            (62, 54),
            (10_000, 10_000),
            (50, 50),
        ];
        let cell_defs = world.cell_defs();
        let mut sequential =
            super::analyze_granular_candidates(&world, &cell_defs, candidates.clone(), false);
        let mut parallel = super::analyze_granular_candidates(&world, &cell_defs, candidates, true);
        super::canonicalize_granular_intents(&mut sequential.intents);
        super::canonicalize_granular_intents(&mut parallel.intents);

        assert_eq!(sequential.intents, parallel.intents);
        assert_eq!(sequential.profile, parallel.profile);
        assert_eq!(sequential.batches, parallel.batches);
        assert_eq!(sequential.batches, 5);
        assert!(
            sequential
                .intents
                .iter()
                .any(|intent| intent.crosses_chunk_boundary())
        );
    }

    #[test]
    fn granular_intent_conflicts_have_stable_legacy_order() {
        let mut intents = vec![
            super::GranularIntent {
                src_x: 33,
                src_y: 31,
                dest_x: 32,
                dest_y: 32,
                cell: crate::world::CellType(100),
            },
            super::GranularIntent {
                src_x: 31,
                src_y: 31,
                dest_x: 32,
                dest_y: 32,
                cell: crate::world::CellType(100),
            },
        ];

        super::canonicalize_granular_intents(&mut intents);

        assert_eq!(intents[0].src_x, 31);
        assert_eq!(intents[1].src_x, 33);
        assert!(intents.iter().all(|intent| intent.crosses_chunk_boundary()));
    }

    #[test]
    fn granular_apply_returns_effects_without_broadcast_side_effects() {
        const SAND: u8 = 100;
        let dir = std::env::temp_dir().join(format!("phys_apply_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
        let world = Arc::new(World::new("phys_apply", 4, 4, cd, &dir).unwrap());
        let cell_defs = world.cell_defs();
        world.set_cell(64, 64, SAND);
        world.set_cell(64, 65, cell_type::EMPTY);
        world.flush().unwrap();

        let mut physics_state = super::GranularPhysicsState::default();
        let result = super::apply_granular_intents(
            &world,
            &cell_defs,
            &mut physics_state,
            vec![super::GranularIntent {
                src_x: 64,
                src_y: 64,
                dest_x: 64,
                dest_y: 65,
                cell: crate::world::CellType(SAND),
            }],
        );

        assert_eq!(result.applied, 1);
        assert_eq!(world.get_cell(64, 64), cell_type::EMPTY);
        assert_eq!(world.get_cell(64, 65), SAND);
        assert_eq!(result.effects.len(), 2);
        let positions: Vec<_> = result
            .effects
            .into_iter()
            .filter_map(|effect| match effect {
                BroadcastEffect::CellUpdate(pos) => Some(<(i32, i32)>::from(pos)),
                BroadcastEffect::Nearby { .. }
                | BroadcastEffect::Direct { .. }
                | BroadcastEffect::BlockUpdate(_) => None,
            })
            .collect();
        assert_eq!(positions, vec![(64, 64), (64, 65)]);
        assert!(result.granular_activated > 0);
        assert!(physics_state.active_cells.contains(&(64, 65)));

        let journal_path = dir.join("phys_apply_world.journal");
        assert!(
            std::fs::metadata(&journal_path).unwrap().len() > 0,
            "granular apply must append durable world mutations"
        );
        world.flush().unwrap();
        assert_eq!(std::fs::metadata(&journal_path).unwrap().len(), 0);
        drop(cell_defs);
        drop(world);

        let reopened = World::new(
            "phys_apply",
            4,
            4,
            CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap(),
            &dir,
        )
        .unwrap();
        assert_eq!(reopened.get_cell(64, 64), cell_type::EMPTY);
        assert_eq!(reopened.get_cell(64, 65), SAND);
    }

    #[test]
    fn physics_runs_and_makes_no_garbage() {
        let dir = std::env::temp_dir().join(format!("phys_repro_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
        let n_defs = cd.cells.len();
        let world = Arc::new(World::new("phys", 4, 4, cd, &dir).unwrap());

        // Контролируемая сцена в очищённой области.
        for y in 50..90 {
            for x in 50..78 {
                world.set_cell(x, y, cell_type::EMPTY);
            }
        }
        // Песчинка вверху столбца — должна упасть на дно области.
        world.set_cell(64, 52, 100); // тёмный песок (is_sand)
        assert!(world.cell_defs().get(100).is_sand(), "100 must be sand");
        // Живая клетка — должна расползтись по пустым соседям.
        world.set_cell(60, 70, cell_type::ALIVE_CYAN);

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(crate::game::GranularWakeQueue::default());
        w.insert_resource(crate::game::alive::AliveWorkQueue::default());
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(crate::game::ScheduleConfigResource(
            crate::config::ScheduleConfig::runtime_baseline(),
        ));
        spawn_connected_test_player(&mut w, 64, 65);
        w.resource::<crate::game::GranularWakeQueue>()
            .seed_region(64, 65);
        w.resource::<crate::game::alive::AliveWorkQueue>()
            .seed_region(64, 65);

        let mut sched = Schedule::default();
        sched.add_systems(super::granular_physics_system);
        sched.add_systems(alive::alive_physics_system);

        for _ in 0..80 {
            sched.run(&mut w);
        }

        // (1) Песок реально двигался: верхняя клетка пуста, ниже по столбцу песок.
        let top = world.get_cell(64, 52);
        let column_has_sand =
            (52..90).any(|y| world.cell_defs().get(world.get_cell(64, y)).is_sand());
        println!(
            "sand top(64,52)={top} (EMPTY={}), column_has_sand={column_has_sand}",
            cell_type::EMPTY
        );
        assert_eq!(
            top,
            cell_type::EMPTY,
            "песок не упал — физика не двигает клетки"
        );
        assert!(column_has_sand, "песок исчез полностью");

        // (2) AliveCyan заливает пустых соседей CYAN-кристаллом (НЕ ALIVE_CYAN!):
        // источник остаётся, вокруг растут CYAN. Проверяем, что нарост случился.
        let mut cyan_grown = 0usize;
        for y in 50..90 {
            for x in 50..78 {
                if world.get_cell(x, y) == cell_type::CYAN {
                    cyan_grown += 1;
                }
            }
        }
        println!("CYAN grown by AliveCyan = {cyan_grown}");
        assert!(
            cyan_grown > 0,
            "alive не разросся — живые клетки не работают"
        );

        // (3) НИ ОДНА клетка во всём мире не должна стать невалидным байтом.
        let cw = world.cells_width().cast_signed();
        let ch = world.cells_height().cast_signed();
        let mut garbage = Vec::new();
        for y in 0..ch {
            for x in 0..cw {
                let c = world.get_cell(x, y);
                if usize::from(c) >= n_defs || world.cell_defs().get(c).name.is_empty() {
                    garbage.push((x, y, c));
                }
            }
        }
        println!(
            "garbage/unnamed cells = {} (n_defs={n_defs})",
            garbage.len()
        );
        for (x, y, c) in garbage.iter().take(20) {
            println!("  ({x},{y}) = {c}");
        }
        assert!(
            garbage.is_empty(),
            "физика создала невалидные/безымянные клетки: {} шт",
            garbage.len()
        );
    }

    /// JS `geophys.js`-паритет проходимости: песок падает сквозь НЕ-`solid` клетки
    /// (дорога 35, ворота 30 — в Rust `isEmpty=true`) и НЕ затирает `solid` кристалл
    /// (regress «сожрало кусок чанка»). До фикса `&& !is_building_background` держал
    /// песок на дороге/воротах — песок не доходил до пола.
    #[test]
    fn sand_falls_through_road_gate_rests_on_crystal() {
        const CRYSTAL: u8 = 107; // зелёные кристаллы — solid (isEmpty=false)
        const SAND: u8 = 100; // тёмный жёлтый песок (is_sand)
        let dir = std::env::temp_dir().join(format!("phys_road_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
        let world = Arc::new(World::new("phys_road", 4, 4, cd, &dir).unwrap());

        let x = 64;
        // Изолируем колонку solid-стенами/полом/потолком, чтобы соседний
        // сгенерированный песок не заносился диагональю/сверху в проверяемую клетку.
        for y in 50..=67 {
            world.set_cell(x - 1, y, CRYSTAL);
            world.set_cell(x + 1, y, CRYSTAL);
        }
        for y in 52..66 {
            world.set_cell(x, y, cell_type::EMPTY);
        }
        world.set_cell(x, 51, CRYSTAL); // потолок
        world.set_cell(x, 66, CRYSTAL); // пол
        world.set_cell(x, 58, cell_type::ROAD); // в JS проходима
        world.set_cell(x, 60, cell_type::GATE); // в JS проходима
        world.set_cell(x, 52, SAND);
        assert!(!world.is_empty(x, 66), "кристалл должен быть solid");
        assert!(
            world.is_empty(x, 58),
            "дорога должна быть проходима (JS !solid)"
        );
        assert!(
            world.is_empty(x, 60),
            "ворота должны быть проходимы (JS !solid)"
        );

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(crate::game::GranularWakeQueue::default());
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(crate::game::ScheduleConfigResource(
            crate::config::ScheduleConfig::runtime_baseline(),
        ));
        spawn_connected_test_player(&mut w, x, 60);
        w.resource::<crate::game::GranularWakeQueue>()
            .seed_region(x, 60);

        let mut sched = Schedule::default();
        sched.add_systems(super::granular_physics_system);
        for _ in 0..40 {
            sched.run(&mut w);
        }

        assert_eq!(
            world.get_cell(x, 66),
            CRYSTAL,
            "песок съел solid-кристалл (eating bug)"
        );
        assert_eq!(
            world.get_cell(x, 65),
            SAND,
            "песок не упал на кристалл сквозь дорогу/ворота (over-restriction)"
        );
        assert_eq!(
            world.get_cell(x, 52),
            cell_type::EMPTY,
            "источник песка не очистился"
        );
    }

    /// C# `TrueEmpty`-блоклист {0,36,37,39}: песок НЕ должен жрать золотую дорогу(36),
    /// дверь(37), полимер(39) — инфраструктуру зданий. Садится сверху, не затирает.
    #[test]
    fn sand_does_not_eat_golden_road() {
        const SAND: u8 = 100;
        let dir = std::env::temp_dir().join(format!("phys_gold_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let cd = CellDefs::load(crate::test_config_path("configs/cells.json")).unwrap();
        let world = Arc::new(World::new("phys_gold", 4, 4, cd, &dir).unwrap());
        let x = 64;
        for y in 50..=61 {
            world.set_cell(x - 1, y, 107); // solid-стены изоляции
            world.set_cell(x + 1, y, 107);
        }
        for y in 52..60 {
            world.set_cell(x, y, cell_type::EMPTY);
        }
        world.set_cell(x, 51, 107); // потолок
        world.set_cell(x, 60, cell_type::GOLDEN_ROAD); // пол — НЕ должен быть съеден
        world.set_cell(x, 52, SAND);
        // golden road: isEmpty=true, но C#-блоклист делает её НЕпроходимой для песка.
        assert!(
            world.is_empty(x, 60),
            "golden road всё ещё isEmpty в конфиге"
        );

        let mut w = bevy_ecs::world::World::new();
        w.insert_resource(WorldResource(Arc::clone(&world)));
        w.insert_resource(crate::game::GranularWakeQueue::default());
        w.insert_resource(BroadcastQueue::default());
        w.insert_resource(crate::game::ScheduleConfigResource(
            crate::config::ScheduleConfig::runtime_baseline(),
        ));
        spawn_connected_test_player(&mut w, x, 56);
        w.resource::<crate::game::GranularWakeQueue>()
            .seed_region(x, 56);
        let mut sched = Schedule::default();
        sched.add_systems(super::granular_physics_system);
        for _ in 0..30 {
            sched.run(&mut w);
        }

        assert_eq!(
            world.get_cell(x, 60),
            cell_type::GOLDEN_ROAD,
            "песок СЪЕЛ золотую дорогу (блоклист {{0,36,37,39}} не сработал)"
        );
        assert_eq!(
            world.get_cell(x, 59),
            SAND,
            "песок не сел на золотую дорогу"
        );
    }
}
