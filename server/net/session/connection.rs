//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::prelude::*;
use crate::net::session::auth::login::handle_auth;
use crate::net::session::dispatch::dispatch_ty_packet;
use crate::net::session::player::init::on_disconnect;

pub async fn handle(
    state: Arc<GameState>,
    mut stream: TcpStream,
    addr: SocketAddr,
) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let client_ip = addr.ip();
    let mut auth_state = AuthState::PreAuth;
    let mut pid: Option<PlayerId> = None;
    let mut buf = BytesMut::with_capacity(4096);
    let mut blocked_remaining: Option<Duration> = None;

    let now = Instant::now();
    state.prune_auth_failures_by_addr(now);

    if let Some(rem) = state.auth_blocked_remaining_by_addr(&client_ip, now) {
        let msg = format!("IP blocked for {}s", rem.as_secs());
        stream.write_all(&make_u_packet_bytes("ST", &status(&msg).1)).await?;
        return Ok(());
    }

    let sid = GameState::generate_session_id();

    loop {
        tokio::select! {
            result = stream.read_buf(&mut buf) => {
                let n = result?;
                if n == 0 { break; }
                while let Some(packet) = Packet::try_decode(&mut buf)? {
                    match auth_state {
                        AuthState::PreAuth => {
                            if packet.event_str() == "AU" {
                                if let Some(au) = AuClientPacket::decode(&packet.payload) {
                                    let now = Instant::now();
                                    if let Some(rem) = state.auth_blocked_remaining_by_addr(&client_ip, now) {
                                        stream.write_all(&make_u_packet_bytes("ST", &status(&format!("Blocked {}s", rem.as_secs())).1)).await?;
                                        continue;
                                    }

                                    match handle_auth(&state, &tx, &au, &sid, &mut auth_state).await {
                                        Ok(Some(id)) => {
                                            pid = Some(id);
                                            state.clear_auth_failure_by_addr(&client_ip);
                                        }
                                        Ok(None) => {
                                            let res = state.record_auth_failure_by_addr(&client_ip, now);
                                            blocked_remaining = merge_blocked_duration(blocked_remaining, res);
                                        }
                                        Err(e) => {
                                            tracing::error!("Auth error: {e}");
                                            let res = state.record_auth_failure_by_addr(&client_ip, now);
                                            blocked_remaining = merge_blocked_duration(blocked_remaining, res);
                                        }
                                    }
                                }
                            }
                        }
                        AuthState::Authenticated => {
                            if let Some(id) = pid {
                                if packet.event_str() == "TY" {
                                    if let Some(ty) = TyPacket::decode(&packet.payload) {
                                        let _ = dispatch_ty_packet(&state, &tx, id, &ty);
                                    }
                                } else if packet.event_str() == "PI" {
                                    if let Some(p) = PongClient::decode(&packet.payload) {
                                        send_u_packet(&tx, "PI", &ping(p.response, p.current_time, "ok").1);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(out_packet) = rx.recv() => {
                stream.write_all(&out_packet).await?;
            }
        }
        if let Some(rem) = blocked_remaining {
            if rem > Duration::from_secs(0) { break; }
        }
    }

    if let Some(id) = pid {
        on_disconnect(&state, id);
    }
    Ok(())
}

fn merge_blocked_duration(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(d1), Some(d2)) => Some(d1.max(d2)),
        (s @ Some(_), None) | (None, s @ Some(_)) => s,
        (None, None) => None,
    }
}

#[derive(PartialEq)]
pub enum AuthState {
    PreAuth,
    Authenticated,
}
