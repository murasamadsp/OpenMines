//! Мелкие обработчики: auto-dig, whoi, программатор TY, настройки.
use crate::net::session::prelude::*;
use crate::protocol::packets::open_programmator;

pub fn handle_auto_dig_toggle(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let new_val = state
        .modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            let mut settings = ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?;
            settings.auto_dig = !settings.auto_dig;
            let val = settings.auto_dig;
            if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                flags.dirty = true;
            }
            Some(val)
        })
        .flatten();

    if let Some(val) = new_val {
        send_u_packet(tx, "BD", &auto_digg(val).1);
    }
}

/// Set auto-dig to a specific value (used by programmator `EnableAutoDig`/`DisableAutoDig`).
pub fn handle_auto_dig_set(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    enabled: bool,
) {
    let changed = state
        .modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            let mut settings = ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?;
            if settings.auto_dig != enabled {
                settings.auto_dig = enabled;
                if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                    flags.dirty = true;
                }
                Some(enabled)
            } else {
                None
            }
        })
        .flatten();

    if let Some(val) = changed {
        send_u_packet(tx, "BD", &auto_digg(val).1);
    }
}

/// TY программатор — как `Session.PROG/PDEL/pRST/PREN` + `StaticGUI` в `server_reference`.
pub async fn handle_prog_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    event: &str,
    payload: &[u8],
) {
    match event {
        "PROG" => {
            let decoded = crate::game::programmator::ProgrammatorState::decode_prog_packet(payload);
            if let Some((prog_id, source)) = decoded {
                if let Err(e) = state.db.save_program(pid, prog_id, &source).await {
                    tracing::warn!("[PROG] DB save failed pid={pid} prog_id={prog_id}: {e:#}");
                }

                let running = state
                    .modify_player(pid, |ecs, entity| {
                        if let Some(mut ps) =
                            ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                        {
                            ps.selected_id = Some(prog_id);
                            ps.selected_data = Some(source.clone());
                            ps.run_program(&source);
                            Some(ps.running)
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .unwrap_or(false);

                tracing::info!(
                    "PROGDIAG PROG pid={pid} id={prog_id} source_len={} running={running}",
                    source.len()
                );

                // C# StartedProg: RunProgramm (Gu close) → UpdateProg (#p) → ProgStatus (@P).
                send_u_packet(tx, "Gu", &gu_close().1);
                let name = state
                    .db
                    .get_program(prog_id)
                    .await
                    .ok()
                    .flatten()
                    .filter(|p| p.player_id == pid) // ownership: блок IDOR (чужой prog_id)
                    .map(|p| p.name)
                    .unwrap_or_default();
                send_u_packet(tx, "#p", &open_programmator(prog_id, &name, &source).1);
                send_u_packet(tx, "@P", &programmator_status(running).1);
                if !running {
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message(
                            "ПРОГРАММАТОР",
                            "Программа сохранена, но не запущена: проверьте команды и метки.",
                        )
                        .1,
                    );
                }
            } else {
                tracing::warn!(
                    "PROGDIAG PROG decode FAILED pid={pid} len={}",
                    payload.len()
                );
                send_u_packet(tx, "@P", &programmator_status(false).1);
                send_u_packet(
                    tx,
                    "OK",
                    &ok_message("ПРОГРАММАТОР", "Не удалось прочитать программу.").1,
                );
            }
        }
        "pRST" => {
            // C# ref Session.Prst:
            //   if (selected != null && !ProgRunning) → OpenProg (send #P editor)
            //   if (ProgRunning) → RunProgramm() (stops it)
            //   then ProgStatus()
            let result = state
                .modify_player(pid, |ecs, entity| {
                    let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                    let was_running = ps.running;
                    let open_editor = !was_running && ps.selected_id.is_some();
                    let editor_data = if open_editor {
                        ps.selected_id.zip(ps.selected_data.clone())
                    } else {
                        None
                    };
                    if was_running {
                        ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?
                            .running = false;
                    }
                    Some((false, editor_data, was_running))
                })
                .flatten();
            let (running, editor_data, was_running) = result.unwrap_or((false, None, false));
            // C# Prst: if (selected && !running) OpenProg (#P); if (running) RunProgramm (Gu close);
            // затем ProgStatus (@P). Ветки взаимоисключающие.
            if let Some((prog_id, source)) = editor_data {
                let name = state
                    .db
                    .get_program(prog_id)
                    .await
                    .ok()
                    .flatten()
                    .filter(|p| p.player_id == pid) // ownership: блок IDOR (чужой prog_id)
                    .map(|p| p.name)
                    .unwrap_or_default();
                send_u_packet(tx, "#P", &open_programmator(prog_id, &name, &source).1);
            }
            if was_running {
                // RunProgramm() закрывает окно перед остановкой.
                send_u_packet(tx, "Gu", &gu_close().1);
            }
            send_u_packet(tx, "@P", &programmator_status(running).1);
        }
        "PDEL" => {
            if let Ok(id_str) = std::str::from_utf8(payload) {
                if let Ok(prog_id) = id_str.trim().parse::<i32>() {
                    if let Err(e) = state.db.delete_program_owned(pid, prog_id).await {
                        tracing::warn!("[PDEL] DB delete failed pid={pid} id={prog_id}: {e:#}");
                    }
                    state.modify_player(pid, |ecs, entity| {
                        if let Some(mut ps) =
                            ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                        {
                            if ps.selected_id == Some(prog_id) {
                                ps.running = false;
                                ps.selected_id = None;
                                ps.selected_data = None;
                            }
                        }
                        Some(())
                    });
                }
            }
            // C# Pdel: StaticGUI.DeleteProg() только удаляет из БД — НИ одного пакета
            // (ProgStatus не вызывается). In-memory сброс selected/running оставлен как
            // безопасность (wire-невидим), сам @P убран для паритета.
        }
        "PCOP" => {
            let prog_id = std::str::from_utf8(payload)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok());
            if let Some(id) = prog_id {
                match state.db.get_program(id).await {
                    Ok(Some(program)) if program.player_id == pid => {
                        let name = format!("{} (copy)", program.name);
                        if let Err(e) = state.db.insert_program(pid, &name, &program.code).await {
                            tracing::warn!("[PCOP] DB copy failed pid={pid} id={id}: {e:#}");
                        }
                    }
                    Ok(Some(program)) => {
                        tracing::warn!(
                            "[PCOP] rejected foreign program copy pid={pid} id={id} owner={}",
                            program.player_id
                        );
                    }
                    Ok(None) => {
                        tracing::warn!("[PCOP] missing program pid={pid} id={id}");
                    }
                    Err(e) => {
                        tracing::warn!("[PCOP] DB get failed pid={pid} id={id}: {e:#}");
                    }
                }
            }
            crate::net::session::social::buildings::handle_programmator_pope_menu(state, tx, pid)
                .await;
        }
        "PREN" => {
            // C# ref Session.cs:150 `StaticGUI.Rename(player, pren.Id)` — открывает
            // диалог переименования с полем ввода. pren.Id — ID программы из payload.
            let prog_id = std::str::from_utf8(payload)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok());
            if let Some(id) = prog_id {
                use crate::net::session::ui::horb::{Button, Horb};

                Horb::new("ПЕРЕИМЕНОВАТЬ")
                    .text("Введите новое название программы")
                    .input("Название программы...", true)
                    .button(Button::new("OK", format!("rename:{id}:%I%")))
                    .close_button()
                    .send(state, tx, pid, format!("pren:{id}"));
            } else {
                let running = state
                    .query_player(pid, |ecs, entity| {
                        ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                            .is_some_and(|ps| ps.running)
                    })
                    .unwrap_or(false);
                send_u_packet(tx, "@P", &programmator_status(running).1);
            }
        }
        _ => {}
    }
}

/// TY `Sett` → `Settings.SendSettingsGUI` в `server_reference` — 1:1 с C# `RichList`.
pub fn handle_sett_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    _payload: &[u8],
) {
    use crate::net::session::ui::horb::{Button, Horb, RichRow, Tab};

    let settings = state.query_player_opt(pid, |ecs, entity| {
        ecs.get::<crate::game::player::PlayerSettings>(entity)
            .copied()
    });
    let Some(s) = settings else { return };

    let has_clan = state
        .query_player_opt(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerStats>(entity)
                .map(|st| st.clan_id.unwrap_or(0) != 0)
        })
        .unwrap_or(false);

    let scale_values = "0:мелко#1:КРУПНО#";
    let mut win = Horb::new("Настройки")
        .tab(Tab::active("Настройки"))
        .rich_row(RichRow::dropdown(
            "Масштаб интерфейса",
            scale_values,
            "isca",
            i64::from(s.isca),
        ))
        .rich_row(RichRow::dropdown(
            "Масштаб территории",
            scale_values,
            "tsca",
            i64::from(s.tsca),
        ))
        .rich_row(RichRow::toggle(
            "Включить управление мышкой",
            "mous",
            s.mous,
        ))
        .rich_row(RichRow::toggle("Упрощённый режим графики", "pot", s.pot))
        .rich_row(RichRow::toggle(
            "Принудительно обновлять породы (увеличит потр. CPU)",
            "frc",
            s.frc,
        ))
        .rich_row(RichRow::toggle(
            "CTRL переключает скорость робота (вместо удерживания)",
            "ctrl",
            s.ctrl,
        ))
        .rich_row(RichRow::toggle("Отключить ближайшие звуки", "mof", s.mof))
        .button(Button::new("Сохранить", "save:%R%"));

    if !has_clan {
        win = win.button(Button::new("Создать клан", "clancreate"));
    }

    win.button(Button::new("Выйти", "exit"))
        .send(state, tx, pid, "settings");
}

pub async fn handle_whoi(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, ids: &[i32]) {
    let mut parts = Vec::new();
    for &id in ids {
        let mut name_opt = state.query_player_opt(id, |ecs: &bevy_ecs::prelude::World, entity| {
            ecs.get::<crate::game::player::PlayerMetadata>(entity)
                .map(|m| m.name.clone())
        });
        if name_opt.is_none() {
            if let Ok(Some(p)) = state.db.get_player_by_id(id).await {
                name_opt = Some(p.name);
            }
        }
        let name = name_opt.unwrap_or_default();
        parts.push(format!("{id}:{name}"));
    }
    send_u_packet(tx, "NL", parts.join(",").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedReceiver;

    struct TestState {
        state: Arc<GameState>,
        player: crate::db::PlayerRow,
        world_name: String,
        db_path: std::path::PathBuf,
    }

    async fn make_test_state(label: &str) -> TestState {
        let dir = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let db_path = dir.join(format!("{label}_{}_{}.db", std::process::id(), nonce));
        let _ = std::fs::remove_file(&db_path);
        let database = crate::db::Database::open(&db_path).await.unwrap();
        let player = database.create_player("prog-user", "p", "h").await.unwrap();

        let cell_defs = crate::world::cells::CellDefs::load("configs/cells.json").unwrap();
        let world_name = format!("{label}_world_{}_{}", std::process::id(), nonce);
        let world = crate::world::World::new(&world_name, 2, 2, cell_defs, &dir).unwrap();
        let config = crate::config::Config {
            world_name: world_name.clone(),
            port: 8090,
            world_chunks_w: 2,
            world_chunks_h: 2,
            data_dir: dir.to_string_lossy().to_string(),
            logging: crate::config::LoggingConfig::default(),
            cron: crate::config::CronConfig::default(),
            gameplay: crate::config::GameplayConfig::default(),
        };

        let state = crate::game::GameState::new(Arc::new(world), Arc::new(database), config).await;
        TestState {
            state,
            player,
            world_name,
            db_path,
        }
    }

    impl TestState {
        fn cleanup(&self) {
            let dir = std::env::temp_dir();
            let _ = std::fs::remove_file(&self.db_path);
            let _ = std::fs::remove_file(self.db_path.with_extension("db-wal"));
            let _ = std::fs::remove_file(self.db_path.with_extension("db-shm"));
            let _ = std::fs::remove_file(dir.join(format!("{}_v2.map", self.world_name)));
            let _ = std::fs::remove_file(dir.join(format!("{}_durability.mapb", self.world_name)));
        }
    }

    fn prog_payload(compiled_len: i32, prog_id: i32, compiled: &[u8], source: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&compiled_len.to_le_bytes());
        payload.extend_from_slice(&prog_id.to_le_bytes());
        payload.extend_from_slice(compiled);
        payload.extend_from_slice(source.as_bytes());
        payload
    }

    fn drain_events(rx: &mut UnboundedReceiver<Vec<u8>>) -> Vec<(String, Vec<u8>)> {
        let mut events = Vec::new();
        while let Ok(frame) = rx.try_recv() {
            let mut buf = BytesMut::from(&frame[..]);
            let packet = crate::protocol::Packet::try_decode(&mut buf)
                .expect("valid packet")
                .expect("decoded packet");
            events.push((packet.event_str().to_owned(), packet.payload.to_vec()));
        }
        events
    }

    #[tokio::test]
    async fn prog_valid_payload_saves_updates_and_reports_failed_run() {
        let test = make_test_state("prog_state_machine").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "old")
            .await
            .unwrap();
        let payload = prog_payload(3, prog_id, &[1, 2, 3], "");

        handle_prog_ty(&test.state, &tx, test.player.id, "PROG", &payload).await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "#p", "@P", "OK"]);
        assert_eq!(events[0].1, b"_");
        assert_eq!(events[2].1, b"0");

        let update_json: serde_json::Value = serde_json::from_slice(&events[1].1).unwrap();
        assert_eq!(update_json["id"], prog_id);
        assert_eq!(update_json["title"], "main");
        assert_eq!(update_json["source"], "");

        let saved = test.state.db.get_program(prog_id).await.unwrap().unwrap();
        assert_eq!(saved.code, "");

        test.cleanup();
    }

    #[tokio::test]
    async fn prst_reopens_selected_stopped_program_with_uppercase_open_packet() {
        let test = make_test_state("prst_state_machine").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        test.state.modify_player(test.player.id, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(String::new());
            ps.running = false;
            Some(())
        });

        handle_prog_ty(&test.state, &tx, test.player.id, "pRST", b"").await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["#P", "@P"]);
        assert_eq!(events[1].1, b"0");

        let open_json: serde_json::Value = serde_json::from_slice(&events[0].1).unwrap();
        assert_eq!(open_json["id"], prog_id);
        assert_eq!(open_json["title"], "main");
        assert_eq!(open_json["source"], "");

        test.cleanup();
    }

    #[tokio::test]
    async fn pcop_copies_owned_program_and_refreshes_list() {
        let test = make_test_state("pcop_state_machine").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "source")
            .await
            .unwrap();

        handle_prog_ty(
            &test.state,
            &tx,
            test.player.id,
            "PCOP",
            prog_id.to_string().as_bytes(),
        )
        .await;

        let programs = test.state.db.list_programs(test.player.id).await.unwrap();
        assert_eq!(programs.len(), 2);
        assert!(
            programs
                .iter()
                .any(|p| p.name == "main (copy)" && p.code == "source")
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "GU");
        let payload = std::str::from_utf8(&events[0].1).unwrap();
        assert!(payload.starts_with("horb:"));
        assert!(payload.contains("main"));
        assert!(payload.contains("main (copy)"));

        test.cleanup();
    }

    #[tokio::test]
    async fn pcop_rejects_foreign_program_without_copying() {
        let test = make_test_state("pcop_foreign_state_machine").await;
        let foreign = test
            .state
            .db
            .create_player("foreign", "p", "h2")
            .await
            .unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let foreign_prog_id = test
            .state
            .db
            .insert_program(foreign.id, "foreign-main", "foreign-source")
            .await
            .unwrap();

        handle_prog_ty(
            &test.state,
            &tx,
            test.player.id,
            "PCOP",
            foreign_prog_id.to_string().as_bytes(),
        )
        .await;

        assert!(
            test.state
                .db
                .list_programs(test.player.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            test.state.db.list_programs(foreign.id).await.unwrap().len(),
            1
        );
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "GU");

        test.cleanup();
    }

    #[tokio::test]
    async fn pdel_deletes_owned_selected_program_without_wire_response() {
        let test = make_test_state("pdel_state_machine").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        test.state.modify_player(test.player.id, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(String::new());
            ps.running = true;
            Some(())
        });

        handle_prog_ty(
            &test.state,
            &tx,
            test.player.id,
            "PDEL",
            prog_id.to_string().as_bytes(),
        )
        .await;

        assert!(test.state.db.get_program(prog_id).await.unwrap().is_none());
        let state_after = test
            .state
            .query_player_opt(test.player.id, |ecs, entity| {
                let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                Some((ps.selected_id, ps.selected_data.clone(), ps.running))
            })
            .unwrap();
        assert_eq!(state_after, (None, None, false));
        assert!(drain_events(&mut rx).is_empty());

        test.cleanup();
    }

    #[tokio::test]
    async fn pren_opens_typed_rename_dialog() {
        let test = make_test_state("pren_state_machine").await;
        let (tx, mut rx) = mpsc::unbounded_channel();
        crate::net::session::player::init::connect_in_tick(&test.state, &tx, &test.player, 1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        handle_prog_ty(
            &test.state,
            &tx,
            test.player.id,
            "PREN",
            prog_id.to_string().as_bytes(),
        )
        .await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "GU");
        let payload = std::str::from_utf8(&events[0].1).unwrap();
        assert!(payload.starts_with("horb:"));
        assert!(payload.contains("ПЕРЕИМЕНОВАТЬ"));
        assert!(payload.contains(&format!("rename:{prog_id}:%I%")));

        test.cleanup();
    }
}
