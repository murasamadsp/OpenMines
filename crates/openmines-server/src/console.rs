use crate::db::players::Role;
use crate::game::GameState;
use crate::game::player::{PlayerId, PlayerMetadata, PlayerStats};
use crate::net::session::wire::make_u_packet_bytes;
use crate::world::WorldProvider;
use std::io::{self, BufRead as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, mpsc};

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

const CONSOLE_INPUT_CAPACITY: usize = 16;

struct StdinReaderStop(Arc<AtomicBool>);

impl Drop for StdinReaderStop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

#[cfg(unix)]
fn stdin_ready() -> io::Result<bool> {
    use std::os::fd::AsRawFd as _;

    let mut descriptor = libc::pollfd {
        fd: io::stdin().as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `descriptor` points to one initialized pollfd for the duration of the call.
    let result = unsafe { libc::poll(&raw mut descriptor, 1, 100) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(result > 0)
}

#[cfg(not(unix))]
fn stdin_ready() -> io::Result<bool> {
    Ok(true)
}

fn spawn_stdin_reader() -> io::Result<(mpsc::Receiver<io::Result<String>>, StdinReaderStop)> {
    let (tx, rx) = mpsc::channel(CONSOLE_INPUT_CAPACITY);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    std::thread::Builder::new()
        .name("admin-console-stdin".to_owned())
        .spawn(move || {
            let stdin = io::stdin();
            while !thread_stop.load(Ordering::Acquire) {
                match stdin_ready() {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(error) => {
                        let _ = tx.blocking_send(Err(error));
                        break;
                    }
                }
                let mut line = String::new();
                let result = stdin.lock().read_line(&mut line).map(|_| line);
                let eof = matches!(&result, Ok(line) if line.is_empty());
                let failed = result.is_err();
                if tx.blocking_send(result).is_err() || eof || failed {
                    break;
                }
            }
        })?;
    Ok((rx, StdinReaderStop(stop)))
}

#[allow(clippy::too_many_lines)]
pub async fn run_repl(state: Arc<GameState>, shutdown_tx: broadcast::Sender<()>) -> io::Result<()> {
    let (mut lines, _stdin_reader_stop) = spawn_stdin_reader()?;
    let mut shutdown_rx = shutdown_tx.subscribe();

    println!(">>> Interactive admin console ready. Type 'help' or '?' for commands.");

    loop {
        let line = tokio::select! {
            _ = shutdown_rx.recv() => break,
            line = lines.recv() => line,
        };
        let Some(line) = line else {
            break;
        };
        let line = line?;
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

        match crate::admin::AdminCommandName::from_console(cmd) {
            Some(crate::admin::AdminCommandName::Help) => {
                println!("{}", crate::admin::console_help());
            }
            Some(crate::admin::AdminCommandName::Shutdown) => {
                tracing::info!(target: "console", "Graceful shutdown triggered from console.");
                println!("Graceful shutdown triggered from console.");
                let _ = shutdown_tx.send(());
                break;
            }
            Some(crate::admin::AdminCommandName::Online) => {
                let online: Vec<_> = state
                    .active_player_ids()
                    .into_iter()
                    .map(|pid| {
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
            Some(crate::admin::AdminCommandName::Find) => {
                if args.is_empty() {
                    println!("Usage: find <name>");
                    continue;
                }
                let name = args[0];
                match state.db.get_player_by_name(name).await {
                    Ok(Some(row)) => {
                        let pid: PlayerId = row.id.into();
                        let online = state.is_player_active(pid);
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
            Some(crate::admin::AdminCommandName::Announce) => {
                if args.is_empty() {
                    println!("Usage: announce <message>");
                    continue;
                }
                let msg = args.join(" ");
                let (event, pkt_body) = crate::protocol::packets::status(&msg);
                let pkt = make_u_packet_bytes(event, &pkt_body);
                let mut count = 0;
                for pid in state.active_player_ids() {
                    state.send_to_player(pid, pkt.clone());
                    count += 1;
                }
                println!("Announced to {count} players.");
            }
            Some(crate::admin::AdminCommandName::Give) => {
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
                    if let Some(tx) = state.player_sender(pid) {
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
            Some(crate::admin::AdminCommandName::Money) => {
                let pid = parse_flag::<PlayerId>(args, "--player", "-p");
                let amount = parse_flag::<i64>(args, "--amount", "-a");
                let (Some(pid), Some(amount)) = (pid, amount) else {
                    println!("Usage: money -p <ID> -a <N>");
                    continue;
                };
                match crate::admin::add_player_money(&state, pid, amount) {
                    Ok(()) => {
                        tracing::info!(target: "console", player_id = %pid, amount, "Added money to player");
                        println!("Added ${amount} to player {pid}.");
                    }
                    Err(crate::admin::AdminCommandError::PlayerUnavailable) => {
                        tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for money");
                        println!("Player {pid} not found/offline.");
                    }
                    Err(crate::admin::AdminCommandError::MissingPlayerState(component)) => {
                        tracing::warn!(target: "console", player_id = %pid, component, "Player state missing for money");
                        println!("Player {pid} state missing: {component}.");
                    }
                }
            }
            Some(crate::admin::AdminCommandName::Teleport) => {
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
                    if let Some(tx) = state.player_sender(pid) {
                        let pkt = crate::protocol::packets::tp(x, y);
                        let _ = tx.send(make_u_packet_bytes(pkt.0, &pkt.1));
                        crate::net::session::play::chunks::check_chunk_changed(&state, &tx, pid);
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
            Some(crate::admin::AdminCommandName::Heal) => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: heal -p <ID>");
                    continue;
                };
                match crate::admin::heal_player(&state, pid) {
                    Ok(()) => {
                        tracing::info!(target: "console", player_id = %pid, "Healed player");
                        println!("Healed player {pid}.");
                    }
                    Err(crate::admin::AdminCommandError::PlayerUnavailable) => {
                        tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for heal");
                        println!("Player {pid} not found/offline.");
                    }
                    Err(crate::admin::AdminCommandError::MissingPlayerState(component)) => {
                        tracing::warn!(target: "console", player_id = %pid, component, "Player state missing for heal");
                        println!("Player {pid} state missing: {component}.");
                    }
                }
            }
            Some(crate::admin::AdminCommandName::Kill) => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: kill -p <ID>");
                    continue;
                };
                let conn_tx = state.player_sender(pid);
                if conn_tx.is_some() {
                    crate::net::session::play::death::request_death(&state, pid);
                    tracing::info!(target: "console", player_id = %pid, "Killed player");
                    println!("Killed player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for kill");
                    println!("Player {pid} not found/offline.");
                }
            }
            Some(crate::admin::AdminCommandName::Kick) => {
                let Some(pid) = parse_flag::<PlayerId>(args, "--player", "-p") else {
                    println!("Usage: kick -p <ID>");
                    continue;
                };
                if state.kick_player(pid) {
                    tracing::info!(target: "console", player_id = %pid, "Kicked player");
                    println!("Kicked player {pid}.");
                } else {
                    tracing::warn!(target: "console", player_id = %pid, "Player not found/offline for kick");
                    println!("Player {pid} not found/offline.");
                }
            }
            Some(crate::admin::AdminCommandName::Role) => {
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
            Some(crate::admin::AdminCommandName::Info) => {
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
            Some(crate::admin::AdminCommandName::Save) => {
                tracing::info!(target: "console", "Manual save triggered from console");
                println!("Saving all active players and flushing world...");
                let pids: Vec<_> = state.active_player_ids();
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
                    Ok(_) => {
                        tracing::info!(target: "console", saved_count = saved, "Manual save complete");
                        println!("Saved {saved} players and flushed world.");
                    }
                    Err(e) => {
                        tracing::error!(target: "console", error = %e, "Error flushing world during manual save");
                        println!("Error flushing world: {e}");
                    }
                }
            }
            Some(crate::admin::AdminCommandName::Schedule) => {
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
            None => {
                println!("Unknown command: '{cmd}'. Type 'help' or '?' for commands.");
            }
            Some(_) => unreachable!("console parser returned a slash-only admin command"),
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
