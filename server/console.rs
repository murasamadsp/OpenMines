use crate::game::GameState;
use crate::game::player::PlayerId;
use crate::net::session::wire::make_u_packet_bytes;
use crate::world::WorldProvider;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;

#[allow(clippy::too_many_lines)]
pub async fn run_repl(state: Arc<GameState>, shutdown_tx: broadcast::Sender<()>) -> io::Result<()> {
    let mut reader = BufReader::new(io::stdin()).lines();

    // Print welcome message
    println!(">>> Interactive admin console ready. Type 'help' or '?' for commands.");

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd = parts[0];
        let args = &parts[1..];

        match cmd {
            "help" | "?" => {
                println!("Available commands:");
                println!("  stop | shutdown                        Stop the server gracefully");
                println!("  online                                 Show online player list");
                println!(
                    "  announce <message>                     Announce status notification to all players"
                );
                println!("  give --player <ID> --item <ID> [amount] Give an item to a player");
                println!("  money --player <ID> --amount <N>       Add money to a player");
                println!("  tp --player <ID> --x <X> --y <Y>       Teleport a player");
                println!("  heal --player <ID>                     Heal player health to maximum");
                println!(
                    "  save                                   Save all players and flush the world"
                );
                println!("  kill --player <ID>                     Kill a player instantly");
                println!(
                    "  info --player <ID>                     Show detailed player information"
                );
            }
            "stop" | "shutdown" => {
                println!("Graceful shutdown triggered from console.");
                let _ = shutdown_tx.send(());
                break;
            }
            "online" => {
                let online_players = state
                    .active_players
                    .iter()
                    .map(|entry| {
                        let pid = *entry.key();
                        let name = state
                            .query_player_opt(pid, |ecs, entity| {
                                ecs.get::<crate::game::player::PlayerMetadata>(entity)
                                    .map(|m| m.name.clone())
                            })
                            .unwrap_or_else(|| "Unknown".to_string());
                        format!("ID: {pid} | Nick: {name}")
                    })
                    .collect::<Vec<_>>();

                if online_players.is_empty() {
                    println!("No players online.");
                } else {
                    println!("Online players ({}):", online_players.len());
                    for p in online_players {
                        println!("  {p}");
                    }
                }
            }
            "announce" => {
                if args.is_empty() {
                    println!("Usage: announce <message>");
                    continue;
                }
                let msg = args.join(" ");
                let (event, pkt_body) = crate::protocol::packets::status(&msg);
                let pkt = make_u_packet_bytes(event, &pkt_body);
                let mut sent_count = 0;
                for entry in &state.active_players {
                    state.query_player(*entry.key(), |ecs, entity| {
                        if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity)
                        {
                            let _ = conn.tx.send(pkt.clone());
                        }
                    });
                    sent_count += 1;
                }
                println!("Announced status message to {sent_count} players.");
            }
            "give" => {
                let mut player_id: Option<PlayerId> = None;
                let mut item_id: Option<i32> = None;
                let mut amount = 1;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        "--item" | "-i" => {
                            if i + 1 < args.len() {
                                item_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        other => {
                            if let Ok(amt) = other.parse() {
                                amount = amt;
                                i += 1;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                    }
                }
                if !ok || player_id.is_none() || item_id.is_none() {
                    println!("Usage: give --player <ID> --item <ID> [amount]");
                    continue;
                }
                let pid = player_id.unwrap();
                let iid = item_id.unwrap();

                let res = state.modify_player(pid, |ecs, entity| {
                    {
                        let mut inv =
                            ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                        *inv.items.entry(iid).or_insert(0) += amount;
                    }
                    if let Some(mut flags) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity)
                    {
                        flags.dirty = true;
                    }
                    let conn_tx = ecs
                        .get::<crate::game::player::PlayerConnection>(entity)
                        .map(|conn| conn.tx.clone());
                    if let Some(tx) = conn_tx {
                        let mut inv =
                            ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                        crate::net::session::outbound::inventory_sync::send_inventory(
                            &tx, &mut inv,
                        );
                    }
                    Some(())
                });

                if res.flatten().is_some() {
                    println!("Gave item {iid} (x{amount}) to player {pid}.");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "money" => {
                let mut player_id: Option<PlayerId> = None;
                let mut amount: Option<i64> = None;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        "--amount" | "-a" => {
                            if i + 1 < args.len() {
                                amount = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || player_id.is_none() || amount.is_none() {
                    println!("Usage: money --player <ID> --amount <N>");
                    continue;
                }
                let pid = player_id.unwrap();
                let amt = amount.unwrap();

                let res = state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                    s.money = s.money.saturating_add(amt);
                    let (m, c) = (s.money, s.creds);
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let pkt = crate::protocol::packets::money(m, c);
                        let bytes = make_u_packet_bytes(pkt.0, &pkt.1);
                        let _ = conn.tx.send(bytes);
                    }
                    Some(())
                });

                if res.flatten().is_some() {
                    println!("Added $ {amt} to player {pid}.");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "tp" => {
                let mut player_id: Option<PlayerId> = None;
                let mut target_x: Option<i32> = None;
                let mut target_y: Option<i32> = None;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        "--x" | "-x" => {
                            if i + 1 < args.len() {
                                target_x = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        "--y" | "-y" => {
                            if i + 1 < args.len() {
                                target_y = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || player_id.is_none() || target_x.is_none() || target_y.is_none() {
                    println!("Usage: tp --player <ID> --x <X> --y <Y>");
                    continue;
                }
                let pid = player_id.unwrap();
                let x = target_x.unwrap();
                let y = target_y.unwrap();

                if !state.world.valid_coord(x, y) {
                    println!("Error: Coordinates ({x}, {y}) are out of bounds.");
                    continue;
                }

                let res = state.modify_player(pid, |ecs, entity| {
                    let mut pos = ecs.get_mut::<crate::game::player::PlayerPosition>(entity)?;
                    pos.x = x;
                    pos.y = y;
                    if let Some(mut ui) = ecs.get_mut::<crate::game::player::PlayerUI>(entity) {
                        ui.current_window = None;
                    }
                    if let Some(mut view) = ecs.get_mut::<crate::game::player::PlayerView>(entity) {
                        view.last_chunk = None;
                        view.visible_chunks.clear();
                    }
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let pkt = crate::protocol::packets::tp(x, y);
                        let bytes = make_u_packet_bytes(pkt.0, &pkt.1);
                        let _ = conn.tx.send(bytes);
                        crate::net::session::play::chunks::check_chunk_changed(
                            &state, &conn.tx, pid,
                        );
                    }
                    Some(())
                });

                if res.flatten().is_some() {
                    println!("Teleported player {pid} to ({x}, {y}).");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "heal" => {
                let mut player_id: Option<PlayerId> = None;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || player_id.is_none() {
                    println!("Usage: heal --player <ID>");
                    continue;
                }
                let pid = player_id.unwrap();

                let res = state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<crate::game::player::PlayerStats>(entity)?;
                    s.health = s.max_health;
                    let (h, mh) = (s.health, s.max_health);
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let pkt = crate::protocol::packets::health(h, mh);
                        let bytes = make_u_packet_bytes(pkt.0, &pkt.1);
                        let _ = conn.tx.send(bytes);
                    }
                    Some(())
                });

                if res.flatten().is_some() {
                    println!("Healed player {pid}.");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "save" => {
                println!("Saving all active players and flushing world...");
                let pids: Vec<_> = state.active_players.iter().map(|e| *e.key()).collect();
                let mut saved_count = 0;
                for pid in pids {
                    let player_row = state.query_player_opt(pid, |ecs, entity| {
                        crate::game::player::extract_player_row(ecs, entity)
                    });
                    if let Some(row) = player_row {
                        match state.db.save_player(&row).await {
                            Ok(()) => saved_count += 1,
                            Err(e) => println!("Error saving player {pid}: {e}"),
                        }
                    }
                }
                match state.world.flush() {
                    Ok(()) => {
                        println!("Saved {saved_count} players and flushed world successfully.");
                    }
                    Err(e) => println!("Error flushing world: {e}"),
                }
            }
            "kill" => {
                let mut player_id: Option<PlayerId> = None;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || player_id.is_none() {
                    println!("Usage: kill --player <ID>");
                    continue;
                }
                let pid = player_id.unwrap();

                let conn_tx = state.query_player_opt(pid, |ecs, entity| {
                    ecs.get::<crate::game::player::PlayerConnection>(entity)
                        .map(|conn| conn.tx.clone())
                });

                if let Some(tx) = conn_tx {
                    crate::net::session::play::death::handle_death(&state, &tx, pid);
                    println!("Killed player {pid}.");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "info" => {
                let mut player_id: Option<PlayerId> = None;

                let mut i = 0;
                let mut ok = true;
                while i < args.len() {
                    match args[i] {
                        "--player" | "-p" => {
                            if i + 1 < args.len() {
                                player_id = args[i + 1].parse().ok();
                                i += 2;
                            } else {
                                ok = false;
                                break;
                            }
                        }
                        _ => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok || player_id.is_none() {
                    println!("Usage: info --player <ID>");
                    continue;
                }
                let pid = player_id.unwrap();

                let details = state.query_player_opt(pid, |ecs, entity| {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let p_stats = ecs.get::<crate::game::player::PlayerStats>(entity)?;
                    let meta = ecs.get::<crate::game::player::PlayerMetadata>(entity)?;
                    let skills = ecs.get::<crate::game::player::PlayerSkills>(entity)?;
                    let prog = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;

                    Some((
                        meta.name.clone(),
                        p_stats.role,
                        skills.states.lvl_summary(),
                        pos.x,
                        pos.y,
                        p_stats.health,
                        p_stats.max_health,
                        p_stats.money,
                        p_stats.creds,
                        p_stats.crystals,
                        prog.running,
                        prog.current_function.clone(),
                    ))
                });

                if let Some((
                    name,
                    role,
                    level,
                    x,
                    y,
                    hp,
                    max_hp,
                    money,
                    creds,
                    crystals,
                    prog_running,
                    prog_func,
                )) = details
                {
                    let role_str = match role {
                        2 => "Admin",
                        1 => "Moderator",
                        _ => "Player",
                    };
                    println!("Player Info for ID {pid}:");
                    println!("  Nickname:  {name}");
                    println!("  Role:      {role_str}");
                    println!("  Level:     {level}");
                    println!("  Position:  ({x}, {y})");
                    println!("  Health:    {hp}/{max_hp}");
                    println!("  Money:     {money} | Credits: {creds}");
                    println!(
                        "  Crystals:  Green:{}, Blue:{}, Red:{}, Violet:{}, White:{}, Cyan:{}",
                        crystals[0],
                        crystals[1],
                        crystals[2],
                        crystals[3],
                        crystals[4],
                        crystals[5]
                    );
                    println!("  Program:   Running: {prog_running} | Current Fn: {prog_func}");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            unknown => {
                println!(
                    "Unknown command: '{unknown}'. Type 'help' or '?' for available commands."
                );
            }
        }
    }

    Ok(())
}
