//! TCP connection: handshake, read loop, write loop.

use super::prelude::*;
use crate::net::session::auth::gui_flow::handle_auth_gui;
use crate::net::session::auth::login::{AuthState, handle_auth, log_and_show_auth_ui};
use crate::net::session::dispatch::handle_ty;
use crate::net::session::player::init::on_disconnect;
use tokio::sync::broadcast;
use tracing::Instrument;

const TY_SUBPAYLOAD_PREVIEW_LEN: usize = 80;

pub async fn handle(
    stream: TcpStream,
    addr: SocketAddr,
    state: Arc<GameState>,
    shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    crate::metrics::TCP_CONNECTIONS_TOTAL.inc();
    crate::metrics::TCP_CONNECTIONS_CURRENT.inc();
    let (reader, writer) = tokio::io::split(stream);
    let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();

    let sid = GameState::generate_session_id();

    // Send initial handshake: ST, AU(session_id), PI
    send_u_packet(&tx, "ST", &status("OpenMines").1);
    send_u_packet(&tx, "AU", &au_session(&sid).1);
    send_u_packet(&tx, "PI", &ping(0, 0, "").1);

    // Writer task: drains rx channel → TCP
    let write_handle = tokio::spawn(write_loop(writer, rx, shutdown.resubscribe()));

    // Reader task: TCP → parse packets → dispatch
    let span = tracing::info_span!("session", peer.addr = %addr, session.id = %sid);
    let result = read_loop(reader, tx.clone(), &sid, addr, state.clone(), shutdown)
        .instrument(span)
        .await;
    if let Err(err) = &result {
        tracing::warn!("Session {addr} ended with error: {err}");
    }

    // Cleanup
    write_handle.abort();
    tracing::info!("Session {addr} disconnected");
    crate::metrics::TCP_CONNECTIONS_CURRENT.dec();
    let _: () = result.unwrap_or(());
    Ok(())
}

async fn write_loop(
    mut writer: tokio::io::WriteHalf<TcpStream>,
    mut rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut shutdown: broadcast::Receiver<()>,
) {
    loop {
        let data = tokio::select! {
            v = rx.recv() => v,
            _ = shutdown.recv() => break,
        };
        let Some(data) = data else { break };
        if let Err(err) = writer.write_all(&data).await {
            tracing::warn!("Socket write failed for {}", describe_wire_packet(&data));
            tracing::warn!("Socket write error: {err}");
            break;
        }
    }
}

async fn read_loop(
    mut reader: tokio::io::ReadHalf<TcpStream>,
    tx: mpsc::UnboundedSender<Vec<u8>>,
    sid: &str,
    addr: SocketAddr,
    state: Arc<GameState>,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let mut buf = BytesMut::with_capacity(8192);
    let mut player_id: Option<PlayerId> = None;
    let mut auth_state = AuthState::WaitingAu;
    let client_ip = addr.ip();
    let mut auth_failure_count: u32 = 0;
    let mut auth_failure_window = Instant::now();
    let mut auth_blocked_until: Option<Instant> = None;
    // Last server PI send — used to estimate RTT for the client FPS ping line (third PI field).
    let mut last_pi_sent_at = Some(Instant::now());
    let mut last_heartbeat_po = Instant::now();
    let mut last_client_activity = Instant::now();
    let mut expected_client_time = 0;
    let mut po_count: u64 = 0;
    let mut heartbeat_pings = 0u64;

    loop {
        let n = tokio::select! {
            r = reader.read_buf(&mut buf) => r?,
            _ = shutdown.recv() => {
                if let Some(pid) = player_id {
                    on_disconnect(&state, pid);
                }
                return Ok(());
            }
        };
        if n == 0 {
            tracing::info!("Read EOF from {addr}");
            // Disconnect
            if let Some(pid) = player_id {
                on_disconnect(&state, pid);
            }
            return Ok(());
        }

        tracing::debug!("Received {n} bytes from {addr}, buf len={}", buf.len());
        let now = Instant::now();
        state.prune_auth_failures_by_addr(now);
        let since_last_po_ms = now.duration_since(last_heartbeat_po).as_millis();
        let since_last_activity_ms = now.duration_since(last_client_activity).as_millis();
        if n > 0 {
            last_client_activity = now;
        }
        let mut saw_po = false;

        while let Some(pkt) = match Packet::try_decode(&mut buf) {
            Ok(pkt) => pkt,
            Err(err) => {
                tracing::warn!("Failed to decode packet from {addr}: {err}");
                return Err(err);
            }
        } {
            let event = pkt.event_str().to_string();
            crate::metrics::PACKETS_IN_TOTAL
                .with_label_values(&[event.as_str()])
                .inc();
            let payload_preview_text = if should_redact_payload(&event, &auth_state) {
                "[redacted]".to_string()
            } else {
                preview_payload_text(&pkt.payload, INCOMING_PACKET_PREVIEW)
            };
            tracing::info!(
                "<<< [{addr}] event={event} type={} len={} payload={payload_preview_text:?}",
                pkt.data_type as char,
                pkt.payload.len()
            );
            match event.as_str() {
                "AU" => {
                    if !matches!(auth_state, AuthState::Authenticated) {
                        let local_block = auth_blocked_remaining(auth_blocked_until, now);
                        let ip_block = state.auth_blocked_remaining_by_addr(&client_ip, now);
                        let block_remaining = merge_blocked_duration(local_block, ip_block);
                        if let Some(block_remaining) = block_remaining {
                            send_auth_blocked_message(&tx, block_remaining);
                            continue;
                        }
                    }

                    if let Some(au) = AuClientPacket::decode(&pkt.payload) {
                        let is_regular = matches!(au.auth_type, AuAuthType::Regular { .. });
                        player_id = handle_auth(&state, &tx, &au, sid, &mut auth_state)?;
                        if is_regular && player_id.is_some() {
                            clear_auth_rate_limit(
                                &mut auth_failure_count,
                                &mut auth_failure_window,
                                &mut auth_blocked_until,
                                now,
                            );
                            state.clear_auth_failure_by_addr(&client_ip);
                        } else if is_regular && player_id.is_none() {
                            let mut blocked_remaining = record_auth_failure(
                                &mut auth_failure_count,
                                &mut auth_failure_window,
                                &mut auth_blocked_until,
                                now,
                            );
                            blocked_remaining = merge_blocked_duration(
                                blocked_remaining,
                                state.record_auth_failure_by_addr(&client_ip, now),
                            );
                            if let Some(remaining) = blocked_remaining {
                                send_auth_blocked_message(&tx, remaining);
                            }
                        }
                    } else {
                        tracing::warn!(
                            "Failed to parse AU payload from {addr}: len={}",
                            pkt.payload.len()
                        );
                        if matches!(auth_state, AuthState::WaitingAu) {
                            log_and_show_auth_ui(&tx, &state, sid, "MalformedAU");
                            auth_state = AuthState::ShowingGui;
                            let mut blocked_remaining = record_auth_failure(
                                &mut auth_failure_count,
                                &mut auth_failure_window,
                                &mut auth_blocked_until,
                                now,
                            );
                            blocked_remaining = merge_blocked_duration(
                                blocked_remaining,
                                state.record_auth_failure_by_addr(&client_ip, now),
                            );
                            if let Some(remaining) = blocked_remaining {
                                send_auth_blocked_message(&tx, remaining);
                            }
                        }
                    }
                }
                "TY" => {
                    if let Some(ty) = TyPacket::decode(&pkt.payload) {
                        let ty_event = ty.event_str();
                        let ty_payload_preview_text =
                            if matches!(auth_state, AuthState::Authenticated) {
                                preview_payload_text(&ty.sub_payload, TY_SUBPAYLOAD_PREVIEW_LEN)
                            } else {
                                "[redacted]".to_string()
                            };
                        tracing::debug!(
                            "TY parsed: pid={:?} ev={} t={} x={} y={} sub_len={} payload={:?}",
                            player_id,
                            ty_event,
                            ty.client_timestamp(),
                            ty.x,
                            ty.y,
                            ty.sub_payload.len(),
                            ty_payload_preview_text,
                        );
                        if let Some(pid) = player_id {
                            handle_ty(&state, &tx, pid, &ty);
                        } else if ty_event == "GUI_" {
                            // Auth GUI flow — no player yet
                            if let Some(button) = decode_gui_button(&ty.sub_payload) {
                                if !matches!(auth_state, AuthState::Authenticated) {
                                    let local_block =
                                        auth_blocked_remaining(auth_blocked_until, now);
                                    let ip_block =
                                        state.auth_blocked_remaining_by_addr(&client_ip, now);
                                    let block_remaining =
                                        merge_blocked_duration(local_block, ip_block);
                                    if let Some(block_remaining) = block_remaining {
                                        send_auth_blocked_message(&tx, block_remaining);
                                        continue;
                                    }
                                }

                                let is_password_flow = button.starts_with("passwd:")
                                    || button.starts_with("newpasswd:");
                                player_id =
                                    handle_auth_gui(&state, &tx, &button, sid, &mut auth_state);
                                if matches!(auth_state, AuthState::Authenticated) {
                                    clear_auth_rate_limit(
                                        &mut auth_failure_count,
                                        &mut auth_failure_window,
                                        &mut auth_blocked_until,
                                        now,
                                    );
                                    state.clear_auth_failure_by_addr(&client_ip);
                                } else if is_password_flow && player_id.is_none() {
                                    let mut blocked_remaining = record_auth_failure(
                                        &mut auth_failure_count,
                                        &mut auth_failure_window,
                                        &mut auth_blocked_until,
                                        now,
                                    );
                                    blocked_remaining = merge_blocked_duration(
                                        blocked_remaining,
                                        state.record_auth_failure_by_addr(&client_ip, now),
                                    );
                                    if let Some(remaining) = blocked_remaining {
                                        send_auth_blocked_message(&tx, remaining);
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    "TY GUI_ payload parse failed before auth from {}",
                                    addr
                                );
                            }
                        } else {
                            tracing::warn!("TY before auth and not GUI_ event: {ty_event}");
                        }
                    } else {
                        tracing::warn!(
                            "Failed to decode TY payload from {addr}: len={}",
                            pkt.payload.len()
                        );
                    }
                }
                "PO" => {
                    saw_po = true;
                    po_count = po_count.saturating_add(1);
                    if let Some(pong) = PongClient::decode(&pkt.payload) {
                        tracing::debug!(
                            po_count,
                            pong_code = pong.response,
                            client_time = pong.current_time,
                            "PO received"
                        );
                        if expected_client_time == 0 {
                            expected_client_time = pong.current_time;
                        }
                        let since_last = last_pi_sent_at
                            .map(|t| now.saturating_duration_since(t))
                            .unwrap_or(Duration::ZERO)
                            .as_millis();
                        let ping_label = format!(
                            "{} ",
                            pong.current_time - (expected_client_time - HEARTBEAT_RTT_BASE_MS)
                        );
                        tracing::debug!(
                            po_count,
                            pong_code = pong.response,
                            po_client_time = pong.current_time,
                            since_last_ms = since_last,
                            ping_label = %ping_label,
                            expected_client_time,
                            "PI response prepared"
                        );
                        let ping_count = po_count.saturating_add(1);
                        send_u_packet(
                            &tx,
                            "PI",
                            &ping(52, pong.current_time + 1, ping_label.as_str()).1,
                        );
                        expected_client_time = pong.current_time + HEARTBEAT_RTT_BASE_MS;
                        last_pi_sent_at = Some(Instant::now());
                        last_heartbeat_po = now;
                        tracing::debug!(
                            po_count,
                            ping_count,
                            pi_client_time = pong.current_time + 1,
                            "PI queued immediately after PO"
                        );
                    } else {
                        tracing::warn!(
                            po_count,
                            payload = %payload_preview_text,
                            "Invalid PO payload, decode failed"
                        );
                    }
                }
                _ => {
                    tracing::debug!("Unknown packet: {event} from {addr}");
                }
            }
        }

        if !saw_po {
            if since_last_activity_ms > HEARTBEAT_DISCONNECT_TIMEOUT.as_millis() {
                tracing::warn!(
                    "Client heartbeat timeout: no client activity from {addr} for {since_last_activity_ms}ms, disconnecting"
                );
                if let Some(pid) = player_id {
                    on_disconnect(&state, pid);
                }
                return Err(anyhow::anyhow!(
                    "heartbeat timeout from {addr}: no client activity for {since_last_activity_ms}ms"
                ));
            } else if since_last_po_ms > HEARTBEAT_FALLBACK_INTERVAL.as_millis() {
                heartbeat_pings = heartbeat_pings.saturating_add(1);
                let expected = if expected_client_time == 0 {
                    0
                } else {
                    expected_client_time
                };
                let ping_label = format!("{} ", expected - (expected - HEARTBEAT_RTT_BASE_MS));
                tracing::debug!(
                    heartbeat_pings,
                    last_po_ms = since_last_po_ms,
                    expected_client_time = expected,
                    ping_client_time = expected + 1,
                    ping_label = %ping_label,
                    "PI fallback sent due heartbeat timeout"
                );
                let delayed_client_time = expected + 1;
                let delayed_label = ping_label.clone();
                let delayed_ping_idx = heartbeat_pings;
                send_u_packet(
                    &tx,
                    "PI",
                    &ping(52, delayed_client_time, delayed_label.as_str()).1,
                );
                tracing::debug!(
                    delayed_ping_idx,
                    expected_client_time = delayed_client_time - 1,
                    "PI fallback sent due heartbeat timeout"
                );
                expected_client_time = expected + HEARTBEAT_RTT_BASE_MS;
                last_heartbeat_po = now;
                tracing::debug!(
                    heartbeat_pings,
                    expected_client_time = expected_client_time,
                    "PI fallback scheduling completed"
                );
            }
        }
    }
}

fn preview_payload_text(payload: &[u8], max_len: usize) -> String {
    let end = payload.len().min(max_len);
    String::from_utf8_lossy(&payload[..end]).to_string()
}

fn should_redact_payload(event: &str, auth_state: &AuthState) -> bool {
    matches!(event, "AU" | "TY") && !matches!(auth_state, AuthState::Authenticated)
}

fn auth_blocked_remaining(auth_blocked_until: Option<Instant>, now: Instant) -> Option<Duration> {
    auth_blocked_until.and_then(|until| (now < until).then_some(until.duration_since(now)))
}

fn merge_blocked_duration(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.max(y)),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

const fn clear_auth_rate_limit(
    auth_failure_count: &mut u32,
    auth_failure_window: &mut Instant,
    auth_blocked_until: &mut Option<Instant>,
    now: Instant,
) {
    *auth_failure_count = 0;
    *auth_failure_window = now;
    *auth_blocked_until = None;
}

fn record_auth_failure(
    auth_failure_count: &mut u32,
    auth_failure_window: &mut Instant,
    auth_blocked_until: &mut Option<Instant>,
    now: Instant,
) -> Option<Duration> {
    match auth_blocked_until.as_ref() {
        Some(until) if now < *until => return None,
        Some(_) => {
            *auth_blocked_until = None;
            *auth_failure_count = 0;
            *auth_failure_window = now;
        }
        None => {}
    }

    if now.duration_since(*auth_failure_window) > GameState::AUTH_FAILURE_WINDOW {
        *auth_failure_count = 0;
        *auth_failure_window = now;
    }

    *auth_failure_count = auth_failure_count.saturating_add(1);
    if *auth_failure_count >= GameState::AUTH_FAILURE_LIMIT {
        let until = now + GameState::AUTH_BLOCK_DURATION;
        *auth_blocked_until = Some(until);
        return Some(until.duration_since(now));
    }
    None
}

fn send_auth_blocked_message(tx: &mpsc::UnboundedSender<Vec<u8>>, remaining: Duration) {
    let remaining_sec = remaining.as_secs().max(1);
    send_u_packet(
        tx,
        "OK",
        &ok_message(
            "Ошибка",
            &format!("Слишком много попыток входа. Подождите {remaining_sec} сек."),
        )
        .1,
    );
}
