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

/// TY программатор — как `Session.PROG/PDEL/pRST/PREN` + `StaticGUI` в server_reference.
pub fn handle_prog_ty(
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
                if let Err(e) = state.db.save_program(pid, prog_id, &source) {
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

                send_u_packet(tx, "Gu", &gu_close().1);
                send_u_packet(tx, "@P", &programmator_status(running).1);
            } else {
                tracing::warn!(
                    "[PROG] Failed to decode payload pid={pid} len={}",
                    payload.len()
                );
                send_u_packet(tx, "@P", &programmator_status(false).1);
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
                        ecs.get_mut::<crate::game::programmator::ProgrammatorState>(entity)?.running = false;
                    }
                    Some((false, editor_data))
                })
                .flatten();
            let (running, editor_data) = result.unwrap_or((false, None));
            // C# ref: OpenProg sends #P packet with program data
            if let Some((prog_id, source)) = editor_data {
                let name = state.db.get_program(prog_id)
                    .ok().flatten()
                    .map(|p| p.name)
                    .unwrap_or_default();
                send_u_packet(tx, "#P", &open_programmator(prog_id, &name, &source).1);
            }
            send_u_packet(tx, "@P", &programmator_status(running).1);
        }
        "PDEL" => {
            if let Ok(id_str) = std::str::from_utf8(payload) {
                if let Ok(prog_id) = id_str.trim().parse::<i32>() {
                    if let Err(e) = state.db.delete_program_owned(pid, prog_id) {
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
            send_u_packet(tx, "@P", &programmator_status(false).1);
        }
        "PREN" => {
            let running = state
                .query_player(pid, |ecs, entity| {
                    ecs.get::<crate::game::programmator::ProgrammatorState>(entity)
                        .is_some_and(|ps| ps.running)
                })
                .unwrap_or(false);
            send_u_packet(tx, "@P", &programmator_status(running).1);
        }
        _ => {}
    }
}

/// TY `Sett` → `Settings.SendSettingsGUI` в server_reference — 1:1 с C# RichList.
pub fn handle_sett_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    _payload: &[u8],
) {
    let settings = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerSettings>(entity)
                .copied()
        })
        .flatten();
    let Some(s) = settings else { return };

    let has_clan = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerStats>(entity)
                .map(|st| st.clan_id.unwrap_or(0) != 0)
        })
        .flatten()
        .unwrap_or(false);

    // C# ref: RichList entries — each entry = [label, type, values, action, value]
    // DropDown: values = "0:label0#1:label1#", action = key, value = index
    // Bool: values = "0", action = key, value = "0"/"1"
    let rich: Vec<serde_json::Value> = vec![
        // DropDown: "Масштаб интерфейса"
        serde_json::json!("Масштаб интерфейса"), serde_json::json!("drop"),
        serde_json::json!("0:мелко#1:КРУПНО#"), serde_json::json!("isca"),
        serde_json::json!(s.isca.to_string()),
        // DropDown: "Масштаб территории"
        serde_json::json!("Масштаб территории"), serde_json::json!("drop"),
        serde_json::json!("0:мелко#1:КРУПНО#"), serde_json::json!("tsca"),
        serde_json::json!(s.tsca.to_string()),
        // Bool: "Включить управление мышкой"
        serde_json::json!("Включить управление мышкой"), serde_json::json!("bool"),
        serde_json::json!("0"), serde_json::json!("mous"),
        serde_json::json!(if s.mous { "1" } else { "0" }),
        // Bool: "Упрощенный режим графики"
        serde_json::json!("Упрощенный режим графики"), serde_json::json!("bool"),
        serde_json::json!("0"), serde_json::json!("pot"),
        serde_json::json!(if s.pot { "1" } else { "0" }),
        // Bool: "ринудительно обновлять породы"
        serde_json::json!("ринудительно обновлять породы (увеличит потр. CPU)"), serde_json::json!("bool"),
        serde_json::json!("0"), serde_json::json!("frc"),
        serde_json::json!(if s.frc { "1" } else { "0" }),
        // Bool: "CTRL переключает скорость"
        serde_json::json!("CTRL переключает скорость робота (вместо удерживания)"), serde_json::json!("bool"),
        serde_json::json!("0"), serde_json::json!("ctrl"),
        serde_json::json!(if s.ctrl { "1" } else { "0" }),
        // Bool: "Отключить ближайшие звуки"
        serde_json::json!("Отключить ближайшие звуки"), serde_json::json!("bool"),
        serde_json::json!("0"), serde_json::json!("mof"),
        serde_json::json!(if s.mof { "1" } else { "0" }),
    ];

    let mut buttons = vec![
        serde_json::json!("Сохранить"),
        serde_json::json!("save:%R%"),
    ];
    if !has_clan {
        buttons.push(serde_json::json!("Создать клан"));
        buttons.push(serde_json::json!("clancreate"));
    }
    buttons.push(serde_json::json!("ВЫЙТИ"));
    buttons.push(serde_json::json!("exit"));

    let gui = serde_json::json!({
        "title": "НА СТРОЙКЕ",
        "tabs": ["Настройки", ""],
        "richList": rich,
        "buttons": buttons,
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());

    state.modify_player(pid, |ecs, entity| {
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
            ui.current_window = Some("settings".to_string());
        }
        Some(())
    });
}

pub fn handle_whoi(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, ids: &[i32]) {
    let parts: Vec<String> = ids
        .iter()
        .map(|&id| {
            let name = state
                .query_player(id, |ecs: &bevy_ecs::prelude::World, entity| {
                    ecs.get::<crate::game::player::PlayerMetadata>(entity)
                        .map(|m| m.name.clone())
                })
                .flatten()
                .or_else(|| state.db.get_player_by_id(id).ok().flatten().map(|p| p.name))
                .unwrap_or_default();
            format!("{id}:{name}")
        })
        .collect();
    send_u_packet(tx, "NL", parts.join(",").as_bytes());
}
