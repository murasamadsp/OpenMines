//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::auth::login::handle_auth;
use crate::net::session::dispatch::dispatch_ty_packet;
use crate::net::session::player::init::on_disconnect;
use crate::net::session::prelude::*;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub async fn handle(state: Arc<GameState>, mut stream: TcpStream, addr: SocketAddr) -> Result<()> {
    tracing::info!("New session from {addr}");
    // Важно для маленьких handshake-пакетов: не ждём Nagle на первых байтах.
    if let Err(err) = stream.set_nodelay(true) {
        tracing::warn!("set_nodelay failed for {addr}: {err}");
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let client_ip = addr.ip();
    let mut auth_state = AuthState::PreAuth;
    let mut pid: Option<PlayerId> = None;
    let mut buf = BytesMut::with_capacity(4096);
    let mut next_expected: i32 = 0;
    let mut last_pong = Instant::now();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));

    // Референс: OnConnected шлёт ST → AU → PI (именно в таком порядке).
    let sid = GameState::generate_session_id();

    // 1:1 ref: `SendU(new StatusPacket("черный хуй в твоей жопе"))`
    let st = status("черный хуй в твоей жопе");
    let st_pkt = make_u_packet_bytes(st.0, &st.1);

    let au_init = au_session(&sid);
    let au_pkt = make_u_packet_bytes(au_init.0, &au_init.1);

    let pi = ping(0, 0, "");
    let pi_pkt = make_u_packet_bytes(pi.0, &pi.1);

    stream.write_all(&st_pkt).await?;
    stream.write_all(&au_pkt).await?;
    stream.write_all(&pi_pkt).await?;
    stream.flush().await?;
    tracing::debug!("Sent ST+AU+PI handshake (sid={sid}) to {addr}");

    loop {
        let blocked_remaining = state.auth_blocked_remaining_by_addr(&client_ip, Instant::now());

        tokio::select! {
            _ = heartbeat.tick() => {
                // 1:1 ref: Session.CheckDisconnected()
                // - if now-lastpong > 30s => Disconnect()
                // - else if now-lastpong > 10s => Ping(new PongPacket(52, nextexpected))
                let idle = Instant::now().saturating_duration_since(last_pong);
                if idle > Duration::from_secs(30) {
                    tracing::warn!("Pong timeout (>30s). Closing {addr}");
                    break;
                }
                if idle > Duration::from_secs(10) && next_expected != 0 {
                    let ct = next_expected;
                    let ne = next_expected;
                    let text = format!("{} ", ct - (ne - 201));
                    let pi = ping(52, ct + 1, &text);
                    send_u_packet(&tx, pi.0, &pi.1);
                    next_expected = ct + 201;
                }
            }
            result = stream.read_buf(&mut buf) => {
                let n = result?;
                if n == 0 {
                    tracing::debug!("Connection closed by remote: {addr}");
                    break;
                }
                loop {
                    if buf.len() < 4 {
                        break;
                    }
                    match Packet::try_decode(&mut buf) {
                        Ok(Some(packet)) => {
                    let ev = packet.event_str();
                    // PO (Pong) обрабатывается в любом состоянии — как в референсе Session.Ping()
                    if ev == "PO" {
                        if let Some(pong) = PongClient::decode(&packet.payload) {
                            last_pong = Instant::now();
                            if next_expected == 0 {
                                next_expected = pong.current_time;
                            }
                            let ct = pong.current_time;
                            let ne = next_expected;
                            let text = format!("{} ", ct - (ne - 201));
                            next_expected = ct + 201;
                            let pi = ping(52, ct + 1, &text);
                            send_u_packet(&tx, pi.0, &pi.1);
                        }
                        continue;
                    }

                    match auth_state {
                        AuthState::PreAuth => {
                            if ev == "AU" {
                                if let Some(au) = AuClientPacket::decode(&packet.payload) {
                                    let now = Instant::now();
                                    let mut new_auth = auth_state;
                                    let res = handle_auth(&state, &tx, &au, &sid, &mut new_auth).await?;
                                    if let Some(id) = res {
                                        pid = Some(id);
                                        auth_state = new_auth;
                                        tracing::info!("Player {addr} authenticated (id={:?})", pid);
                                    } else {
                                        tracing::warn!("Auth failed for {addr}");
                                        let wait = state.record_auth_failure_by_addr(&client_ip, now);
                                        if wait.is_some() { break; }
                                    }
                                }
                            }
                        }
                        AuthState::Authenticated => {
                            if let Some(id) = pid {
                                if ev == "TY" {
                                    if let Some(ty) = TyPacket::decode(&packet.payload) {
                                        let _ = dispatch_ty_packet(&state, &tx, id, &ty);
                                    }
                                }
                            }
                        }
                    }
                        }
                        Ok(None) => {
                            // Недостаточно данных для полного пакета — ждём следующий read.
                            break;
                        }
                        Err(err) => {
                            let preview_len = buf.len().min(crate::net::session::wire::INCOMING_PACKET_PREVIEW);
                            let preview = if preview_len == 0 {
                                "<empty>".to_string()
                            } else {
                                format!("{:02x?}", &buf[..preview_len])
                            };
                            tracing::warn!(
                                "Wire decode error from {addr}: {err} (buf_len={} preview={preview})",
                                buf.len(),
                            );
                            break;
                        }
                    }
                }
            }
            Some(out_packet) = rx.recv() => {
                stream.write_all(&out_packet).await?;
                while let Ok(more) = rx.try_recv() {
                    stream.write_all(&more).await?;
                }
                stream.flush().await?;
            }
        }
        if let Some(rem) = blocked_remaining {
            if rem > Duration::from_secs(0) {
                tracing::warn!("IP {client_ip} is blocked. Closing.");
                break;
            }
        }
    }

    if let Some(id) = pid {
        on_disconnect(&state, id);
    }
    Ok(())
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum AuthState {
    PreAuth,
    Authenticated,
}
