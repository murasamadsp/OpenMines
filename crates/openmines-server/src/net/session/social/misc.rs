//! Мелкие обработчики: auto-dig, whoi, программатор TY, настройки.
use crate::net::session::prelude::*;
use crate::protocol::packets::open_programmator;

async fn load_owned_program_name(
    state: &Arc<GameState>,
    pid: PlayerId,
    prog_id: i32,
) -> anyhow::Result<Option<String>> {
    let program = state.db.get_program(prog_id).await?;
    Ok(program
        .filter(|p| p.player_id == pid.as_i32())
        .map(|p| p.name))
}

fn send_programmator_state_error(tx: &Outbox) {
    send_u_packet(
        tx,
        "OK",
        &ok_message("ПРОГРАММАТОР", "Состояние программатора недоступно.").1,
    );
}

fn clear_programmator_window(state: &Arc<GameState>, pid: PlayerId) {
    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
            ui.current_window = None;
        }
        Some(())
    });
}

fn send_programmator_start_position(tx: &Outbox, server_pos: (i32, i32), running: bool) {
    if running {
        send_u_packet(tx, "@T", &tp(server_pos.0, server_pos.1).1);
    }
}

/// TY программатор — как `Session.PROG/PDEL/pRST/PREN` + `StaticGUI` в `server_reference`.
pub async fn handle_prog_ty(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    event: &str,
    payload: &[u8],
) {
    match event {
        "PROG" => {
            let decoded = crate::game::programmator::ProgrammatorState::decode_prog_packet(payload);
            if let Some((prog_id, source)) = decoded {
                if prog_id <= 0 {
                    tracing::warn!(
                        player_id = %pid,
                        program_id = prog_id,
                        "PROG received no selected client program; opening program list"
                    );
                    crate::net::session::social::buildings::handle_programmator_pope_menu(
                        state, tx, pid,
                    )
                    .await;
                    return;
                }
                if let Err(e) = state.db.save_program(pid.into(), prog_id, &source).await {
                    tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB save failed");
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("ПРОГРАММАТОР", "Не удалось сохранить программу.").1,
                    );
                    return;
                }
                if let Err(e) = state
                    .db
                    .set_selected_program(pid.into(), Some(prog_id))
                    .await
                {
                    tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB selected program update failed for PROG");
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("ПРОГРАММАТОР", "Не удалось выбрать программу.").1,
                    );
                    return;
                }
                let prog_name = match load_owned_program_name(state, pid, prog_id).await {
                    Ok(Some(name)) => name,
                    Ok(None) => {
                        tracing::error!(player_id = %pid, program_id = prog_id, "Saved program is missing after PROG save");
                        send_u_packet(
                            tx,
                            "OK",
                            &ok_message("ПРОГРАММАТОР", "Программа недоступна.").1,
                        );
                        return;
                    }
                    Err(e) => {
                        tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB get failed after PROG save");
                        send_u_packet(
                            tx,
                            "OK",
                            &ok_message("ПРОГРАММАТОР", "Не удалось прочитать программу.").1,
                        );
                        return;
                    }
                };
                apply_saved_program_to_tick_state(state, tx, pid, prog_id, &prog_name, &source);
            } else {
                tracing::warn!(
                    player_id = %pid,
                    len = payload.len(),
                    "PROGDIAG PROG decode FAILED"
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
            handle_prog_reset_ty(state, tx, pid);
        }
        "PDEL" => {
            if let Ok(id_str) = std::str::from_utf8(payload) {
                if let Ok(prog_id) = id_str.trim().parse::<i32>() {
                    match state.db.delete_program_owned(pid.into(), prog_id).await {
                        Ok(true) => {}
                        Ok(false) => {
                            tracing::warn!(
                                player_id = %pid,
                                program_id = prog_id,
                                "Program delete rejected: missing or foreign row"
                            );
                            send_u_packet(
                                tx,
                                "OK",
                                &ok_message("ПРОГРАММАТОР", "Программа не найдена.").1,
                            );
                            return;
                        }
                        Err(e) => {
                            tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB delete failed");
                            send_u_packet(
                                tx,
                                "OK",
                                &ok_message("ПРОГРАММАТОР", "Не удалось удалить программу.").1,
                            );
                            return;
                        }
                    }
                    let cleared_selected = clear_deleted_program_runtime(state, pid, prog_id);
                    if cleared_selected
                        && let Err(e) = state.db.set_selected_program(pid.into(), None).await
                    {
                        tracing::error!(player_id = %pid, program_id = prog_id, error = ?e, "DB selected program clear failed after delete");
                    }
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
                    Ok(Some(program)) if program.player_id == pid.as_i32() => {
                        let name = format!("{} (copy)", program.name);
                        if let Err(e) = state
                            .db
                            .insert_program(pid.into(), &name, &program.code)
                            .await
                        {
                            tracing::error!(player_id = %pid, program_id = id, error = ?e, "DB copy failed");
                            send_u_packet(
                                tx,
                                "OK",
                                &ok_message("ПРОГРАММАТОР", "Не удалось скопировать программу.").1,
                            );
                            return;
                        }
                    }
                    Ok(Some(program)) => {
                        tracing::warn!(
                            player_id = %pid,
                            program_id = id,
                            owner_id = program.player_id,
                            "Rejected foreign program copy"
                        );
                        send_u_packet(
                            tx,
                            "OK",
                            &ok_message("ПРОГРАММАТОР", "Программа недоступна.").1,
                        );
                        return;
                    }
                    Ok(None) => {
                        tracing::warn!(
                            player_id = %pid,
                            program_id = id,
                            "Missing program for copy"
                        );
                        send_u_packet(
                            tx,
                            "OK",
                            &ok_message("ПРОГРАММАТОР", "Программа не найдена.").1,
                        );
                        return;
                    }
                    Err(e) => {
                        tracing::error!(player_id = %pid, program_id = id, error = ?e, "DB get failed for copy");
                        send_u_packet(
                            tx,
                            "OK",
                            &ok_message("ПРОГРАММАТОР", "Не удалось прочитать программу.").1,
                        );
                        return;
                    }
                }
            } else {
                send_u_packet(
                    tx,
                    "OK",
                    &ok_message("ПРОГРАММАТОР", "Некорректный идентификатор программы.").1,
                );
                return;
            }
            crate::net::session::social::buildings::handle_programmator_pope_menu(state, tx, pid)
                .await;
        }
        "PREN" => {
            handle_prog_rename_prompt_ty(state, tx, pid, payload);
        }
        _ => {}
    }
}

pub fn clear_deleted_program_runtime(state: &Arc<GameState>, pid: PlayerId, prog_id: i32) -> bool {
    state
        .modify_player(pid, |ecs, entity| {
            let cleared = if let Some(mut ps) =
                ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)
                && ps.selected_id == Some(prog_id)
            {
                ps.running = false;
                ps.selected_id = None;
                ps.selected_data = None;
                true
            } else {
                false
            };
            Some(cleared)
        })
        .flatten()
        .unwrap_or(false)
}

pub fn apply_saved_program_to_tick_state(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    prog_id: i32,
    prog_name: &str,
    source: &str,
) {
    let source_owned = source.to_owned();
    let run_state = state
        .modify_player(pid, |ecs, entity| {
            let server_pos = ecs
                .get::<crate::game::player::PlayerPosition>(entity)
                .map(|pos| (pos.x, pos.y))?;
            // На запуске окно программатора закрывается (ниже шлём `Gu`).
            // ОБЯЗАТЕЛЬНО синхронизируем серверный `current_window=None`:
            // иначе после стопа гард `window_open` в `handle_move` режет
            // ручной ход и шлёт `@T` назад («сервер кидает назад»).
            // Pope-меню/окно ставило `current_window`, а сырой `Gu`
            // (send_u_packet) его не сбрасывал.
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = None;
            }
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(source_owned.clone());
            ps.run_program(&source_owned);
            Some((ps.running, server_pos))
        })
        .flatten();

    let Some((running, server_pos)) = run_state else {
        tracing::error!(player_id = %pid, program_id = prog_id, "Programmator state missing for PROG");
        send_u_packet(tx, "@P", &programmator_status(false).1);
        send_programmator_state_error(tx);
        return;
    };

    if running
        && let Some((entity, due_at)) = state.query_player_opt(pid, |ecs, entity| {
            ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                .filter(|program| program.running)
                .map(|program| (entity, program.delay))
        })
    {
        state.schedule_programmator(entity, due_at);
    }

    tracing::info!(
        player_id = %pid,
        program_id = prog_id,
        len = source.len(),
        running,
        "PROGDIAG PROG program run status"
    );

    send_u_packet(tx, "Gu", &gu_close().1);
    send_programmator_start_position(tx, server_pos, running);
    if running {
        // Unity @P=1 activates the whole ProgrammatorWindow. #p must
        // be last: UpdateProgramm hydrates selected state and ends by
        // hiding that window again.
        send_u_packet(tx, "@P", &programmator_status(true).1);
        send_u_packet(tx, "BH", &hand_mode(false).1);
        send_u_packet(tx, "#p", &open_programmator(prog_id, prog_name, source).1);
    } else {
        send_u_packet(tx, "@P", &programmator_status(false).1);
        send_u_packet(tx, "BH", &hand_mode(false).1);
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
}

pub fn handle_prog_reset_ty(state: &Arc<GameState>, tx: &Outbox, pid: PlayerId) {
    // Unity uses stopped `pRST` as a pre-open/reset signal from
    // `OnProgButton()`. It must not open `#P`: doing so reopens the
    // editor over gameplay when the user is only toggling program mode.
    // Only a real running->stopped transition sends visible wire.
    let result = state
        .modify_player(pid, |ecs, entity| {
            let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
            let was_running = ps.running;
            if was_running {
                ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?
                    .stop_program();
            }
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_window = None;
            }
            Some(was_running)
        })
        .flatten();
    let Some(was_running) = result else {
        tracing::error!(player_id = %pid, "Programmator state missing for pRST");
        send_u_packet(tx, "@P", &programmator_status(false).1);
        send_programmator_state_error(tx);
        return;
    };
    if was_running {
        // RunProgramm() закрывает окно перед остановкой.
        clear_programmator_window(state, pid);
        send_u_packet(tx, "Gu", &gu_close().1);
        send_u_packet(tx, "@P", &programmator_status(false).1);
        send_u_packet(tx, "BH", &hand_mode(false).1);
    }
}

pub fn handle_prog_rename_prompt_ty(
    state: &Arc<GameState>,
    tx: &Outbox,
    pid: PlayerId,
    payload: &[u8],
) {
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

pub async fn handle_whoi(state: &Arc<GameState>, tx: &Outbox, ids: &[i32]) {
    let mut parts = Vec::new();
    for &id in ids {
        let mut name_opt =
            state.query_player_opt(id.into(), |ecs: &bevy_ecs::prelude::World, entity| {
                ecs.get::<crate::game::player::PlayerMetadata>(entity)
                    .map(|m| m.name.clone())
            });
        if name_opt.is_none() {
            match state.db.get_player_by_id(id).await {
                Ok(Some(p)) => name_opt = Some(p.name),
                Ok(None) => {}
                Err(e) => {
                    tracing::error!(player_id = id, error = ?e, "DB get failed for Whoi");
                    send_u_packet(
                        tx,
                        "OK",
                        &ok_message("НИКИ", "Не удалось прочитать имя игрока.").1,
                    );
                    return;
                }
            }
        }
        // Wire-контракт NL допускает пустое имя для реально отсутствующего id.
        let name = name_opt.unwrap_or_default();
        parts.push(format!("{id}:{name}"));
    }
    send_u_packet(tx, "NL", parts.join(",").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ServerTestHarness, drain_events};

    async fn make_test_state(label: &str) -> ServerTestHarness {
        ServerTestHarness::new(label, "prog-user").await
    }

    fn prog_payload(compiled_len: i32, prog_id: i32, compiled: &[u8], source: &str) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&compiled_len.to_le_bytes());
        payload.extend_from_slice(&prog_id.to_le_bytes());
        payload.extend_from_slice(compiled);
        payload.extend_from_slice(source.as_bytes());
        payload
    }

    #[tokio::test]
    async fn prog_valid_payload_saves_updates_and_reports_failed_run() {
        let test = make_test_state("prog_state_machine").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "old")
            .await
            .unwrap();
        let payload = prog_payload(3, prog_id, &[1, 2, 3], "");

        // Эмулируем открытое окно (Pope-меню ставит current_window). Запуск проги
        // ОБЯЗАН его сбросить — иначе после стопа гард window_open в handle_move
        // кидает игрока назад («сервер кидает назад»).
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)?
                .current_window = Some("pope".to_string());
            Some(())
        });

        handle_prog_ty(&test.state, &tx, pid, "PROG", &payload).await;

        let window_after = test.state.query_player_opt(pid, |ecs, entity| {
            Some(
                ecs.get::<crate::game::player::PlayerUI>(entity)?
                    .current_window
                    .is_some(),
            )
        });
        assert_eq!(
            window_after,
            Some(false),
            "current_window должен сброситься на запуске проги (иначе tp-back после стопа)"
        );

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "@P", "BH", "OK"]);
        assert_eq!(events[0].1, b"_");
        assert_eq!(events[1].1, b"0");
        assert_eq!(events[2].1, b"0");

        let saved = test.state.db.get_program(prog_id).await.unwrap().unwrap();
        assert_eq!(saved.code, "");
    }

    #[tokio::test]
    async fn prog_running_start_syncs_authoritative_position_before_status() {
        let test = make_test_state("prog_start_position_sync").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "old")
            .await
            .unwrap();
        let payload = prog_payload(0, prog_id, &[], "$z");

        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut pos = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
            pos.x = 17;
            pos.y = 23;
            Some(())
        });

        handle_prog_ty(&test.state, &tx, pid, "PROG", &payload).await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "@T", "@P", "BH", "#p"]);
        assert!(!names.contains(&"#P"));
        assert_eq!(events[0].1, b"_");
        assert_eq!(events[1].1, b"17:23");
        assert_eq!(events[2].1, b"1");
        assert_eq!(events[3].1, b"0");
        let update_json: serde_json::Value = serde_json::from_slice(&events[4].1).unwrap();
        assert_eq!(update_json["id"], prog_id);
        assert_eq!(update_json["title"], "main");
        assert_eq!(update_json["source"], "$z");
    }

    #[tokio::test]
    async fn prog_rejects_missing_positive_program_id_without_creating_default_program() {
        let test = make_test_state("prog_missing_positive_id").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let missing_prog_id = 99_999;
        let payload = prog_payload(0, missing_prog_id, &[], "$z");
        let pid = PlayerId(test.player.id);

        handle_prog_ty(&test.state, &tx, pid, "PROG", &payload).await;

        assert!(
            test.state
                .db
                .get_program(missing_prog_id)
                .await
                .unwrap()
                .is_none()
        );
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Не удалось сохранить программу."));
    }

    #[tokio::test]
    async fn prog_zero_id_opens_program_list_even_when_server_has_stale_selected_program() {
        let test = make_test_state("prog_zero_id_opens_list").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "$z")
            .await
            .unwrap();
        test.state
            .db
            .set_selected_program(test.player.id, Some(prog_id))
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some("$z".to_string());
            Some(())
        });

        let payload = prog_payload(0, 0, &[], "");
        handle_prog_ty(&test.state, &tx, pid, "PROG", &payload).await;

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "GU");
        let payload = std::str::from_utf8(&events[0].1).unwrap();
        assert!(payload.starts_with("horb:"));
        assert!(payload.contains("ПРОГРАММАТОР"));
        assert!(payload.contains("main"));
        assert!(
            !events
                .iter()
                .any(|(event, _)| event == "OK" || event == "@P"),
            "fallback must open the picker, not report an invalid program or toggle run status"
        );

        let saved = test.state.db.get_program(prog_id).await.unwrap().unwrap();
        assert_eq!(saved.code, "$z");
    }

    #[tokio::test]
    async fn auto_dig_toggle_missing_flags_is_explicit_error_without_settings_mutation() {
        let test = make_test_state("auto_dig_toggle_missing_flags").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerSettings>(entity)
                .unwrap()
                .auto_dig = false;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }

        crate::game::logic::commands::apply_player_command(
            &test.state,
            crate::game::PlayerCommand::ToggleAutoDig { player_id: pid },
        );

        let auto_dig = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerSettings>(entity)?
                        .auto_dig,
                )
            })
            .unwrap();
        assert!(!auto_dig);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(!events.iter().any(|(event, _)| event == "BD"));
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние настроек недоступно."));
    }

    #[tokio::test]
    async fn auto_dig_set_missing_flags_is_explicit_error_without_settings_mutation() {
        let test = make_test_state("auto_dig_set_missing_flags").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.get_mut::<crate::game::player::PlayerSettings>(entity)
                .unwrap()
                .auto_dig = false;
            ecs.entity_mut(entity)
                .remove::<crate::game::player::PlayerFlags>();
        }

        crate::game::logic::commands::apply_programmator_auto_dig_set(&test.state, &tx, pid, true);

        let auto_dig = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                Some(
                    ecs.get::<crate::game::player::PlayerSettings>(entity)?
                        .auto_dig,
                )
            })
            .unwrap();
        assert!(!auto_dig);
        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
        assert!(!events.iter().any(|(event, _)| event == "BD"));
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Состояние настроек недоступно."));
    }

    #[tokio::test]
    async fn aggression_toggle_updates_state_and_sends_ba() {
        let test = make_test_state("aggression_toggle").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        crate::game::logic::commands::apply_player_command(
            &test.state,
            crate::game::PlayerCommand::ToggleAggression { player_id: pid },
        );

        let (aggression, dirty) = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let settings = ecs.get::<crate::game::player::PlayerSettings>(entity)?;
                let flags = ecs.get::<crate::game::player::PlayerFlags>(entity)?;
                Some((settings.aggression, flags.dirty))
            })
            .unwrap();
        assert!(aggression);
        assert!(dirty);

        let events = drain_events(&mut rx);
        assert_eq!(events, vec![("BA".to_string(), b"1".to_vec())]);
    }

    #[tokio::test]
    async fn aggression_set_unchanged_is_silent() {
        let test = make_test_state("aggression_set_unchanged").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        crate::game::logic::commands::apply_programmator_aggression_set(
            &test.state,
            &tx,
            pid,
            false,
        );

        assert!(drain_events(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn prst_from_open_program_list_does_not_reopen_selected_stopped_program() {
        let test = make_test_state("prst_state_machine").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(String::new());
            ps.running = false;
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)?
                .current_window = Some("prog".to_string());
            Some(())
        });

        handle_prog_ty(&test.state, &tx, pid, "pRST", b"").await;

        let window_after = test.state.query_player_opt(pid, |ecs, entity| {
            Some(
                ecs.get::<crate::game::player::PlayerUI>(entity)?
                    .current_window
                    .clone(),
            )
        });
        assert_eq!(window_after, Some(None));

        let events = drain_events(&mut rx);
        assert!(
            events.is_empty(),
            "stopped pRST must not emit #P or @P; client may send it as pre-open reset"
        );
    }

    #[tokio::test]
    async fn prst_preopen_signal_for_hydrated_editor_sends_no_packets() {
        let test = make_test_state("prst_preopen_no_packets").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(String::new());
            ps.running = false;
            ecs.get_mut::<crate::game::player::PlayerUI>(entity)?
                .current_window = None;
            Some(())
        });

        handle_prog_ty(&test.state, &tx, pid, "pRST", b"").await;

        let events = drain_events(&mut rx);
        assert!(
            events.is_empty(),
            "stopped pre-open pRST must not emit @P 0"
        );
    }

    #[tokio::test]
    async fn prst_stops_running_program_and_clears_hand_mode_wire_state() {
        let test = make_test_state("prst_stop_clears_hand_mode").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.running = true;
            ps.hand_mode_active = true;
            Some(())
        });

        handle_prog_ty(&test.state, &tx, pid, "pRST", b"").await;

        let state_after = test.state.query_player_opt(pid, |ecs, entity| {
            let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
            Some((ps.running, ps.hand_mode_active))
        });
        assert_eq!(state_after, Some((false, false)));

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["Gu", "@P", "BH"]);
        assert_eq!(events[0].1, b"_");
        assert_eq!(events[1].1, b"0");
        assert_eq!(events[2].1, b"0");
    }

    #[tokio::test]
    async fn prog_missing_programmator_state_is_explicit_error_not_stopped_fallback() {
        let test = make_test_state("prog_missing_component").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "old")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::programmator::ProgrammatorState>();
        }

        let payload = prog_payload(3, prog_id, &[1, 2, 3], "");
        handle_prog_ty(&test.state, &tx, pid, "PROG", &payload).await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["@P", "OK"]);
        assert_eq!(events[0].1, b"0");
        let message = std::str::from_utf8(&events[1].1).unwrap();
        assert!(message.contains("Состояние программатора недоступно."));
    }

    #[tokio::test]
    async fn prst_missing_programmator_state_is_explicit_error_not_stopped_fallback() {
        let test = make_test_state("prst_missing_component").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let pid = PlayerId(test.player.id);
        let entity = test.state.get_player_entity(pid).unwrap();
        {
            let mut ecs = test.state.ecs.write();
            ecs.entity_mut(entity)
                .remove::<crate::game::programmator::ProgrammatorState>();
        }

        handle_prog_ty(&test.state, &tx, pid, "pRST", b"").await;

        let events = drain_events(&mut rx);
        let names: Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, vec!["@P", "OK"]);
        assert_eq!(events[0].1, b"0");
        let message = std::str::from_utf8(&events[1].1).unwrap();
        assert!(message.contains("Состояние программатора недоступно."));
    }

    #[tokio::test]
    async fn pcop_copies_owned_program_and_refreshes_list() {
        let test = make_test_state("pcop_state_machine").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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
            PlayerId(test.player.id),
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
    }

    #[tokio::test]
    async fn pcop_rejects_foreign_program_without_copying() {
        let test = make_test_state("pcop_foreign_state_machine").await;
        let foreign = test.create_player("foreign").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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
            PlayerId(test.player.id),
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
        assert_eq!(events[0].0, "OK");
        let message = std::str::from_utf8(&events[0].1).unwrap();
        assert!(message.contains("Программа недоступна."));
    }

    #[tokio::test]
    async fn pdel_deletes_owned_selected_program_without_wire_response() {
        let test = make_test_state("pdel_state_machine").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let prog_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(prog_id);
            ps.selected_data = Some(String::new());
            ps.running = true;
            Some(())
        });

        handle_prog_ty(
            &test.state,
            &tx,
            pid,
            "PDEL",
            prog_id.to_string().as_bytes(),
        )
        .await;

        assert!(test.state.db.get_program(prog_id).await.unwrap().is_none());
        let state_after = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                Some((ps.selected_id, ps.selected_data.clone(), ps.running))
            })
            .unwrap();
        assert_eq!(state_after, (None, None, false));
        assert!(drain_events(&mut rx).is_empty());
    }

    #[tokio::test]
    async fn pdel_rejects_foreign_program_without_clearing_selected_state() {
        let test = make_test_state("pdel_foreign_state_machine").await;
        let foreign = test.create_player("foreign-pdel").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);

        let own_prog_id = test
            .state
            .db
            .insert_program(test.player.id, "own", "")
            .await
            .unwrap();
        let foreign_prog_id = test
            .state
            .db
            .insert_program(foreign.id, "foreign", "")
            .await
            .unwrap();
        let pid = PlayerId(test.player.id);
        test.state.modify_player(pid, |ecs, entity| {
            let mut ps = ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?;
            ps.selected_id = Some(own_prog_id);
            ps.selected_data = Some(String::new());
            ps.running = true;
            Some(())
        });

        handle_prog_ty(
            &test.state,
            &tx,
            pid,
            "PDEL",
            foreign_prog_id.to_string().as_bytes(),
        )
        .await;

        assert!(
            test.state
                .db
                .get_program(foreign_prog_id)
                .await
                .unwrap()
                .is_some()
        );
        let state_after = test
            .state
            .query_player_opt(pid, |ecs, entity| {
                let ps = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                Some((ps.selected_id, ps.selected_data.clone(), ps.running))
            })
            .unwrap();
        assert_eq!(state_after, (Some(own_prog_id), Some(String::new()), true));

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "OK");
    }

    #[tokio::test]
    async fn pren_opens_typed_rename_dialog() {
        let test = make_test_state("pren_state_machine").await;
        let (tx, mut rx) = test.connect_with_outbox(1);
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
            PlayerId(test.player.id),
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
    }

    #[tokio::test]
    async fn stale_program_persistence_completion_does_not_mutate_reconnected_player() {
        let test = make_test_state("program_completion_stale_session").await;
        let (_old_tx, mut old_rx) = test.connect_with_outbox(1);
        drain_events(&mut old_rx);

        let (new_tx, mut new_rx) = crate::net::session::outbox::channel();
        let player_id = PlayerId(test.player.id);
        let new_session = crate::game::SessionId::new(2);
        test.state
            .sessions
            .register_test_outbox(new_session, new_tx);
        assert!(test.state.sessions.bind_player(new_session, player_id));

        crate::game::logic::commands::apply_persistence_completion(
            &test.state,
            crate::game::PersistenceCompletion::ProgramSaved {
                request: crate::game::ProgramSaveRequest {
                    player_id,
                    session_id: crate::game::SessionId::new(1),
                    program_id: 77,
                    source: "$z".to_owned(),
                },
                result: crate::game::ProgramSaveResult::Saved {
                    program_name: "main".to_owned(),
                },
            },
        );

        let selected = test.state.query_player_opt(player_id, |ecs, entity| {
            ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                .map(|state| state.selected_id)
        });
        assert_eq!(selected, Some(None));
        assert!(drain_events(&mut old_rx).is_empty());
        assert!(drain_events(&mut new_rx).is_empty());
    }

    #[tokio::test]
    async fn prog_command_produces_typed_save_without_direct_database_mutation() {
        let test = make_test_state("program_command_persistence_request").await;
        let (_tx, mut rx) = test.connect_with_outbox(1);
        drain_events(&mut rx);
        let program_id = test
            .state
            .db
            .insert_program(test.player.id, "main", "old")
            .await
            .unwrap();

        let effects = crate::game::logic::commands::apply_player_command(
            &test.state,
            crate::game::PlayerCommand::ProgramAction {
                player_id: PlayerId(test.player.id),
                session_id: crate::game::SessionId::new(1),
                event: "PROG".to_owned(),
                payload: bytes::Bytes::from(prog_payload(0, program_id, &[], "new")),
            },
        );

        assert!(effects.events.is_empty());
        assert!(matches!(
            effects.saves.as_slice(),
            [crate::game::SaveCommand::Program {
                request: crate::game::ProgramSaveRequest {
                    player_id,
                    session_id,
                    program_id: saved_program_id,
                    source,
                }
            }] if *player_id == PlayerId(test.player.id)
                && *session_id == crate::game::SessionId::new(1)
                && *saved_program_id == program_id
                && source == "new"
        ));
        assert_eq!(
            test.state
                .db
                .get_program(program_id)
                .await
                .unwrap()
                .unwrap()
                .code,
            "old"
        );
    }
}
