use crate::db::players::Role;
use crate::game::GameState;
use crate::game::player::{PlayerId, PlayerMetadata, PlayerStats};
use crate::net::session::wire::make_u_packet_bytes;
use crate::world::WorldProvider;
use std::sync::Arc;
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;

// ─── Arg-parsing helpers ─────────────────────────────────────────────────────

fn flag_value<'a>(args: &[&'a str], long: &str, short: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|w| w[0] == long || (!short.is_empty() && w[0] == short))
        .map(|w| w[1])
}

fn parse_flag<T: std::str::FromStr>(args: &[&str], long: &str, short: &str) -> Option<T> {
    flag_value(args, long, short)?.parse().ok()
}

// ─── REPL ────────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
pub async fn run_repl(state: Arc<GameState>, shutdown_tx: broadcast::Sender<()>) -> io::Result<()> {
    let mut reader = BufReader::new(io::stdin()).lines();

    println!(">>> Interactive admin console ready. Type 'help' or '?' for commands.");

    while let Some(line) = reader.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        let cmd = parts[0];
        let args = &parts[1..];

        tracing::info!(
            target: "console",
            command = cmd,
            arguments = ?args,
            "Console operator command executed"
        );

        match cmd {
            "help" | "?" => {
                println!("Available commands:");
                println!(
                    "  stop | shutdown                              Stop the server gracefully"
                );
                println!("  online                                       Show online player list");
                println!(
                    "  find <name>                                  Find player by name (online + DB)"
                );
                println!(
                    "  announce <message>                           Broadcast status notification"
                );
                println!("  give   -p <ID> -i <ID> [-a <N>]             Give item to player");
                println!("  money  -p <ID> -a <N>                        Add money to player");
                println!("  tp     -p <ID> -x <X> -y <Y>                Teleport player");
                println!("  heal   -p <ID>                               Heal player to max HP");
                println!("  kill   -p <ID>                               Kill player instantly");
                println!("  kick   -p <ID>                               Force-disconnect player");
                println!("  role   -p <ID> -r admin|mod|player           Set player role");
                println!(
                    "  info   -p <ID>                               Show detailed player info"
                );
                println!(
                    "  save                                         Save all players and flush world"
                );
                println!(
                    "  schedule <name> <ms>                         Set ECS schedule interval (0 disables)"
                );
            }
            "stop" | "shutdown" => {
                tracing::info!(target: "console", "Graceful shutdown triggered from console.");
                println!("Graceful shutdown triggered from console.");
                let _ = shutdown_tx.send(());
                break;
            }
            "online" => {
                let online: Vec<_> = state
                    .active_players
                    .iter()
                    .map(|entry| {
                        let pid = *entry.key();
                        state
                            .query_player(pid, |ecs, entity| {
                                let name = ecs
                                    .get::<PlayerMetadata>(entity)
                                    .map_or_else(|| "?".to_string(), |m| m.name.clone());
                                let role = ecs
                                    .get::<PlayerStats>(entity)
                                    .map_or("?", |s| role_str(s.role));
                                format!("ID:{pid} [{role}] {name}")
                            })
                            .unwrap_or_else(|| format!("ID:{pid} [?] Unknown"))
                    })
                    .collect();
                if online.is_empty() {
                    println!("No players online.");
                } else {
                    println!("Online ({}):", online.len());
                    for p in online {
                        println!("  {p}");
                    }
                }
            }
            "find" => {
                if args.is_empty() {
                    println!("Usage: find <name>");
                    continue;
                }
                let name = args[0];
                match state.db.get_player_by_name(name).await {
                    Ok(Some(row)) => {
                        let pid: PlayerId = row.id.into();
                        let online = state.active_players.contains_key(&pid);
                        if online {
                            let detail = state.query_player_opt(pid, |ecs, entity| {
                                let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                                let s = ecs.get::<PlayerStats>(entity)?;
                                Some(format!(
                                    "ONLINE | Pos:({},{}) | HP:{}/{} | ${}",
                                    pos.x, pos.y, s.health, s.max_health, s.money
                                ))
                            });
                            println!(
                                "ID:{} [{}] {} — {}",
                                pid,
                                role_str(row.role),
                                row.name,
                                detail.unwrap_or_else(|| "ONLINE".to_string())
                            );
                        } else {
                            println!(
                                "ID:{} [{}] {} — OFFLINE | Last:({},{})",
                                pid,
                                role_str(row.role),
                                row.name,
                                row.x,
                                row.y
                            );
                        }
                    }
                    Ok(None) => println!("Player '{name}' not found."),
                    Err(e) => println!("DB error: {e}"),
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
                let mut count = 0;
                for entry in &state.active_players {
                    state.query_player(*entry.key(), |ecs, entity| {
                        if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity)
                        {
                            let _ = conn.tx.send(pkt.clone());
                        }
                    });
                    count += 1;
                }
                println!("Announced to {count} players.");
            }
            "give" => {
                let pid = parse_flag::<PlayerId>(args, "--player", "-p");
                let iid = parse_flag::<i32>(args, "--item", "-i");
                let amount = parse_flag::<i32>(args, "--amount", "-a").unwrap_or(1);
                let (Some(pid), Some(iid)) = (pid, iid) else {
                    println!("Usage: give -p <ID> -i <ID> [-a <N>]");
                    continue;
                };
                let res = state.modify_player(pid, |ecs, entity| {
                    {
                        let mut inv =
                            ecs.get_mut::<crate::game::player::PlayerInventory>(entity)?;
                        *inv.items.entry(iid).or_insert(0) += amount;
                    }
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    let conn_tx = ecs
                        .get::<crate::game::player::PlayerConnection>(entity)
                        .map(|c| c.tx.clone());
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
                    tracing::info!(target: "console", player_id = %pid, item_id = iid, amount, "Gave item to player");
                    println!("Gave item {iid} x{amount} to player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for give");
                    println!("Player {pid} not found/offline.");
                }
            }
            "money" => {
                let pid = parse_flag::<PlayerId>(args, "--player", "-p");
                let amount = parse_flag::<i64>(args, "--amount", "-a");
                let (Some(pid), Some(amount)) = (pid, amount) else {
                    println!("Usage: money -p <ID> -a <N>");
                    continue;
                };
                let res = state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                    s.money = s.money.saturating_add(amount);
                    let (m, c) = (s.money, s.creds);
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let pkt = crate::protocol::packets::money(m, c);
                        let _ = conn.tx.send(make_u_packet_bytes(pkt.0, &pkt.1));
                    }
                    Some(())
                });
                if res.flatten().is_some() {
                    tracing::info!(target: "console", player_id = %pid, amount, "Added money to player");
                    println!("Added ${amount} to player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for money");
                    println!("Player {pid} not found/offline.");
                }
            }
            "tp" => {
                let pid = parse_flag::<PlayerId>(args, "--player", "-p");
                let x = parse_flag::<i32>(args, "--x", "-x");
                let y = parse_flag::<i32>(args, "--y", "-y");
                let (Some(pid), Some(x), Some(y)) = (pid, x, y) else {
                    println!("Usage: tp -p <ID> -x <X> -y <Y>");
                    continue;
                };
                if !state.world.valid_coord(x, y) {
                    println!("Coordinates ({x},{y}) are out of bounds.");
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
                        let _ = conn.tx.send(make_u_packet_bytes(pkt.0, &pkt.1));
                        crate::net::session::play::chunks::check_chunk_changed(
                            &state, &conn.tx, pid,
                        );
                    }
                    Some(())
                });
                if res.flatten().is_some() {
                    tracing::info!(target: "console", player_id = %pid, x, y, "Teleported player");
                    println!("Teleported player {pid} to ({x},{y}).");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for tp");
                    println!("Player {pid} not found/offline.");
                }
            }
            "heal" => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: heal -p <ID>");
                    continue;
                };
                let res = state.modify_player(pid, |ecs, entity| {
                    let mut s = ecs.get_mut::<PlayerStats>(entity)?;
                    s.health = s.max_health;
                    let (h, mh) = (s.health, s.max_health);
                    if let Some(mut f) = ecs.get_mut::<crate::game::player::PlayerFlags>(entity) {
                        f.dirty = true;
                    }
                    if let Some(conn) = ecs.get::<crate::game::player::PlayerConnection>(entity) {
                        let pkt = crate::protocol::packets::health(h, mh);
                        let _ = conn.tx.send(make_u_packet_bytes(pkt.0, &pkt.1));
                    }
                    Some(())
                });
                if res.flatten().is_some() {
                    tracing::info!(target: "console", player_id = %pid, "Healed player");
                    println!("Healed player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for heal");
                    println!("Player {pid} not found/offline.");
                }
            }
            "kill" => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: kill -p <ID>");
                    continue;
                };
                let conn_tx = state.query_player_opt(pid, |ecs, entity| {
                    ecs.get::<crate::game::player::PlayerConnection>(entity)
                        .map(|c| c.tx.clone())
                });
                if let Some(tx) = conn_tx {
                    crate::net::session::play::death::handle_death(&state, &tx, pid);
                    tracing::info!(target: "console", player_id = %pid, "Killed player");
                    println!("Killed player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for kill");
                    println!("Player {pid} not found/offline.");
                }
            }
            "kick" => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: kick -p <ID>");
                    continue;
                };
                if state.kick_channels.remove(&pid).is_some() {
                    tracing::info!(target: "console", player_id = %pid, "Kicked player");
                    println!("Kicked player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for kick");
                    println!("Player {pid} not found/offline.");
                }
            }
            "role" => {
                let pid = parse_flag::<i32>(args, "--player", "-p");
                let role_arg = flag_value(args, "--role", "-r");
                let (Some(pid), Some(role_arg)) = (pid, role_arg) else {
                    println!("Usage: role -p <ID> -r admin|mod|player");
                    continue;
                };
                let role = match role_arg {
                    "admin" => Role::Admin,
                    "mod" | "moderator" => Role::Moderator,
                    "player" => Role::Player,
                    _ => {
                        println!("Invalid role '{role_arg}'. Use: admin|mod|player");
                        continue;
                    }
                };
                match state.db.set_player_role(pid, role).await {
                    Ok(true) => {
                        state.modify_player(pid.into(), |ecs, entity| {
                            if let Some(mut s) = ecs.get_mut::<PlayerStats>(entity) {
                                s.role = role as i32;
                            }
                            Some(())
                        });
                        tracing::info!(target: "console", player_id = %pid, role = role_arg, "Set player role");
                        println!("Set role '{role_arg}' for player {pid}.");
                    }
                    Ok(false) => {
                        tracing::warn!(target: "console", player_id = %pid, "Player not found in DB for role");
                        println!("Player {pid} not found in DB.");
                    }
                    Err(e) => {
                        tracing::error!(target: "console", player_id = %pid, error = %e, "DB error for role");
                        println!("DB error: {e}");
                    }
                }
            }
            "info" => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: info -p <ID>");
                    continue;
                };
                let details = state.query_player_opt(pid, |ecs, entity| {
                    let pos = ecs.get::<crate::game::player::PlayerPosition>(entity)?;
                    let s = ecs.get::<PlayerStats>(entity)?;
                    let meta = ecs.get::<PlayerMetadata>(entity)?;
                    let skills = ecs.get::<crate::game::player::PlayerSkillsComp>(entity)?;
                    let prog = ecs.get::<crate::game::programmator::ProgrammatorState>(entity)?;
                    Some((
                        meta.name.clone(),
                        s.role,
                        skills.states.lvl_summary(),
                        pos.x,
                        pos.y,
                        s.health,
                        s.max_health,
                        s.money,
                        s.creds,
                        s.crystals,
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
                    println!("Player {pid}:");
                    println!("  Nickname:  {name}");
                    println!("  Role:      {}", role_str(role));
                    println!("  Level:     {level}");
                    println!("  Position:  ({x},{y})");
                    println!("  Health:    {hp}/{max_hp}");
                    println!("  Money:     ${money} | Credits: {creds}");
                    println!(
                        "  Crystals:  G:{} B:{} R:{} V:{} W:{} C:{}",
                        crystals[0],
                        crystals[1],
                        crystals[2],
                        crystals[3],
                        crystals[4],
                        crystals[5]
                    );
                    println!("  Program:   running={prog_running} fn={prog_func}");
                } else {
                    println!("Player {pid} not found/offline.");
                }
            }
            "save" => {
                tracing::info!(target: "console", "Manual save triggered from console");
                println!("Saving all active players and flushing world...");
                let pids: Vec<_> = state.active_players.iter().map(|e| *e.key()).collect();
                let mut saved = 0;
                for pid in pids {
                    let row = state.query_player_opt(pid, |ecs, entity| {
                        crate::game::player::extract_player_row(ecs, entity)
                    });
                    if let Some(row) = row {
                        match state.db.save_player(&row).await {
                            Ok(()) => saved += 1,
                            Err(e) => {
                                tracing::error!(target: "console", player_id = %pid, error = %e, "Error saving player");
                                println!("Error saving player {pid}: {e}");
                            }
                        }
                    }
                }
                match state.world.flush() {
                    Ok(()) => {
                        tracing::info!(target: "console", saved_count = saved, "Manual save complete");
                        println!("Saved {saved} players and flushed world.");
                    }
                    Err(e) => {
                        tracing::error!(target: "console", error = %e, "Error flushing world during manual save");
                        println!("Error flushing world: {e}");
                    }
                }
            }
            "schedule" => {
                let [name, interval_ms] = args else {
                    println!("Usage: schedule <name> <ms>");
                    continue;
                };
                let Ok(interval_ms) = interval_ms.parse::<u64>() else {
                    println!("Invalid interval '{interval_ms}'. Use milliseconds.");
                    continue;
                };
                if state.set_schedule_interval(name, interval_ms) {
                    tracing::info!(
                        target: "console",
                        schedule = %name,
                        interval_ms,
                        "Updated ECS schedule interval"
                    );
                    if interval_ms == 0 {
                        println!("Schedule '{name}' disabled.");
                    } else {
                        println!("Schedule '{name}' interval set to {interval_ms} ms.");
                    }
                } else {
                    println!("Unknown schedule '{name}'.");
                }
            }
            unknown => {
                println!("Unknown command: '{unknown}'. Type 'help' or '?' for commands.");
            }
        }
    }

    Ok(())
}

const fn role_str(role: i32) -> &'static str {
    match role {
        2 => "Admin",
        1 => "Mod",
        _ => "Player",
    }
}
