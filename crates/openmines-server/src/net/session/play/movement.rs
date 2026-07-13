//! Движение робота по миру и рассылка HB соседям.
//! Референс: C# `Player.Move` — БЕЗ серверного cooldown внутри Move
//! (тайминг движения клиентский, `SpeedPacket`). Серверный silent-drop
//! cooldown ломал client-prediction → rubber-band, убран (1:1 C#).
use crate::game::buildings::PackType;
use crate::game::player::{PlayerFlags, PlayerPosition, PlayerStats};
use crate::net::session::prelude::*;

/// Исход `Move` внутри ECS-лока. `Autodig` сигнализирует, что нужно копнуть
/// ПОСЛЕ освобождения лока (`handle_dig` сам берёт `modify_player` —
/// реентрантность лока недопустима).
enum MoveOutcome {
    Moved {
        nx: i32,
        ny: i32,
        ndir: i32,
        skin: i32,
        clan: i32,
    },
    Autodig(i32),
}

fn send_move_state_error(tx: &dyn PacketSink) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ДВИЖЕНИЕ", "Состояние игрока недоступно.").1,
    );
}

enum MoveFollowup {
    Autodig(i32),
    OpenPack(crate::game::structures::buildings::PackView),
}

#[derive(Default)]
struct MoveApplication {
    movement_fanout: Option<crate::net::session::play::chunks::ChunkFanout>,
    chunk_packets: Vec<Vec<u8>>,
    chunk_fanouts: Vec<crate::net::session::play::chunks::ChunkFanout>,
    followup: Option<MoveFollowup>,
}

#[derive(Clone, Copy)]
pub struct MoveRequest {
    pub target_x: i32,
    pub target_y: i32,
    pub direction: i32,
    pub programmatic: bool,
}

fn apply_move(
    state: &Arc<GameState>,
    tx: &dyn PacketSink,
    pid: PlayerId,
    request: MoveRequest,
) -> MoveApplication {
    let MoveRequest {
        target_x,
        target_y,
        direction,
        programmatic,
    } = request;
    // `programmatic` пропускает guards «программа бежит»/«окно открыто», но
    // coord/коллизия/ворота/дистанция остаются общими с ручным ходом.
    // C# `Player.cs:414-415`: Ctrl-move шлёт `(dir+10)` — снять флаг (клиент
    // `ClientController.cs:1022`). `dir==-1` (autoDig-в-стену) проходит без изменений.
    let dir = if direction > 9 {
        direction - 10
    } else {
        direction
    };
    let ctx = crate::game::ExpContext::from_state(state);

    let tp_back = |reason: &str,
                   txc: &dyn PacketSink,
                   from_x: i32,
                   from_y: i32,
                   to_x: i32,
                   to_y: i32,
                   extra: &str| {
        tracing::debug!(
            "[Move] TP back reason={reason} pid={pid} from=({from_x},{from_y}) to=({to_x},{to_y}) {extra}"
        );
        if !programmatic {
            send_u_packet(txc, "@T", &tp(from_x, from_y).1);
        }
    };

    let result = state
        .modify_player(pid, |ecs, entity| {
            // 1. Immutable data gathering
            let (px, py, skin, clan, window_open, manual_control_allowed, auto_dig) = {
                let Some(pos) = ecs.get::<PlayerPosition>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerPosition", "Player component missing for movement");
                    send_move_state_error(tx);
                    return None;
                };
                let Some(player_stats) = ecs.get::<PlayerStats>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerStats", "Player component missing for movement");
                    send_move_state_error(tx);
                    return None;
                };
                let Some(ui) = ecs.get::<crate::game::player::PlayerUI>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerUI", "Player component missing for movement");
                    send_move_state_error(tx);
                    return None;
                };
                let Some(prog) = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                else {
                    tracing::error!(player_id = %pid, component = "ProgrammatorState", "Player component missing for movement");
                    send_move_state_error(tx);
                    return None;
                };
                let Some(settings) = ecs.get::<crate::game::player::PlayerSettings>(entity) else {
                    tracing::error!(player_id = %pid, component = "PlayerSettings", "Player component missing for movement");
                    send_move_state_error(tx);
                    return None;
                };
                (
                    pos.x,
                    pos.y,
                    player_stats.skin,
                    player_stats.clan_id.unwrap_or(0),
                    ui.current_window.is_some(),
                    prog.is_manual_control_allowed(),
                    settings.auto_dig,
                )
            };

            // 2. 1:1 C# `Player.Move`: ВНУТРИ Move НЕТ серверного cooldown.
            // Тайминг движения — клиентский (SpeedPacket pacing). Серверный
            // silent-drop cooldown (re-added рефактором) ломал client-
            // prediction: при любом стопоре tick'а очередь Xmov батчилась,
            // лишние ходы тихо дропались, клиент уходил вперёд → dist>порог
            // → жёсткий @T rubber-band. Убрано (1:1 C# Move + server/
            // CLAUDE.md методология «rate-limit делает клиент» + ae637b9).
            // C# `Move`: `(win != null && !prog)` → tp; `TryAct`:
            // ProgRunning → silent return.
            if !programmatic && !manual_control_allowed {
                return None;
            }
            if window_open && !programmatic {
                tracing::debug!(
                    player_id = %pid,
                    x = px,
                    y = py,
                    reason = "window_open",
                    "Movement rejected"
                );
                tp_back("window", tx, px, py, target_x, target_y, "");
                return None;
            }

            // 3. Movement validation
            if !state.world.valid_coord(target_x, target_y) {
                tp_back("invalid_coord", tx, px, py, target_x, target_y, "");
                return None;
            }

            // Gate check must happen before generic obstacle check: Gate cell 30
            // is not empty, but same-clan players pass through it in C#.
            let gate_at_target = if let Some((ox, oy)) =
                state.find_pack_covering_in_ecs(ecs, target_x, target_y)
                && let Some(bld_entity) = state.building_entity_at(ox, oy)
            {
                if let (Some(meta), Some(ownership)) = (
                    ecs.get::<crate::game::BuildingMetadata>(bld_entity),
                    ecs.get::<crate::game::BuildingOwnership>(bld_entity),
                ) {
                    if meta.pack_type == PackType::Gate {
                        if ownership.clan_id != clan {
                            tracing::debug!(
                                player_id = %pid,
                                gate_clan = ownership.clan_id,
                                player_clan = clan,
                                x = px,
                                y = py,
                                dest_x = target_x,
                                dest_y = target_y,
                                reason = "gate",
                                "Movement rejected"
                            );
                            tp_back(
                                "gate",
                                tx,
                                px,
                                py,
                                target_x,
                                target_y,
                                &format!("pack_clan={} player_clan={clan}", ownership.clan_id),
                            );
                            return None;
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !gate_at_target && !state.world.is_empty(target_x, target_y) {
                let cell = state.world.get_cell_typed(target_x, target_y);
                tracing::debug!(
                    player_id = %pid,
                    cell = cell.0,
                    x = px,
                    y = py,
                    dest_x = target_x,
                    dest_y = target_y,
                    reason = "obstacle",
                    "Movement rejected"
                );
                tp_back(
                    "not_empty",
                    tx,
                    px,
                    py,
                    target_x,
                    target_y,
                    &format!("cell={}", cell.0),
                );
                // 1:1 C# `Player.cs:429-437`: непустая клетка + `dir==-1` + autoDig →
                // tp назад и копнуть (`Bz`). Направление копки — из дельты (как this.dir
                // в C# `Player.cs:416-417`), совпадает с `dir_offset`. Иначе просто tp.
                if dir == -1 && auto_dig {
                    let dig_dir = if px > target_x {
                        1
                    } else if px < target_x {
                        3
                    } else if py > target_y {
                        2
                    } else {
                        0
                    };
                    return Some(MoveOutcome::Autodig(dig_dir));
                }
                return None;
            }

            let dx = (target_x - px) as f32;
            let dy = (target_y - py) as f32;
            let dist = dx.hypot(dy);
            // 1:1 C# `Player.cs:441`: `if (Distance < 1.2f) accept else tp`.
            // Безопасно теперь, когда cooldown-дропов нет (сервер
            // обрабатывает каждый ход → dist всегда ~1.0 при честной игре).
            if dist >= 1.2 {
                tracing::debug!(
                    player_id = %pid,
                    dist,
                    x = px,
                    y = py,
                    dest_x = target_x,
                    dest_y = target_y,
                    reason = "distance",
                    "Movement rejected"
                );
                tp_back(
                    "dist",
                    tx,
                    px,
                    py,
                    target_x,
                    target_y,
                    &format!("dist={dist:.3}"),
                );
                return None;
            }

            // 1:1 C# `Player.cs:416-418`: позиция меняется (или dir==-1) →
            // направление из дельты реального хода; иначе — присланный dir.
            let actual_dir = if dir == -1 || px != target_x || py != target_y {
                if px > target_x {
                    1
                } else if px < target_x {
                    3
                } else if py > target_y {
                    2
                } else {
                    0
                }
            } else {
                dir
            };

            // 4. State updates
            {
                let mut pos_mut = ecs.get_mut::<PlayerPosition>(entity)?;
                pos_mut.x = target_x;
                pos_mut.y = target_y;
                pos_mut.dir = actual_dir;
            }
            {
                let mut flags_mut = ecs.get_mut::<PlayerFlags>(entity)?;
                flags_mut.dirty = true;
            }

            {
                let mut skills = ecs.get_mut::<crate::game::player::PlayerSkillsComp>(entity)?;
                if let Some(sk) = ctx.add_skill_exp(&mut skills.states, "M", 1.0) {
                    send_u_packet(tx, sk.0, &sk.1);
                }
            }

            Some(MoveOutcome::Moved {
                nx: target_x,
                ny: target_y,
                ndir: actual_dir,
                skin,
                clan,
            })
        })
        .flatten();

    let (nx, ny, ndir, skin, clan) = match result {
        Some(MoveOutcome::Moved {
            nx,
            ny,
            ndir,
            skin,
            clan,
        }) => (nx, ny, ndir, skin, clan),
        Some(MoveOutcome::Autodig(dig_dir)) => {
            return MoveApplication {
                followup: Some(MoveFollowup::Autodig(dig_dir)),
                ..MoveApplication::default()
            };
        }
        None => return MoveApplication::default(),
    };

    let tail = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                .map_or(0, |ps| u8::from(ps.running))
        })
        .unwrap_or(0);
    let (cx, cy) = World::chunk_pos(nx, ny);
    let bot = hb_bot(
        net_u16_nonneg(pid),
        net_u16_nonneg(nx),
        net_u16_nonneg(ny),
        net_u8_clamped(ndir, 3),
        net_u8_clamped(skin, 255),
        net_u16_nonneg(clan),
        tail,
    );
    let movement_fanout = crate::net::session::play::chunks::ChunkFanout {
        recipients: state.nearby_session_ids(cx, cy, None),
        data: encode_hb_bundle(&hb_bundle(&[bot]).1),
    };

    let chunk_packets = crate::net::session::wire::PacketBatch::default();
    let chunk_fanouts =
        crate::net::session::play::chunks::prepare_chunk_changed(state, &chunk_packets, pid);

    // C# `World.AddPack` регистрирует pack только в origin-клетке. Проверка
    // footprint здесь была регрессией: road-клетки Resp открывали GUI.
    let followup = state.get_pack_at(nx, ny).and_then(|view| {
        (tail == 0 && (view.clan_id == 0 || view.clan_id == clan))
            .then_some(MoveFollowup::OpenPack(view))
    });

    MoveApplication {
        movement_fanout: Some(movement_fanout),
        chunk_packets: chunk_packets.into_packets(),
        chunk_fanouts,
        followup,
    }
}

fn deliver_move_application(
    state: &Arc<GameState>,
    tx: &Outbox,
    application: &mut MoveApplication,
) {
    if let Some(fanout) = application.movement_fanout.take() {
        state.sessions.fanout(&fanout.recipients, &fanout.data);
    }
    for packet in application.chunk_packets.drain(..) {
        if tx.send(packet).is_err() {
            break;
        }
    }
    for fanout in application.chunk_fanouts.drain(..) {
        state.sessions.fanout(&fanout.recipients, &fanout.data);
    }
}

fn run_move_followup(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    followup: MoveFollowup,
    programmatic: bool,
) {
    match followup {
        MoveFollowup::Autodig(direction) => {
            crate::net::session::play::dig_build::handle_dig(
                state,
                tx,
                pid,
                direction,
                programmatic,
            );
        }
        MoveFollowup::OpenPack(view) => {
            crate::net::session::ui::gui_buttons::open_pack_gui(state, tx, pid, &view);
        }
    }
}

pub fn apply_move_command(
    state: &Arc<GameState>,
    pid: PlayerId,
    session_id: crate::game::SessionId,
    request: MoveRequest,
) -> crate::game::CommandEffects {
    if state
        .active_player_entity_for_session(pid, session_id)
        .is_none()
    {
        return crate::game::CommandEffects::default();
    }
    let Some(tx) = state.sessions.outbox_for_session(session_id) else {
        return crate::game::CommandEffects::default();
    };

    let direct_packets = crate::net::session::wire::PacketBatch::default();
    let mut application = apply_move(state, &direct_packets, pid, request);
    let direct_packets = direct_packets.into_packets();

    if let Some(followup) = application.followup.take() {
        // Пока auto-dig/open-pack сами не возвращают effects, весь редкий путь
        // доставляется синхронно. Иначе GUI/dig output обгонит queued move output.
        for packet in direct_packets {
            if tx.send(packet).is_err() {
                return crate::game::CommandEffects::default();
            }
        }
        deliver_move_application(state, &tx, &mut application);
        run_move_followup(state, &tx, pid, followup, request.programmatic);
        return crate::game::CommandEffects::default();
    }

    let mut effects = crate::game::CommandEffects::default();
    if !direct_packets.is_empty() {
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id: pid,
            packets: direct_packets,
        });
    }
    if let Some(fanout) = application.movement_fanout {
        effects.events.push(crate::game::GameEvent::Fanout {
            recipients: fanout.recipients,
            data: fanout.data,
        });
    }
    if !application.chunk_packets.is_empty() {
        effects.events.push(crate::game::GameEvent::SessionBatch {
            session_id,
            player_id: pid,
            packets: application.chunk_packets,
        });
    }
    effects
        .events
        .extend(application.chunk_fanouts.into_iter().map(|fanout| {
            crate::game::GameEvent::Fanout {
                recipients: fanout.recipients,
                data: fanout.data,
            }
        }));
    effects
}

#[allow(clippy::too_many_arguments)]
pub fn handle_move(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    _client_time: u32,
    target_x: i32,
    target_y: i32,
    dir: i32,
    programmatic: bool,
) {
    let request = MoveRequest {
        target_x,
        target_y,
        direction: dir,
        programmatic,
    };
    let mut application = apply_move(state, tx, pid, request);
    deliver_move_application(state, tx, &mut application);
    if let Some(followup) = application.followup {
        run_move_followup(state, tx, pid, followup, programmatic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ServerTestHarness, ServerTestHarnessBuilder, drain_events};

    async fn make_test_state(label: &str) -> ServerTestHarness {
        let mut builder = ServerTestHarnessBuilder::new(label, "move-user").await;
        builder.player.x = 10;
        builder.player.y = 10;
        builder.build().await
    }

    #[tokio::test]
    async fn stale_session_move_cannot_mutate_reconnected_player() {
        let test = make_test_state("stale_session_move").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let pid = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            crate::game::PlayerCommand::Move {
                player_id: pid,
                session_id: crate::game::SessionId::new(2),
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert!(effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert_eq!(
            test.state.query_player(pid, |ecs, entity| {
                let position = ecs.get::<PlayerPosition>(entity)?;
                Some((position.x, position.y))
            }),
            Some(Some((10, 10)))
        );
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn manual_move_returns_output_without_sending_during_dispatch() {
        let test = make_test_state("manual_move_effect").await;
        test.state.world.set_cell(11, 10, cell_type::EMPTY);
        let session_id = crate::game::SessionId::new(1);
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let pid = PlayerId(test.player.id);

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            crate::game::PlayerCommand::Move {
                player_id: pid,
                session_id,
                time: 0,
                x: 11,
                y: 10,
                direction: 3,
                programmatic: false,
            },
        );

        assert_eq!(
            test.state.query_player(pid, |ecs, entity| {
                let position = ecs.get::<PlayerPosition>(entity)?;
                Some((position.x, position.y))
            }),
            Some(Some((11, 10)))
        );
        assert!(!effects.events.is_empty());
        assert!(effects.saves.is_empty());
        assert!(rx.try_recv().is_err(), "dispatch must not write to outbox");

        for event in effects.events {
            match event {
                crate::game::GameEvent::PlayerInit { .. } => {
                    panic!("ordinary move cannot produce player init")
                }
                crate::game::GameEvent::SessionBatch {
                    session_id,
                    player_id,
                    packets,
                } => crate::net::session::player::init::deliver_initial_presentation(
                    &test.state,
                    session_id,
                    player_id,
                    packets,
                ),
                crate::game::GameEvent::Fanout { recipients, data } => {
                    test.state.sessions.fanout(&recipients, &data);
                }
                crate::game::GameEvent::GuiView { .. }
                | crate::game::GameEvent::ChatFanout { .. } => {
                    panic!("ordinary move cannot produce this event")
                }
            }
        }
        assert!(
            drain_events(&mut rx).iter().any(|(event, _)| event == "HB"),
            "presentation delivery must emit the movement HB"
        );
    }

    #[tokio::test]
    async fn move_missing_position_is_explicit_error_not_silent_reject() {
        let test = make_test_state("move_missing_position").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity).remove::<PlayerPosition>();
        }

        handle_move(&test.state, &tx, pid, 0, 11, 10, 3, false);

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние игрока недоступно."));
    }

    #[tokio::test]
    async fn move_distance_reject_stays_tp_back_without_state_error() {
        let test = make_test_state("move_distance_reject").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            15,
            10,
            3,
            false,
        );

        // @T is a legitimate gameplay reject. No OK state error should appear.
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "@T");
        assert_eq!(events[0].1, b"10:10");
    }

    #[tokio::test]
    async fn programmatic_move_reject_does_not_send_tp_back() {
        let test = make_test_state("programmatic_move_distance_reject").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            15,
            10,
            3,
            true,
        );

        let events = drain_events(&mut rx);
        assert!(
            events.iter().all(|(event, _)| event != "@T"),
            "server-driven programmator move must not rubber-band the client"
        );
    }

    #[tokio::test]
    async fn programmatic_autodig_sends_self_hb_without_tp_back() {
        let test = make_test_state("programmatic_autodig_self_hb").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?
                .auto_dig = true;
            Some(())
        });
        test.state.world.set_cell(10, 11, cell_type::ROCK);
        test.state.world.set_durability(10, 11, 0.0);

        handle_move(&test.state, &tx, pid, 0, 10, 11, -1, true);

        assert_eq!(test.state.world.get_cell(10, 11), cell_type::EMPTY);
        let events = drain_events(&mut rx);
        assert!(events.iter().all(|(event, _)| event != "@T"));
        assert!(
            events.iter().any(|(event, _)| event == "HB"),
            "programmatic autodig must notify the owning client through HB"
        );
    }

    #[tokio::test]
    async fn moving_onto_own_gate_sends_gu_close_like_reference() {
        let mut test = make_test_state("own_gate_sends_gu").await;
        test.player.clan_id = Some(7);

        let extra = crate::db::BuildingExtra {
            charge: 0,
            max_charge: 0,
            cost: 0,
            hp: 0,
            max_hp: 0,
            money_inside: 0,
            crystals_inside: [0; 6],
            items_inside: std::collections::HashMap::new(),
            craft_recipe_id: None,
            craft_num: 0,
            craft_end_ts: 0,
            craft_ready: false,
            clanzone: 0,
        };
        let spec = crate::game::BuildingInsertSpec {
            type_code: " ",
            pack_type: PackType::Gate,
            x: 11,
            y: 10,
            owner_id: PlayerId(test.player.id),
            clan_id: 7,
            extra: &extra,
        };
        test.state.insert_building_runtime(&spec).await.unwrap();

        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        handle_move(
            &test.state,
            &tx,
            PlayerId(test.player.id),
            0,
            11,
            10,
            3,
            false,
        );

        let events = drain_events(&mut rx);
        assert!(
            events
                .iter()
                .any(|(event, payload)| event == "Gu" && payload == b"_"),
            "own Gate entry must close GUI with Gu like C# SendWindow(null), events: {events:?}"
        );
        assert!(
            events.iter().all(|(event, _)| event != "GU"),
            "Gate.GUIWin returns null; it must not open a GU window, events: {events:?}"
        );
    }
}
