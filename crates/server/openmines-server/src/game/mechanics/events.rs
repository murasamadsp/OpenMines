use crate::db::SkillSlots;
use crate::game::skills::{SkillType, add_skill_exp, skill_progress_payload};
use crate::protocol::packets::skills_packet;
use num_traits::ToPrimitive;

// ─── ActiveEvent / ActiveEvents ───────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActiveEvent {
    pub id: String,
    pub title: String,
    pub starts_at: i64,
    pub ends_at: i64,
    pub xp_mult: f64,
    pub drop_mult: f64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ActiveEvents {
    pub list: Vec<ActiveEvent>,
}

impl ActiveEvents {
    fn get_xp_multiplier(&self, now: i64) -> f64 {
        self.list
            .iter()
            .filter(|e| now >= e.starts_at && now < e.ends_at)
            .fold(1.0, |acc, e| acc * e.xp_mult)
    }

    fn get_drop_multiplier(&self, now: i64) -> f64 {
        self.list
            .iter()
            .filter(|e| now >= e.starts_at && now < e.ends_at)
            .fold(1.0, |acc, e| acc * e.drop_mult)
    }
}

// ─── ExpContext ───────────────────────────────────────────────────────────────
//
// Единственная точка применения ивент-мультипликаторов.
// Хендлеры (dig, build, move, heal, death) создают его один раз и используют
// ctx.add_skill_exp / ctx.apply_drop — никакого знания об ивентах снаружи.

/// Контекст ивентных множителей для одного игрового действия.
/// Создаётся ДО замыканий ECS, чтобы не было borrow-конфликтов.
#[derive(Clone, Copy)]
pub struct ExpContext {
    pub xp_mult: f32,
    pub drop_mult: f64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SkillProgressSnapshot {
    pub entries: Vec<(String, i32)>,
}

impl ExpContext {
    /// Вычислить контекст из текущего состояния ивентов.
    pub fn from_state(state: &crate::game::GameState) -> Self {
        let now = crate::time::now_unix();
        let events = state.active_events.read();
        Self {
            xp_mult: events.get_xp_multiplier(now).to_f32().unwrap_or(1.0),
            drop_mult: events.get_drop_multiplier(now),
        }
    }

    /// Добавить XP скилла с учётом ивент-множителя.
    /// Возвращает `Some(payload)` если уровень или pct изменились (для отправки `@S`).
    pub fn add_skill_exp(
        &self,
        states: &mut SkillSlots,
        code: &str,
        base: f32,
    ) -> Option<(&'static str, Vec<u8>)> {
        if add_skill_exp(states, code, base * self.xp_mult) {
            let payload = skill_progress_payload(states);
            Some(skills_packet(&payload))
        } else {
            None
        }
    }

    /// Add experience through a domain skill type and return a wire-free snapshot.
    pub fn add_typed_skill_exp(
        &self,
        states: &mut SkillSlots,
        skill: SkillType,
        base: f32,
    ) -> Option<SkillProgressSnapshot> {
        add_skill_exp(states, skill.code(), base * self.xp_mult).then(|| SkillProgressSnapshot {
            entries: skill_progress_payload(states),
        })
    }

    /// Применить drop-множитель к базовому количеству кристаллов.
    pub fn apply_drop(&self, base_amount: i64) -> i64 {
        base_amount
            .to_f64()
            .and_then(|base| (base * self.drop_mult).trunc().to_i64())
            .unwrap_or(base_amount)
    }
}
