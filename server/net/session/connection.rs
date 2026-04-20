//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::prelude::*;
use crate::net::session::auth::login::handle_auth;
use crate::net::session::dispatch::dispatch_ty_packet;
use crate::net::session::player::init::on_disconnect;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use bytes::BytesMut;

pub async fn handle(
    state: Arc<GameState>,
    mut stream: TcpStream,
    addr: SocketAddr,
) -> Result<()> {
    println!("[Net] New session from {}", addr);
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let client_ip = addr.ip();
    let mut auth_state = AuthState::PreAuth;
    let mut pid: Option<PlayerId> = None;
    let mut buf = BytesMut::with_capacity(4096);
    let mut next_expected: i32 = 0;

    // Референс: OnConnected шлёт ST → AU → PI (именно в таком порядке).
    let sid = GameState::generate_session_id();

    // Версия в ST — быстрая проверка, что клиент попал на свежую сборку (не старый процесс/образ).
    let st = status(concat!("OpenMines ", env!("CARGO_PKG_VERSION"), " rust"));
    let st_pkt = make_u_packet_bytes(st.0, &st.1);

    let au_init = au_session(&sid);
    let au_pkt = make_u_packet_bytes(au_init.0, &au_init.1);

    let pi = ping(0, 0, "");
    let pi_pkt = make_u_packet_bytes(pi.0, &pi.1);

    stream.write_all(&st_pkt).await?;
    stream.write_all(&au_pkt).await?;
    stream.write_all(&pi_pkt).await?;
    println!("[Net] Sent ST+AU+PI handshake (sid={}) to {}", sid, addr);

    loop {
        let blocked_remaining = state.auth_blocked_remaining_by_addr(&client_ip, Instant::now());
        
        tokio::select! {
            result = stream.read_buf(&mut buf) => {
                let n = result?;
                if n == 0 { 
                    println!("[Net] Connection closed by remote: {}", addr);
                    break; 
                }
                println!("[Net] Read {} bytes from {}", n, addr);
                while let Some(packet) = Packet::try_decode(&mut buf)? {
                    let ev = packet.event_str();
                    println!("[Net] Received packet {} from {}", ev, addr);
                    // PO (Pong) обрабатывается в любом состоянии — как в референсе Session.Ping()
                    if ev == "PO" {
                        if let Some(pong) = PongClient::decode(&packet.payload) {
                            if next_expected == 0 {
                                next_expected = pong.current_time;
                            }
                            let ct = pong.current_time;
                            let ne = next_expected;
                            let tx2 = tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                let text = format!("{} ", ct - (ne - 201));
                                let pi = ping(52, ct + 1, &text);
                                send_u_packet(&tx2, pi.0, &pi.1);
                            });
                            next_expected = ct + 201;
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
                                        println!("[Net] Player {} authenticated (id={:?})", addr, pid);
                                    } else {
                                        println!("[Net] Auth failed for {}", addr);
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
            }
            Some(out_packet) = rx.recv() => {
                stream.write_all(&out_packet).await?;
            }
        }
        if let Some(rem) = blocked_remaining {
            if rem > Duration::from_secs(0) { 
                println!("[Net] IP {} is blocked. Closing.", client_ip);
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
