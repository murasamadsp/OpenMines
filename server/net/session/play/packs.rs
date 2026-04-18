//! Паки на клетке: обнаружение, UI, взаимодействие.
use crate::net::session::prelude::*;

const CRYSTAL_NAMES: [&str; 6] = [
    "Зелёный",
    "Синий",
    "Красный",
    "Фиолетовый",
    "Белый",
    "Голубой",
];
pub const CRYSTAL_PRICES: [i64; 6] = [1, 3, 2, 6, 4, 5];

pub fn check_pack_at_position(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    x: i32,
    y: i32,
) {
    let current_window = state
        .active_players
        .get(&pid)
        .and_then(|p| p.current_window.clone());

    if let Some(pack_pos) = state.find_pack_covering(x, y) {
        if let Some(pack) = state.get_pack_at(pack_pos.0, pack_pos.1) {
            let pack = pack.clone();
            // Gate — это клан-проход без GUI: ничего не показываем при входе на клетку.
            if pack.pack_type == PackType::Gate {
                return;
            }
            let window = format!("pack:{}:{}", pack.x, pack.y);
            if current_window.as_deref() == Some(window.as_str()) {
                return;
            }
            // Check clan access: public or player's clan
            let player_clan = state
                .active_players
                .get(&pid)
                .map_or(0, |p| p.data.clan_id.unwrap_or(0));
            if pack.clan_id != 0 && pack.clan_id != player_clan {
                tracing::warn!(
                    "check_pack_at_position: pid={pid} denied access to clan pack at ({},{}): pack_clan={} player_clan={}",
                    pack.x,
                    pack.y,
                    pack.clan_id,
                    player_clan
                );
                return;
            }
            let gui_json = build_pack_gui(state, pid, &pack);
            if let Some(mut p) = state.active_players.get_mut(&pid) {
                p.current_window = Some(format!("pack:{}:{}", pack.x, pack.y));
            }
            send_u_packet(tx, "GU", gui_json.as_bytes());
            return;
        }
    }
    if current_window.is_some_and(|w| w.starts_with("pack:")) {
        if let Some(mut p) = state.active_players.get_mut(&pid) {
            p.current_window = None;
        }
        send_u_packet(tx, "Gu", &[]);
    }
}

pub fn build_pack_gui(state: &Arc<GameState>, pid: PlayerId, pack: &PackData) -> String {
    match pack.pack_type {
        PackType::Resp => {
            let is_bound = state
                .active_players
                .get(&pid)
                .is_some_and(|p| p.data.resp_x == Some(pack.x) && p.data.resp_y == Some(pack.y));
            let is_owner = pack.owner_id == pid;
            let can_bind = is_owner || pack.clan_id == 0;
            let text = if is_bound {
                format!(
                    "@@Респ - это место, где будет появляться ваш робот\nпосле уничтожения (HP = 0)\n\nЦена восстановления: <color=green>${}</color>\n\n<color=#8f8>Вы привязаны к этому респу.</color>",
                    pack.cost
                )
            } else {
                format!(
                    "@@Респ - это место, где будет появляться ваш робот\nпосле уничтожения (HP = 0)\n\nЦена восстановления: <color=green>${}</color>\n\n<color=#f88>Привязать робота к респу?</color>",
                    pack.cost
                )
            };
            let mut buttons = pack_close_buttons();
            // Insert bind button before exit
            if can_bind && !is_bound {
                buttons.insert(0, serde_json::json!("ПРИВЯЗАТЬ"));
                buttons.insert(
                    1,
                    serde_json::json!(format!("resp_bind:{}:{}", pack.x, pack.y)),
                );
            }
            add_pack_remove_button(state, pid, pack, &mut buttons);
            let gui = serde_json::json!({
                "title": "РЕСП",
                "text": text,
                "buttons": buttons,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Teleport => {
            let charge_txt = format!(
                "Заряд телепорта: {:.0} / {:.0}\n\n",
                pack.charge, pack.max_charge
            );
            // Build list of nearby teleports
            let mut tp_buttons = Vec::new();
            for entry in &state.packs {
                let tp = entry.value();
                if tp.pack_type != PackType::Teleport {
                    continue;
                }
                if tp.x == pack.x && tp.y == pack.y {
                    continue;
                }
                // Within 1000 cells
                if (tp.x - pack.x).abs() < 1000 && (tp.y - pack.y).abs() < 1000 {
                    tp_buttons.push(serde_json::json!(format!("tp_go:{}:{}", tp.x, tp.y)));
                }
            }
            add_pack_remove_button(state, pid, pack, &mut tp_buttons);
            tp_buttons.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "Тп",
                "text": format!("{charge_txt}Выберите точку телепортации"),
                "buttons": tp_buttons,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Market => {
            // Crystal prices: green=1, red=2, blue=3, white=4, cyan=5, violet=6
            let names = CRYSTAL_NAMES;
            let prices = CRYSTAL_PRICES;
            let mut btns: Vec<serde_json::Value> = Vec::new();
            let player_crystals = state
                .active_players
                .get(&pid)
                .map(|p| p.data.crystals)
                .unwrap_or([0; 6]);
            let mut text = format!(
                "@@Маркет — продажа кристаллов\nВладелец #{}\n\n",
                pack.owner_id
            );
            for i in 0..6usize {
                text.push_str(&format!(
                    "{}: {}$ за шт. (у вас: {})\n",
                    names[i], prices[i], player_crystals[i]
                ));
            }
            for i in 0..6usize {
                if player_crystals[i] > 0 {
                    btns.push(serde_json::json!(format!("Продать {} x1", names[i])));
                    btns.push(serde_json::json!(format!(
                        "market_sell:{}:1:{}:{}",
                        i, pack.x, pack.y
                    )));
                    btns.push(serde_json::json!(format!("Продать {} x10", names[i])));
                    btns.push(serde_json::json!(format!(
                        "market_sell:{}:10:{}:{}",
                        i, pack.x, pack.y
                    )));
                    btns.push(serde_json::json!(format!("Продать {} (всё)", names[i])));
                    btns.push(serde_json::json!(format!(
                        "market_sell:{}:max:{}:{}",
                        i, pack.x, pack.y
                    )));
                }
            }
            add_pack_remove_button(state, pid, pack, &mut btns);
            btns.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "Маркет",
                "text": text,
                "buttons": btns,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Up => {
            let player_skills = state
                .active_players
                .get(&pid)
                .map(|p| p.data.skills.clone())
                .unwrap_or_default();
            let mut text = String::from("@@Прокачка навыков\n\n");
            let mut btns: Vec<serde_json::Value> = Vec::new();
            for (code, ss) in &player_skills {
                let max_exp = 100.0 * ss.level as f32;
                text.push_str(&format!(
                    "{}: ур.{} ({:.0}/{:.0} exp)\n",
                    code, ss.level, ss.exp, max_exp
                ));
                if ss.exp >= max_exp {
                    btns.push(serde_json::json!(format!("Улучшить {}", code)));
                    btns.push(serde_json::json!(format!("skill_up:{}", code)));
                }
            }
            add_pack_remove_button(state, pid, pack, &mut btns);
            btns.push(serde_json::json!("Добавить навык"));
            btns.push(serde_json::json!("skill_install"));
            btns.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "UP",
                "text": text,
                "buttons": btns,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Gun => {
            let player_cyan = state
                .active_players
                .get(&pid)
                .map(|p| p.data.crystals[5])
                .unwrap_or(0);
            let text = format!(
                "@@Пушка\nЗаряд: {:.0} / {:.0}\nЦиановых кристаллов: {}\n\n+100 заряда = 10 кристаллов\n+1000 заряда = 100 кристаллов",
                pack.charge, pack.max_charge, player_cyan
            );
            let mut btns = Vec::new();
            btns.push(serde_json::json!("+100 заряда"));
            btns.push(serde_json::json!(format!(
                "gun_fill:100:{}:{}",
                pack.x, pack.y
            )));
            btns.push(serde_json::json!("+1000 заряда"));
            btns.push(serde_json::json!(format!(
                "gun_fill:1000:{}:{}",
                pack.x, pack.y
            )));
            btns.push(serde_json::json!("Макс. заряд"));
            btns.push(serde_json::json!(format!(
                "gun_fill:max:{}:{}",
                pack.x, pack.y
            )));
            add_pack_remove_button(state, pid, pack, &mut btns);
            btns.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "Пушка",
                "text": text,
                "buttons": btns,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Storage => {
            let player_crystals = state
                .active_players
                .get(&pid)
                .map(|p| p.data.crystals)
                .unwrap_or([0; 6]);
            let names = CRYSTAL_NAMES;
            let mut text = String::from("@@Хранилище\n\nВ хранилище:\n");
            for i in 0..6usize {
                text.push_str(&format!("  {}: {}\n", names[i], pack.crystals_inside[i]));
            }
            text.push_str("\nУ вас:\n");
            for i in 0..6usize {
                text.push_str(&format!("  {}: {}\n", names[i], player_crystals[i]));
            }
            let mut btns: Vec<serde_json::Value> = Vec::new();
            for i in 0..6usize {
                if player_crystals[i] > 0 {
                    btns.push(serde_json::json!(format!("Положить {} x1", names[i])));
                    btns.push(serde_json::json!(format!(
                        "storage_deposit:{}:1:{}:{}",
                        i, pack.x, pack.y
                    )));
                    btns.push(serde_json::json!(format!("Положить {} (всё)", names[i])));
                    btns.push(serde_json::json!(format!(
                        "storage_deposit:{}:max:{}:{}",
                        i, pack.x, pack.y
                    )));
                }
                if pack.crystals_inside[i] > 0 {
                    btns.push(serde_json::json!(format!("Взять {} x1", names[i])));
                    btns.push(serde_json::json!(format!(
                        "storage_withdraw:{}:1:{}:{}",
                        i, pack.x, pack.y
                    )));
                    btns.push(serde_json::json!(format!("Взять {} (всё)", names[i])));
                    btns.push(serde_json::json!(format!(
                        "storage_withdraw:{}:max:{}:{}",
                        i, pack.x, pack.y
                    )));
                }
            }
            add_pack_remove_button(state, pid, pack, &mut btns);
            btns.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "Хранилище",
                "text": text,
                "buttons": btns,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Craft => build_crafter_gui(state, pid, pack),
        PackType::Spot => {
            let mut buttons = pack_close_buttons();
            add_pack_remove_button(state, pid, pack, &mut buttons);
            let gui = serde_json::json!({
                "title": "Спот",
                "text": format!("Спот #{}\nВладелец #{}", pack.id, pack.owner_id),
                "buttons": buttons,
                "back": false
            });
            format!("horb:{gui}")
        }
        PackType::Gate => {
            // У ворот нет GUI — окно не должно открываться. Возвращаем пустую строку
            // на случай если build_pack_gui вызовется извне (не должно случиться).
            let mut buttons = pack_close_buttons();
            add_pack_remove_button(state, pid, pack, &mut buttons);
            let gui = serde_json::json!({
                "title": "Ворота",
                "text": format!("Клан #{}", pack.clan_id),
                "buttons": buttons,
                "back": false
            });
            format!("horb:{gui}")
        }
    }
}

fn can_manage_pack(state: &Arc<GameState>, pid: PlayerId, pack: &PackData) -> bool {
    is_pack_owner_or_clan_member(state, pid, pack)
}

fn add_pack_remove_button(
    state: &Arc<GameState>,
    pid: PlayerId,
    pack: &PackData,
    buttons: &mut Vec<serde_json::Value>,
) {
    if can_manage_pack(state, pid, pack) {
        buttons.push(serde_json::json!("УДАЛИТЬ"));
        buttons.push(serde_json::json!(format!(
            "pack_remove:{}:{}",
            pack.x, pack.y
        )));
    }
}

fn pack_close_buttons() -> Vec<serde_json::Value> {
    CLOSE_WINDOW_BUTTON_LABELS
        .iter()
        .map(|label| serde_json::Value::String(label.to_string()))
        .collect()
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(0))
        .unwrap_or(0)
}

const fn cost_crystal_names() -> [&'static str; 6] {
    CRYSTAL_NAMES
}

fn format_costs(recipe: &Recipe) -> String {
    let names = cost_crystal_names();
    let mut parts: Vec<String> = Vec::new();
    for c in recipe.cost_crys {
        let idx = usize::try_from(c.id).unwrap_or(0).min(5);
        parts.push(format!("{} x{}", names[idx], c.num));
    }
    for c in recipe.cost_res {
        parts.push(format!("предмет#{} x{}", c.id, c.num));
    }
    if parts.is_empty() {
        "бесплатно".to_string()
    } else {
        parts.join(", ")
    }
}

fn build_crafter_gui(state: &Arc<GameState>, pid: PlayerId, pack: &PackData) -> String {
    // Активный крафт: прогресс и Claim при готовности.
    if let Some(recipe_id) = pack.craft_recipe_id {
        if let Some(recipe) = recipe_by_id(recipe_id) {
            let now = now_unix();
            let remaining = (pack.craft_end_ts - now).max(0);
            let total = i64::from(recipe.time_sec) * i64::from(pack.craft_num.max(1));
            let done = (total - remaining).max(0);
            let percent = if total > 0 {
                ((done * 100) / total).clamp(0, 100)
            } else {
                100
            };
            let bar_len = 30i64;
            let filled = (percent * bar_len / 100).clamp(0, bar_len) as usize;
            let bar = format!(
                "<color=#aaeeaa>{}</color>{}",
                "|".repeat(filled),
                "-".repeat(bar_len as usize - filled)
            );
            let ready = pack.craft_end_ts > 0 && now >= pack.craft_end_ts;
            let text = format!(
                "@@Крафтер\n{} x{}\n\n{}% {}\n\n{}",
                recipe.title,
                pack.craft_num,
                percent,
                bar,
                if ready {
                    "<color=#8f8>ГОТОВО — забирай!</color>".to_string()
                } else {
                    format!("Осталось {remaining} сек.")
                }
            );
            let mut btns: Vec<serde_json::Value> = Vec::new();
            if ready {
                btns.push(serde_json::json!("Забрать"));
                btns.push(serde_json::json!(format!(
                    "craft_claim:{}:{}",
                    pack.x, pack.y
                )));
            }
            add_pack_remove_button(state, pid, pack, &mut btns);
            btns.extend(pack_close_buttons());
            let gui = serde_json::json!({
                "title": "Крафтер",
                "text": text,
                "buttons": btns,
                "back": false
            });
            return format!("horb:{gui}");
        }
    }

    // Idle — показываем список рецептов.
    let mut text = String::from("@@Крафтер\nВыберите рецепт:\n\n");
    let mut btns: Vec<serde_json::Value> = Vec::new();
    for recipe in recipes() {
        text.push_str(&format!(
            "• {} → предмет#{} x{} ({} сек.)\n   Стоимость: {}\n",
            recipe.title,
            recipe.result.id,
            recipe.result.num,
            recipe.time_sec,
            format_costs(recipe)
        ));
        btns.push(serde_json::json!(format!("Крафт: {} x1", recipe.title)));
        btns.push(serde_json::json!(format!(
            "craft_start:{}:1:{}:{}",
            recipe.id, pack.x, pack.y
        )));
    }
    add_pack_remove_button(state, pid, pack, &mut btns);
    btns.extend(pack_close_buttons());
    let gui = serde_json::json!({
        "title": "Крафтер",
        "text": text,
        "buttons": btns,
        "back": false
    });
    format!("horb:{gui}")
}
