//! Чат, команды `/`, смерть, вспомогательные обработчики.
use serde_json::json;
use crate::net::session::outbound::chat_sync::send_chat_init;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::play::spawn::spawn_crystal_box;
use crate::net::session::prelude::*;
use crate::game::player::PlayerInventory;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, modify_pack_with_db,
};
use crate::net::session::social::clans::{
    handle_clan_create, handle_clan_kick_by_name, handle_clan_leave,
};

const ADMIN_COMMAND_HELP: &str = concat!(
    "Админские команды:\n",
    "/give ITEM_ID AMOUNT — выдать предмет\n",
    "/money AMOUNT — добавить денег игроку\n",
    "/moneyall AMOUNT — добавить денег всем игрокам\n",
    "/tp X Y — телепортироваться\n",
    "/heal — восстановить HP\n",
    "/pack owner X Y OWNER_ID — сменить владельца здания\n",
    "/pack clan X Y CLAN_ID — изменить клан здания\n",
    "/pack move X Y NX NY — переместить здание\n",
    "/pack type X Y TYPE — сменить тип здания (T/R/G/M/U/L/F/O)\n",
    "/admin — показать справку по админ-командам\n",
    "/adminhelp — показать справку по админ-командам",
);

const CMD_USAGE_GIVE: &str = "Использование: /give ITEM_ID AMOUNT";
const CMD_USAGE_MONEY: &str = "Использование: /money AMOUNT";
const CMD_USAGE_MONEY_ALL: &str = "Использование: /moneyall AMOUNT";
const CMD_USAGE_TP: &str = "Использование: /tp X Y";
const CMD_USAGE_PACK_OWNER: &str = "Использование: /pack owner X Y OWNER_ID";
const CMD_USAGE_PACK_CLAN: &str = "Использование: /pack clan X Y CLAN_ID";
const CMD_USAGE_PACK_MOVE: &str = "Использование: /pack move X Y NX NY";
const CMD_USAGE_PACK_TYPE: &str = "Использование: /pack type X Y TYPE (T/R/G/M/U/L/F/O)";
const CMD_USAGE_PACK: &str = "Команды: /pack owner X Y OWNER_ID | /pack clan X Y CLAN_ID | /pack move X Y NX NY | /pack type X Y TYPE";
const CMD_USAGE_CLAN_CREATE: &str = "Использование: /clan create ИМЯ ТЕГ";
const CMD_USAGE_CLAN_KICK: &str = "Использование: /clan kick ИМЯ";
const CMD_USAGE_CLAN: &str = "Команды: /clan create ИМЯ ТЕГ | /clan leave | /clan kick ИМЯ";

const ADMIN_COMMAND_NO_RIGHTS: &str = "Нет прав на админ-команду";

// ─── Other handlers ─────────────────────────────────────────────────────────

pub fn handle_geo(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        if let Some(pos) = ecs.get::<crate::game::player::PlayerPosition>(entity) {
            // TODO: реализовать geo-стек (World.GetProp). Пока пустая строка.
            send_u_packet(tx, "GE", &geo("").1);
        }
    });
}

pub fn handle_auto_dig_toggle(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let new_val = state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut settings = ecs.get_mut::<crate::game::player::PlayerSettings>(entity)?;
        settings.auto_dig = !settings.auto_dig;
        let val = settings.auto_dig;
        if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
            flags.dirty = true;
        }
        Some(val)
    }).flatten();

    if let Some(val) = new_val {
        send_u_packet(tx, "BD", &auto_digg(val).1);
    }
}

pub fn handle_local_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    handle_chat_text(state, tx, pid, msg);
}

pub fn handle_chat_message(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let msg = String::from_utf8_lossy(payload).to_string();
    handle_chat_text(state, tx, pid, &msg);
}

fn handle_chat_text(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    let msg = msg.trim();
    if msg.is_empty() { return; }
    if handle_chat_command_if_present(state, tx, pid, msg) { return; }
    broadcast_player_chat(state, pid, msg);
}

fn handle_chat_command_if_present(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) -> bool {
    let msg = msg.trim();
    if !msg.starts_with('/') { return false; }
    handle_chat_command(state, tx, pid, msg);
    true
}

fn broadcast_player_chat(state: &Arc<GameState>, pid: PlayerId, msg: &str) {
    let data = state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        Some((pos.x, pos.y, meta.name.clone()))
    }).flatten();

    if let Some((px, py, name)) = data {
        let text = format!("{name}: {msg}");
        let (cx, cy) = World::chunk_pos(px, py);
        let chat_sub = hb_chat(net_u16_nonneg(pid), net_u16_nonneg(px), net_u16_nonneg(py), &text);
        state.broadcast_to_nearby(cx, cy, &encode_hb_bundle(&hb_bundle(&[chat_sub]).1), None);
    }
}

pub(crate) fn send_ok(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

pub(crate) fn send_admin_help(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_ok(tx, "Админ-команды", ADMIN_COMMAND_HELP);
}

fn ensure_admin(tx: &mpsc::UnboundedSender<Vec<u8>>, state: &Arc<GameState>, pid: PlayerId) -> bool {
    if is_admin_command(state, pid) { true }
    else { send_ok(tx, "Ошибка", ADMIN_COMMAND_NO_RIGHTS); false }
}

pub fn handle_channel_chat(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, payload: &[u8]) {
    let text = extract_channel_message_text(payload);
    if text.is_empty() { return; }
    if handle_chat_command_if_present(state, tx, pid, &text) { return; }

    let p_data = state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        let stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        let ui = ecs.get::<crate::game::player::PlayerUI>(entity)?;
        Some((meta.name.clone(), meta.id, stats.clan_id.unwrap_or(0), ui.current_chat.clone()))
    }).flatten();

    let Some((nickname, user_id, clan_id, channel_tag)) = p_data else { return; };
    let time = Instant::now().duration_since(std::time::Instant::now()).as_secs() as i64; // dummy
    let msg = ChatMessage { time, clan_id, user_id, nickname: nickname.clone(), text: text.clone(), color: 1 };

    let db_tag = if channel_tag == "CLAN" { format!("CLAN_{clan_id}") } else { channel_tag.clone() };
    let _ = state.db.add_chat_message(&db_tag, &nickname, &text);

    let (is_global, packet_data) = {
        let mut channels = state.chat_channels.write();
        let target_ch = channels.iter_mut().find(|c| c.tag == channel_tag);
        if let Some(ch) = target_ch {
            ch.messages.push_back(msg.clone());
            if ch.messages.len() > 50 { ch.messages.pop_front(); }
            (ch.global, chat_messages(&channel_tag, &[msg]).1)
        } else if channel_tag == "CLAN" && clan_id != 0 {
            (false, chat_messages("CLAN", &[msg]).1)
        } else { return; }
    };
    send_channel_packet_to_players(state, &packet_data, if is_global { None } else { Some(clan_id) });
}

pub fn extract_channel_message_text(payload: &[u8]) -> String {
    let raw = String::from_utf8_lossy(payload).trim().to_string();
    let Some((prefix, body)) = raw.split_once('#') else { return raw; };
    if prefix.contains(':') { body.to_string() } else { raw }
}

pub fn handle_chat_switch(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, payload: &[u8]) {
    let tag = String::from_utf8_lossy(payload).trim().to_string();
    if tag.is_empty() { return; }
    if !state.chat_channels.read().iter().any(|c| c.tag == tag) { return; }
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) { ui.current_chat = tag.clone(); }
        Some(())
    });
    send_chat_init(state, tx, pid, &tag);
}

/// TY `Chin` — запрос состояния чата (`"_"` или `1:TAG:…` по референсу).
pub fn handle_chat_init_ty(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let s = String::from_utf8_lossy(payload).trim().to_string();
    let current_tag = state
        .query_player(pid, |ecs, entity| {
            ecs.get::<crate::game::player::PlayerUI>(entity)
                .map(|ui| ui.current_chat.clone())
        })
        .flatten()
        .unwrap_or_else(|| "FED".to_string());

    if s.is_empty() || s == "_" {
        send_chat_init(state, tx, pid, &current_tag);
        return;
    }
    if let Some(rest) = s.strip_prefix("1:") {
        let tag = rest
            .split(':')
            .next()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        if tag.is_empty() || !state.chat_channels.read().iter().any(|c| c.tag == tag) {
            send_chat_init(state, tx, pid, &current_tag);
            return;
        }
        state.modify_player(pid, |ecs, entity| {
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                ui.current_chat = tag.clone();
            }
            Some(())
        });
        send_chat_init(state, tx, pid, &tag);
        return;
    }
    send_chat_init(state, tx, pid, &current_tag);
}

/// TY программатор — как `Session.PROG/PDEL/pRST/PREN` + `StaticGUI` в server_reference (без `#p` с сервера).
pub fn handle_prog_ty(tx: &mpsc::UnboundedSender<Vec<u8>>, event: &str, payload: &[u8]) {
    match event {
        "PROG" => {
            let running = !payload.is_empty();
            send_u_packet(tx, "@P", &programmator_status(running).1);
        }
        "PDEL" | "PREN" => {
            send_u_packet(tx, "@P", &programmator_status(false).1);
        }
        "pRST" => {
            send_u_packet(tx, "@P", &programmator_status(true).1);
        }
        _ => {}
    }
}

/// TY `Sett` → `Settings.SendSettingsGUI` в server_reference (упрощённое окно настроек).
pub fn handle_sett_ty(
    _state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    _pid: PlayerId,
    _payload: &[u8],
) {
    let gui = serde_json::json!({
        "title": "НА СТРОЙКЕ",
        "text": "Упрощённое окно (ref Settings.SendSettingsGUI). Полный RichList в Rust пока не портирован.",
        "buttons": ["ВЫЙТИ", "exit"],
        "back": false
    });
    send_u_packet(tx, "GU", format!("horb:{gui}").as_bytes());
}

pub fn handle_chat_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, msg: &str) {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    if parts.is_empty() { return; }
    let cmd = parts[0]; let args = &parts[1..];
    match cmd {
        "/give" => handle_chat_give_command(state, tx, pid, args),
        "/money" => handle_chat_money_command(state, tx, pid, args),
        "/moneyall" => handle_chat_money_all_command(state, tx, pid, args),
        "/tp" => handle_chat_teleport_command(state, tx, pid, args),
        "/heal" => handle_chat_heal_command(state, tx, pid),
        "/clan" => handle_chat_clan_command(state, tx, pid, args),
        "/pack" => handle_chat_pack_command(state, tx, pid, args),
        "/admin" | "/adminhelp" => if ensure_admin(tx, state, pid) { send_admin_help(tx); },
        _ => if is_admin_command(state, pid) { send_admin_help(tx); } else { send_ok(tx, "Ошибка", &format!("Неизвестная команда: {cmd}")); }
    }
}

fn handle_chat_give_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) { return; }
    let item_id = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_GIVE) { Some(id) => id, None => return };
    let amount = match parse_optional_arg_with_default::<i32>(tx, args, 1, 1, CMD_USAGE_GIVE) { Some(a) => a, None => return };
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut inv = ecs.get_mut::<PlayerInventory>(entity)?;
        *inv.items.entry(item_id).or_insert(0) += amount;
        let inv_clone = inv.clone();
        if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { flags.dirty = true; }
        send_inventory(tx, &inv_clone);
        Some(())
    });
}

fn handle_chat_money_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) { return; }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY) { Some(a) => a, None => return };
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
        s.money += amount;
        let (m, c) = (s.money, s.creds);
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { f.dirty = true; }
        send_u_packet(tx, "P$", &money(m, c).1);
        Some(())
    });
}

fn handle_chat_money_all_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) { return; }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY_ALL) { Some(a) => a, None => return };
    if let Ok(count) = state.db.add_money_to_all(amount) {
        for entry in &state.active_players {
            state.modify_player(*entry.key(), |ecs: &mut bevy_ecs::prelude::World, entity| {
                let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                s.money = s.money.saturating_add(amount);
                let (m, c) = (s.money, s.creds);
                if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { f.dirty = true; }
                if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) { send_u_packet(&conn.tx, "P$", &money(m, c).1); }
                Some(())
            });
        }
        send_ok(tx, "Банк", &format!("Выдано $ {amount} всем игрокам ({count})"));
    }
}

fn handle_chat_teleport_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) { return; }
    let x = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_TP) { Some(v) => v, None => return };
    let y = match parse_required_arg::<i32>(tx, args, 1, CMD_USAGE_TP) { Some(v) => v, None => return };
    if !state.world.valid_coord(x, y) { send_ok(tx, "Ошибка", "Координаты вне карты"); return; }
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut pos = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
        pos.x = x; pos.y = y;
        if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) { ui.current_window = None; }
        if let Some(mut view) = ecs.get_mut::<crate::game::player::PlayerView>(entity) { view.last_chunk = None; view.visible_chunks.clear(); }
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { f.dirty = true; }
        Some(())
    });
    send_u_packet(tx, "@T", &tp(x, y).1);
    check_chunk_changed(state, tx, pid);
}

fn handle_chat_heal_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    if !ensure_admin(tx, state, pid) { return; }
    state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
        let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
        s.health = s.max_health;
        let (h, mh) = (s.health, s.max_health);
        if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { f.dirty = true; }
        send_u_packet(tx, "@L", &health(h, mh).1);
        Some(())
    });
}

fn handle_chat_clan_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    let Some((sub, subargs)) = args.split_first() else { send_ok(tx, "Клан", CMD_USAGE_CLAN); return };
    match *sub {
        "create" => handle_chat_clan_create_command(state, tx, pid, subargs),
        "leave" => handle_clan_leave(state, tx, pid),
        "kick" => handle_chat_clan_kick_command(state, tx, pid, subargs),
        _ => send_ok(tx, "Клан", CMD_USAGE_CLAN),
    }
}

fn handle_chat_clan_create_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    let name = args.first().copied().unwrap_or("");
    let tag = args.get(1).copied().unwrap_or("");
    if name.is_empty() || tag.is_empty() { send_ok(tx, "Клан", CMD_USAGE_CLAN_CREATE); return; }
    handle_clan_create(state, tx, pid, name, tag);
}

fn handle_chat_clan_kick_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    let target = args.first().copied().unwrap_or("");
    if target.is_empty() { send_ok(tx, "Клан", CMD_USAGE_CLAN_KICK); return; }
    handle_clan_kick_by_name(state, tx, pid, target);
}

fn handle_chat_pack_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId, args: &[&str]) {
    if !ensure_admin(tx, state, pid) { return; }
    let Some((sub, subargs)) = args.split_first() else { send_ok(tx, "Пак", CMD_USAGE_PACK); return };
    match *sub {
        "owner" => handle_pack_owner_command(state, tx, subargs),
        "clan" => handle_pack_clan_command(state, tx, subargs),
        "move" => handle_pack_move_command(state, tx, subargs),
        "type" => handle_pack_type_command(state, tx, subargs),
        _ => send_ok(tx, "Пак", CMD_USAGE_PACK),
    }
}

fn parse_required_arg<T: std::str::FromStr>(tx: &mpsc::UnboundedSender<Vec<u8>>, args: &[&str], idx: usize, usage: &str) -> Option<T> {
    args.get(idx).and_then(|s| s.parse().ok()).or_else(|| { send_ok(tx, "Ошибка", usage); None })
}

fn parse_optional_arg_with_default<T: std::str::FromStr>(tx: &mpsc::UnboundedSender<Vec<u8>>, args: &[&str], idx: usize, def: T, usage: &str) -> Option<T> {
    args.get(idx).map(|s| s.parse().ok().or_else(|| { send_ok(tx, "Ошибка", usage); None })).unwrap_or(Some(def))
}

fn handle_pack_owner_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_OWNER, 0, 1) { Some(p) => p, None => return };
    let owner = parts.get(2).and_then(|s| s.parse().ok());
    if let Some(oid) = owner {
        if let Ok(_) = modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut o) = ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity) { o.owner_id = oid; }
        }) { send_ok(tx, "Пак", "Владелец обновлен"); }
        else { send_ok(tx, "Ошибка", "Здание не найдено"); }
    }
}

fn handle_pack_clan_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_CLAN, 0, 1) { Some(p) => p, None => return };
    let clan = parts.get(2).and_then(|s| s.parse().ok());
    if let Some(cid) = clan {
        if let Ok(_) = modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut o) = ecs.get_mut::<crate::game::buildings::BuildingOwnership>(entity) { o.clan_id = cid; }
        }) { send_ok(tx, "Пак", "Клан обновлен"); }
        else { send_ok(tx, "Ошибка", "Здание не найдено"); }
    }
}

fn handle_pack_move_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_MOVE, 0, 1) { Some(p) => p, None => return };
    let nx = parts.get(2).and_then(|s| s.parse().ok());
    let ny = parts.get(3).and_then(|s| s.parse().ok());
    if let (Some(nx), Some(ny)) = (nx, ny) {
        // NOTE: Move requires updating building_index, but modify_pack_with_db doesn't handle it yet.
        // For now let's just update GridPosition and hope for the best or implement full move logic.
        if let Ok(_) = modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut p) = ecs.get_mut::<crate::game::buildings::GridPosition>(entity) { p.x = nx; p.y = ny; }
        }) { 
            if let Some((_, entity)) = state.building_index.remove(&(x, y)) { state.building_index.insert((nx, ny), entity); }
            send_ok(tx, "Пак", "Позиция обновлена"); 
        }
        else { send_ok(tx, "Ошибка", "Здание не найдено"); }
    }
}

fn handle_pack_type_command(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, parts: &[&str]) {
    let (x, y) = match parse_pack_pos(parts, tx, CMD_USAGE_PACK_TYPE, 0, 1) { Some(p) => p, None => return };
    let t = parts.get(2).and_then(|&s| crate::game::buildings::PackType::from_str(s));
    if let Some(nt) = t {
        let ex = building_extra_for_pack_type(nt);
        if let Ok(_) = modify_pack_with_db(state, x, y, |ecs: &mut bevy_ecs::prelude::World, entity| {
            if let Some(mut m) = ecs.get_mut::<crate::game::buildings::BuildingMetadata>(entity) { m.pack_type = nt; }
            if let Some(mut s) = ecs.get_mut::<crate::game::buildings::BuildingStats>(entity) {
                s.max_charge = ex.max_charge; s.charge = s.charge.min(ex.max_charge);
                s.max_hp = ex.max_hp; s.hp = s.hp.min(ex.max_hp);
            }
        }) { send_ok(tx, "Пак", "Тип обновлен"); }
        else { send_ok(tx, "Ошибка", "Здание не найдено"); }
    }
}

fn parse_pack_pos(parts: &[&str], tx: &mpsc::UnboundedSender<Vec<u8>>, usage: &str, xi: usize, yi: usize) -> Option<(i32, i32)> {
    let x = parts.get(xi).and_then(|s| s.parse().ok());
    let y = parts.get(yi).and_then(|s| s.parse().ok());
    match (x, y) { (Some(x), Some(y)) => Some((x, y)), _ => { send_ok(tx, "Пак", usage); None } }
}

pub(crate) fn is_admin_command(state: &Arc<GameState>, pid: PlayerId) -> bool {
    state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| { ecs.get::<crate::game::player::PlayerStats>(entity).is_some_and(|s| s.role == 2) }).unwrap_or(false)
}

pub fn handle_whoi(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, ids: &[i32]) {
    let parts: Vec<String> = ids.iter().map(|&id| {
        let name = state.query_player(id, |ecs: &bevy_ecs::prelude::World, entity| { ecs.get::<crate::game::player::PlayerMetadata>(entity).map(|m| m.name.clone()) }).flatten()
            .or_else(|| state.db.get_player_by_id(id).ok().flatten().map(|p| p.name)).unwrap_or_default();
        format!("{id}:{name}")
    }).collect();
    send_u_packet(tx, "NL", parts.join(",").as_bytes());
}

pub fn handle_death(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    let data = state.query_player(pid, |ecs: &bevy_ecs::prelude::World, entity| {
        let s = ecs.get::<crate::game::player::PlayerStats>(entity)?;
        let p = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
        let m = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
        Some((p.x, p.y, s.crystals, m.resp_x, m.resp_y, s.max_health))
    }).flatten();

    if let Some((dx, dy, cry, rx_p, ry_p, mh)) = data {
        if cry.iter().sum::<i64>() > 0 {
            state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| { if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) { s.crystals = [0; 6]; } Some(()) });
            if let Some((bx, by)) = spawn_crystal_box(state, dx, dy) { broadcast_cell_update(state, bx, by); }
            let fx = hb_fx(dx as u16, dy as u16, 2);
            state.broadcast_to_nearby(World::chunk_pos(dx, dy).0, World::chunk_pos(dx, dy).1, &encode_hb_bundle(&hb_bundle(&[fx]).1), None);
        }
        let (rx, ry) = if let (Some(x), Some(y)) = (rx_p, ry_p) { if state.get_pack_at(x, y).is_some_and(|p| p.pack_type == crate::game::buildings::PackType::Resp && p.charge > 0.0) { (x+2, y) } else { (10, 10) } } else { (10, 10) };
        state.modify_player(pid, |ecs: &mut bevy_ecs::prelude::World, entity| {
            let mut p = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
            p.x = rx; p.y = ry;
            if let Some(mut s) = ecs.get_mut::<crate::game::player::PlayerStats>(entity) { s.health = mh; }
            if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) { ui.current_window = None; }
            if let Some(mut v) = ecs.get_mut::<crate::game::player::PlayerView>(entity) { v.last_chunk = None; v.visible_chunks.clear(); }
            if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) { f.dirty = true; }
            Some(())
        });
        send_u_packet(tx, "@T", &tp(rx, ry).1); send_u_packet(tx, "@L", &health(mh, mh).1); send_u_packet(tx, "@B", &basket(&[0; 6], 1000).1);
        check_chunk_changed(state, tx, pid);
    }
}

fn send_channel_packet_to_players(state: &Arc<GameState>, data: &[u8], clan: Option<i32>) {
    let pkt = make_u_packet_bytes("mU", data);
    for entry in &state.active_players {
        state.query_player(*entry.key(), |ecs: &bevy_ecs::prelude::World, entity| {
            if let (Some(s), Some(c)) = (ecs.get::<crate::game::player::PlayerStats>(entity), ecs.get::<crate::game::player::PlayerConnection>(entity)) {
                if clan.is_none_or(|id| s.clan_id == Some(id)) { let _ = c.tx.send(pkt.clone()); }
            }
        });
    }
}
