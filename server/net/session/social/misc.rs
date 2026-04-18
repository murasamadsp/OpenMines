//! Чат, команды `/`, смерть, вспомогательные обработчики.
use crate::net::session::outbound::chat_sync::send_chat_init;
use crate::net::session::outbound::inventory_sync::send_inventory;
use crate::net::session::play::chunks::check_chunk_changed;
use crate::net::session::play::dig_build::broadcast_cell_update;
use crate::net::session::play::spawn::spawn_crystal_box;
use crate::net::session::prelude::*;
use crate::net::session::social::buildings::{
    building_extra_for_pack_type, update_pack_with_world_sync,
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
const CMD_ERROR_AMOUNT_POSITIVE: &str = "Сумма должна быть больше нуля";

const ADMIN_COMMAND_NO_RIGHTS: &str = "Нет прав на админ-команду";

// ─── Other handlers ─────────────────────────────────────────────────────────

pub fn handle_geo(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    if let Some(p) = state.active_players.get(&pid) {
        send_u_packet(tx, "GE", &geo(p.data.x, p.data.y).1);
    }
}

pub fn handle_auto_dig_toggle(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    let new_val = {
        let Some(mut p) = state.active_players.get_mut(&pid) else {
            return;
        };
        p.auto_dig = !p.auto_dig;
        p.data.auto_dig = p.auto_dig;
        p.auto_dig
    };
    send_u_packet(tx, "BD", &auto_digg(new_val).1);
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
    if msg.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, msg) {
        return;
    }
    broadcast_player_chat(state, pid, msg);
}

fn handle_chat_command_if_present(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) -> bool {
    let msg = msg.trim();
    if !msg.starts_with('/') {
        return false;
    }
    handle_chat_command(state, tx, pid, msg);
    true
}

fn broadcast_player_chat(state: &Arc<GameState>, pid: PlayerId, msg: &str) {
    let (px, py, name) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        (p.data.x, p.data.y, p.data.name.clone())
    };

    let text = format!("{name}: {msg}");
    let (cx, cy) = World::chunk_pos(px, py);
    let chat_sub = hb_chat(
        net_u16_nonneg(pid),
        net_u16_nonneg(px),
        net_u16_nonneg(py),
        &text,
    );
    let data = encode_hb_bundle(&hb_bundle(&[chat_sub]).1);
    state.broadcast_to_nearby(cx, cy, &data, None);
}

fn send_ok(tx: &mpsc::UnboundedSender<Vec<u8>>, title: &str, text: &str) {
    send_u_packet(tx, "OK", &ok_message(title, text).1);
}

fn send_admin_help(tx: &mpsc::UnboundedSender<Vec<u8>>) {
    send_ok(tx, "Админ-команды", ADMIN_COMMAND_HELP);
}

fn ensure_admin(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    state: &Arc<GameState>,
    pid: PlayerId,
) -> bool {
    if is_admin_command(state, pid) {
        true
    } else {
        send_ok(tx, "Ошибка", ADMIN_COMMAND_NO_RIGHTS);
        false
    }
}

/// Handle "Chat" TY event — post message to player's current channel.
pub fn handle_channel_chat(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let text = extract_channel_message_text(payload);
    if text.is_empty() {
        return;
    }
    if handle_chat_command_if_present(state, tx, pid, &text) {
        tracing::debug!("Chat command from player {pid}: {text}");
        return;
    }

    let (nickname, user_id, clan_id, channel_tag) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        (
            p.data.name.clone(),
            p.data.id,
            p.data.clan_id.unwrap_or(0),
            p.current_chat.clone(),
        )
    };

    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
        / 60_000;

    let msg = ChatMessage {
        time,
        clan_id,
        user_id,
        nickname: nickname.clone(),
        text: text.clone(),
        color: 1,
    };

    // Save to DB
    let db_tag = if channel_tag == "CLAN" {
        format!("CLAN_{clan_id}")
    } else {
        channel_tag.clone()
    };
    if let Err(e) = state.db.add_chat_message(&db_tag, &nickname, &text) {
        tracing::error!("Failed to save chat message to DB: {e}");
    }

    // Add message to channel, cap at 50
    let (is_global, packet_data) = {
        let mut channels = state.chat_channels.write();
        let target_ch = channels.iter_mut().find(|c| c.tag == channel_tag);

        if let Some(ch) = target_ch {
            ch.messages.push_back(msg.clone());
            if ch.messages.len() > 50 {
                ch.messages.pop_front();
            }
            let data = chat_messages(&channel_tag, &[msg]).1;
            (ch.global, data)
        } else if channel_tag == "CLAN" && clan_id != 0 {
            // Clan chat is special and doesn't have a global entry in state.chat_channels (for now)
            let data = chat_messages("CLAN", &[msg]).1;
            (false, data)
        } else {
            return;
        }
    };

    // Broadcast mU to relevant players
    let target_clan = if is_global { None } else { Some(clan_id) };
    send_channel_packet_to_players(state, &packet_data, target_clan);
}

pub fn extract_channel_message_text(payload: &[u8]) -> String {
    let raw = String::from_utf8_lossy(payload).trim().to_string();
    let Some((prefix, body)) = raw.split_once('#') else {
        return raw;
    };
    if prefix.contains(':') {
        body.to_string()
    } else {
        raw
    }
}

/// Handle "Chin" TY event — switch player's active channel.
pub fn handle_chat_switch(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    payload: &[u8],
) {
    let tag = String::from_utf8_lossy(payload).trim().to_string();
    if tag.is_empty() {
        return;
    }

    // Validate channel exists
    {
        let channels = state.chat_channels.read();
        if !channels.iter().any(|c| c.tag == tag) {
            return;
        }
    }

    // Update player's current_chat
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.current_chat = tag.clone();
    }

    send_chat_init(state, tx, pid, &tag);
}

pub fn handle_chat_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    msg: &str,
) {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    if parts.is_empty() {
        return;
    }
    let cmd = parts.first().copied().unwrap_or("");
    let args = &parts[1..];
    match cmd {
        "/give" => handle_chat_give_command(state, tx, pid, args),
        "/money" => handle_chat_money_command(state, tx, pid, args),
        "/moneyall" => handle_chat_money_all_command(state, tx, pid, args),
        "/tp" => handle_chat_teleport_command(state, tx, pid, args),
        "/heal" => handle_chat_heal_command(state, tx, pid),
        "/clan" => handle_chat_clan_command(state, tx, pid, args),
        "/pack" => handle_chat_pack_command(state, tx, pid, args),
        "/adminhelp" | "/admin" => {
            if !ensure_admin(tx, state, pid) {
                return;
            }
            send_admin_help(tx);
        }
        _ => {
            tracing::debug!("Unknown chat command: {msg} from player {pid}");
            if is_admin_command(state, pid) {
                send_admin_help(tx);
            } else {
                send_ok(tx, "Ошибка", &format!("Неизвестная команда: {cmd}"));
            }
        }
    }
}

fn handle_chat_give_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let item_id = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_GIVE) {
        Some(item_id) => item_id,
        None => return,
    };
    let amount = match parse_optional_arg_with_default::<i32>(tx, args, 1, 1, CMD_USAGE_GIVE) {
        Some(amount) => amount,
        None => return,
    };
    if amount <= 0 {
        send_ok(tx, "Ошибка", CMD_ERROR_AMOUNT_POSITIVE);
        return;
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        *p.data.inventory.entry(item_id).or_insert(0) += amount;
        let inv = p.data.inventory.clone();
        let sel = p.inv_selected;
        drop(p);
        send_inventory(tx, &inv, sel);
    }
    tracing::info!("Admin /give item={item_id} amount={amount} to player {pid}");
}

fn handle_chat_money_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY) {
        Some(amount) => amount,
        None => return,
    };
    if amount <= 0 {
        send_ok(tx, "Ошибка", CMD_ERROR_AMOUNT_POSITIVE);
        return;
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.data.money += amount;
        let m = p.data.money;
        let c = p.data.creds;
        drop(p);
        send_u_packet(tx, "P$", &money(m, c).1);
    }
    tracing::info!("Admin /money amount={amount} to player {pid}");
}

fn handle_chat_money_all_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let amount = match parse_required_arg::<i64>(tx, args, 0, CMD_USAGE_MONEY_ALL) {
        Some(amount) => amount,
        None => return,
    };
    if amount <= 0 {
        send_ok(tx, "Ошибка", CMD_ERROR_AMOUNT_POSITIVE);
        return;
    }
    match state.db.add_money_to_all(amount) {
        Ok(count) => {
            let ids: Vec<PlayerId> = state.active_players.iter().map(|p| *p.key()).collect();
            for pid in ids {
                if let Some(mut player) = state.active_players.get_mut(&pid) {
                    player.data.money = player.data.money.saturating_add(amount);
                    let m = player.data.money;
                    let c = player.data.creds;
                    let player_tx = player.tx.clone();
                    drop(player);
                    send_u_packet(&player_tx, "P$", &money(m, c).1);
                }
            }
            send_ok(
                tx,
                "Банк",
                &format!("Выдано $ {amount} всем игрокам (DB rows: {count})"),
            );
            tracing::info!("Admin /moneyall amount={amount} updated rows={count}");
        }
        Err(err) => {
            send_ok(tx, "Ошибка", &format!("Не удалось выдать деньги: {err}"));
        }
    }
}

fn handle_chat_teleport_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let x = match parse_required_arg::<i32>(tx, args, 0, CMD_USAGE_TP) {
        Some(x) => x,
        None => return,
    };
    let y = match parse_required_arg::<i32>(tx, args, 1, CMD_USAGE_TP) {
        Some(y) => y,
        None => return,
    };
    if !state.world.valid_coord(x, y) {
        send_ok(tx, "Ошибка", "Координаты вне карты");
        return;
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.data.x = x;
        p.data.y = y;
        // Телепорт должен сбрасывать GUI и окно чанков, иначе игрок может остаться "в окне" и
        // сервер будет откатывать движения, а чанки вокруг новой позиции не прогрузятся.
        p.current_window = None;
        p.last_chunk = None;
        p.visible_chunks.clear();
    }
    send_u_packet(tx, "@T", &tp(x, y).1);
    check_chunk_changed(state, tx, pid);
    tracing::info!("Admin /tp player {pid} to ({x}, {y})");
}

fn handle_chat_heal_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.data.health = p.data.max_health;
        let h = p.data.health;
        let mh = p.data.max_health;
        drop(p);
        send_u_packet(tx, "@L", &health(h, mh).1);
    }
    tracing::info!("Admin /heal player {pid}");
}

fn handle_chat_clan_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    let Some((subcmd, subargs)) = args.split_first() else {
        send_ok(tx, "Клан", CMD_USAGE_CLAN);
        return;
    };

    match *subcmd {
        "create" => handle_chat_clan_create_command(state, tx, pid, subargs),
        "leave" => handle_clan_leave(state, tx, pid),
        "kick" => handle_chat_clan_kick_command(state, tx, pid, subargs),
        _ => send_ok(tx, "Клан", CMD_USAGE_CLAN),
    }
}

fn handle_chat_clan_create_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    let name = args.first().copied().unwrap_or("");
    let tag = args.get(1).copied().unwrap_or("");
    if name.is_empty() || tag.is_empty() {
        send_ok(tx, "Клан", CMD_USAGE_CLAN_CREATE);
        return;
    }
    if tag.len() > 3 {
        send_ok(tx, "Клан", "Тег должен быть 1-3 символа");
        return;
    }
    handle_clan_create(state, tx, pid, name, tag);
}

fn handle_chat_clan_kick_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    let target_name = args.first().copied().unwrap_or("");
    if target_name.is_empty() {
        send_ok(tx, "Клан", CMD_USAGE_CLAN_KICK);
        return;
    }
    handle_clan_kick_by_name(state, tx, pid, target_name);
}

fn handle_chat_pack_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    pid: PlayerId,
    args: &[&str],
) {
    if !ensure_admin(tx, state, pid) {
        return;
    }
    let Some((subcmd, subargs)) = args.split_first() else {
        send_ok(tx, "Пак", CMD_USAGE_PACK);
        return;
    };
    match *subcmd {
        "owner" => handle_pack_owner_command(state, tx, subargs),
        "clan" => handle_pack_clan_command(state, tx, subargs),
        "move" => handle_pack_move_command(state, tx, subargs),
        "type" => handle_pack_type_command(state, tx, subargs),
        _ => send_ok(tx, "Пак", CMD_USAGE_PACK),
    }
}

fn parse_required_arg<T: std::str::FromStr>(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    args: &[&str],
    index: usize,
    usage: &str,
) -> Option<T> {
    let Some(value) = args.get(index) else {
        send_ok(tx, "Ошибка", usage);
        return None;
    };
    value.parse::<T>().map_or_else(
        |_| {
            send_ok(tx, "Ошибка", usage);
            None
        },
        Some,
    )
}

fn parse_optional_arg_with_default<T>(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    args: &[&str],
    index: usize,
    default: T,
    usage: &str,
) -> Option<T>
where
    T: std::str::FromStr,
{
    let Some(value) = args.get(index) else {
        return Some(default);
    };
    value.parse::<T>().map_or_else(
        |_| {
            send_ok(tx, "Ошибка", usage);
            None
        },
        Some,
    )
}

fn handle_pack_owner_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
    let (x, y) = match parse_pack_coordinates(parts, tx, CMD_USAGE_PACK_OWNER, 0, 1) {
        Some(pos) => pos,
        None => return,
    };
    let new_owner = parts.get(2).and_then(|s| s.parse::<i32>().ok());
    let Some(new_owner) = new_owner else {
        send_ok(tx, "Пак", CMD_USAGE_PACK_OWNER);
        return;
    };

    match state.db.get_player_by_id(new_owner) {
        Ok(Some(_)) => {}
        Ok(None) => {
            send_ok(tx, "Ошибка", "Игрок не найден");
            return;
        }
        Err(_) => {
            send_ok(tx, "Ошибка", "Ошибка проверки владельца");
            return;
        }
    }

    apply_pack_update(
        state,
        tx,
        x,
        y,
        "Владелец здания обновлен",
        "Не удалось обновить владельца",
        |pack| {
            pack.owner_id = new_owner;
        },
    );
}

fn handle_pack_clan_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
    let (x, y) = match parse_pack_coordinates(parts, tx, CMD_USAGE_PACK_CLAN, 0, 1) {
        Some(pos) => pos,
        None => return,
    };
    let new_clan = parts.get(2).and_then(|s| s.parse::<i32>().ok());
    let Some(new_clan) = new_clan else {
        send_ok(tx, "Пак", CMD_USAGE_PACK_CLAN);
        return;
    };

    apply_pack_update(
        state,
        tx,
        x,
        y,
        "Клан здания обновлен",
        "Не удалось обновить клан",
        |pack| {
            pack.clan_id = new_clan;
        },
    );
}

fn handle_pack_move_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
    let (x, y) = match parse_pack_coordinates(parts, tx, CMD_USAGE_PACK_MOVE, 0, 1) {
        Some(pos) => pos,
        None => return,
    };
    let nx = parts.get(2).and_then(|s| s.parse::<i32>().ok());
    let ny = parts.get(3).and_then(|s| s.parse::<i32>().ok());
    let (Some(nx), Some(ny)) = (nx, ny) else {
        send_ok(tx, "Пак", CMD_USAGE_PACK_MOVE);
        return;
    };

    if (x, y) == (nx, ny) {
        send_ok(tx, "Пак", "Новая позиция совпадает с текущей");
        return;
    }

    apply_pack_update(
        state,
        tx,
        x,
        y,
        "Положение здания обновлено",
        "Не удалось переместить здание",
        |pack| {
            pack.x = nx;
            pack.y = ny;
        },
    );
}

fn handle_pack_type_command(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    parts: &[&str],
) {
    let (x, y) = match parse_pack_coordinates(parts, tx, CMD_USAGE_PACK_TYPE, 0, 1) {
        Some(pos) => pos,
        None => return,
    };
    let raw_type = parts.get(2).copied();
    let Some(raw_type) = raw_type else {
        send_ok(tx, "Пак", CMD_USAGE_PACK_TYPE);
        return;
    };
    let new_type = match PackType::from_str(raw_type) {
        Some(t) => t,
        None => {
            send_ok(tx, "Ошибка", "Неизвестный тип здания");
            return;
        }
    };

    let extra = building_extra_for_pack_type(new_type);
    apply_pack_update(
        state,
        tx,
        x,
        y,
        "Тип здания обновлен",
        "Не удалось сменить тип",
        |pack| {
            pack.pack_type = new_type;
            pack.charge = pack.charge.min(extra.max_charge);
            pack.max_charge = extra.max_charge;
            pack.cost = extra.cost;
            pack.hp = pack.hp.min(extra.max_hp);
            pack.max_hp = extra.max_hp;
        },
    );
}

fn parse_pack_coordinates(
    parts: &[&str],
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    usage: &str,
    x_idx: usize,
    y_idx: usize,
) -> Option<(i32, i32)> {
    let x = parts.get(x_idx).and_then(|s| s.parse::<i32>().ok());
    let y = parts.get(y_idx).and_then(|s| s.parse::<i32>().ok());
    match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => {
            send_ok(tx, "Пак", usage);
            None
        }
    }
}

fn apply_pack_update(
    state: &Arc<GameState>,
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    x: i32,
    y: i32,
    success_message: &str,
    error_prefix: &str,
    apply: impl FnOnce(&mut PackData),
) {
    let Some(pack) = state.get_pack_at(x, y).map(|p| p.clone()) else {
        send_ok(tx, "Ошибка", "Здание не найдено");
        return;
    };
    if let Err(err) = update_pack_with_world_sync(state, pack.x, pack.y, apply) {
        send_ok(tx, "Ошибка", &format!("{error_prefix}: {err}"));
        return;
    }
    send_ok(tx, "Пак", success_message);
}

fn is_admin_command(state: &Arc<GameState>, pid: PlayerId) -> bool {
    state
        .active_players
        .get(&pid)
        .is_some_and(|p| p.data.as_role().is_admin())
}

pub fn handle_whoi(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, ids: &[i32]) {
    // NickList: "id1:name1,id2:name2,..."
    let parts: Vec<String> = ids
        .iter()
        .map(|&id| {
            let name = state
                .active_players
                .get(&id)
                .map(|p| p.data.name.clone())
                .or_else(|| state.db.get_player_by_id(id).ok().flatten().map(|p| p.name))
                .unwrap_or_default();
            format!("{id}:{name}")
        })
        .collect();
    let msg = parts.join(",");
    send_u_packet(tx, "NL", msg.as_bytes());
}

pub fn handle_death(state: &Arc<GameState>, tx: &mpsc::UnboundedSender<Vec<u8>>, pid: PlayerId) {
    // Drop crystals as a box at death position
    let (death_x, death_y, had_crystals) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        let total: i64 = p.data.crystals.iter().sum();
        (p.data.x, p.data.y, total > 0)
    };

    if had_crystals {
        let dropped_crystals = {
            state.active_players.get_mut(&pid).map(|mut p| {
                let stored = p.data.crystals;
                p.data.crystals = [0; 6];
                stored
            })
        };
        if let Some(_crys) = dropped_crystals {
            if let Some((box_x, box_y)) = spawn_crystal_box(state, death_x, death_y) {
                tracing::debug!("Player {pid} dropped crystals into box at ({box_x},{box_y})");
                broadcast_cell_update(state, box_x, box_y);
            }
        }
        // Send death FX
        let fx = hb_fx(net_u16_nonneg(death_x), net_u16_nonneg(death_y), 2);
        let fx_data = encode_hb_bundle(&hb_bundle(&[fx]).1);
        let (cx, cy) = World::chunk_pos(death_x, death_y);
        state.broadcast_to_nearby(cx, cy, &fx_data, None);
    }

    // Respawn: use resp if set, otherwise default spawn
    let (resp_x, resp_y) = {
        let Some(p) = state.active_players.get(&pid) else {
            return;
        };
        if let (Some(rx), Some(ry)) = (p.data.resp_x, p.data.resp_y) {
            // Check if resp building still exists
            if let Some(resp_pack) = state.get_pack_at(rx, ry) {
                if resp_pack.pack_type == PackType::Resp && resp_pack.charge > 0.0 {
                    // Random point in spawn area (x+2..x+5, y-1..y+2)
                    use rand::Rng;
                    let mut rng = rand::rng();
                    (
                        rng.random_range(rx + 2..rx + 6),
                        rng.random_range(ry - 1..ry + 3),
                    )
                } else {
                    (10, 10)
                }
            } else {
                (10, 10)
            }
        } else {
            (10, 10)
        }
    };
    if let Some(mut p) = state.active_players.get_mut(&pid) {
        p.data.x = resp_x;
        p.data.y = resp_y;
        p.data.health = p.data.max_health;
        p.current_window = None;
        p.last_chunk = None;
        p.visible_chunks.clear();
    }
    send_u_packet(tx, "@T", &tp(resp_x, resp_y).1);
    if let Some(p) = state.active_players.get(&pid) {
        send_u_packet(tx, "@L", &health(p.data.health, p.data.max_health).1);
        send_u_packet(tx, "@B", &basket(&p.data.crystals, 1000).1);
    }
    check_chunk_changed(state, tx, pid);
}

fn send_channel_packet_to_players(
    state: &Arc<GameState>,
    packet_data: &[u8],
    clan_id: Option<i32>,
) {
    let packet = make_u_packet_bytes("mU", packet_data);
    for entry in &state.active_players {
        if clan_id.is_none_or(|id| entry.data.clan_id == Some(id)) {
            let _ = entry.tx.send(packet.clone());
        }
    }
}
