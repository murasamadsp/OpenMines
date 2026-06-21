//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::auth::gui_flow::handle_gui_auth_flow;
use crate::net::session::auth::login::handle_auth;
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
    let mut last_pong = Instant::now();
    // Серверно-пейсированный PI (фикс PI-шторма ~17/с): сервер сам шлёт
    // PI раз в HEARTBEAT (400мс), клиент шлёт 1 PO на 1 PI → частота PI
    // = частота тика (НЕ tight-loop PO↔PI на RTT = шторм). `text` =
    // РЕАЛЬНЫЙ измеренный RTT (`now − момент отправки PI`) — реальный
    // пинг развязан с пейсингом.
    //
    // `num2` (= клиентский `lastPITime`): клиент пишет «FREEZE» если
    // `NowTime() − lastPITime > 1500мс` (`ServerTime.cs:155`). Поэтому
    // `num2` ОБЯЗАН ≈ ТЕКУЩИЕ часы клиента. PI шлётся МЕЖДУ PO, свежего
    // pong на руках нет → ЭКСТРАПОЛИРУЕМ: `last_pong_ct + (now −
    // last_pong)` (часы клиента ≈ мс от handshake, дрейф за <1с
    // ничтожен). До 1-го PO: `last_pong_ct=0`, `last_pong`=старт цикла
    // ≈ handshake → оценка ≈ клиентский NowTime. Это и есть анти-FREEZE
    // (не частота тика — прошлая версия ставила num2 на раунд позже →
    // постоянный FREEZE).
    let mut last_pi_sent_at: Option<Instant> = None;
    let mut last_rtt_ms: i32 = 50;
    let mut last_pong_ct: i32 = 0;
    // 400мс: норм. разрыв клиента ≈ RTT/2+400 ≈ 440мс; даже
    // пропущенный/сдвоенный тик под нагрузкой (~800-1200мс) остаётся
    // < 1500мс (клиентский порог «FREEZE»). 2.5 PI/с — НЕ шторм.
    let mut heartbeat = tokio::time::interval(Duration::from_millis(400));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

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
                // Дисконнект мёртвого клиента: нет PO >30s (ref-порог
                // `Session.CheckDisconnected`). Реальный разрыв также
                // ловится `read_buf`==0 ниже.
                let idle = Instant::now().saturating_duration_since(last_pong);
                if idle > Duration::from_secs(30) {
                    tracing::warn!("Pong timeout (>30s). Closing {addr}");
                    break;
                }
                // 1 PI / тик → клиент 1 PO / PI → нет шторма. `num2` =
                // ЭКСТРАПОЛИРОВАННЫЕ текущие часы клиента (анти-FREEZE,
                // см. коммент у last_pi_sent_at): last_pong_ct + мс с
                // момента того PO. `text` = реальный RTT.
                let since_pong_ms = i32::try_from(
                    Instant::now()
                        .saturating_duration_since(last_pong)
                        .as_millis(),
                )
                .unwrap_or(i32::MAX);
                let num2 = last_pong_ct
                    .saturating_add(since_pong_ms)
                    .saturating_add(1);
                let text = format!("{last_rtt_ms} ");
                let pi = ping(52, num2, &text);
                send_u_packet(&tx, pi.0, &pi.1);
                last_pi_sent_at = Some(Instant::now());
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
                            // PO НЕ триггерит ответный PI (иначе tight-loop
                            // PO↔PI на RTT = шторм ~17/с). Тут только:
                            // liveness, измерение РЕАЛЬНОГО RTT (now −
                            // момент отправки того PI) для показа, и время
                            // клиента для num2 след. PI. PI шлёт heartbeat.
                            last_pong = Instant::now();
                            if let Some(sent) = last_pi_sent_at {
                                let rtt_ms = last_pong.saturating_duration_since(sent).as_millis();
                                last_rtt_ms =
                                    i32::try_from(rtt_ms).unwrap_or(i32::MAX).clamp(1, 99_999);
                            }
                            last_pong_ct = pong.current_time;
                        }
                        continue;
                    }

                    match auth_state {
                        AuthState::PreAuth => {
                            if ev == "AU" {
                                if let Some(au) = AuClientPacket::decode(&packet.payload) {
                                    let now = Instant::now();
                                    let mut new_auth = auth_state.clone();
                                    let res = handle_auth(&state, &tx, &au, &sid, &mut new_auth).await?;
                                    if let Some(id) = res {
                                        pid = Some(id);
                                        auth_state = new_auth;
                                        tracing::info!("Player {addr} authenticated (id={:?})", pid);
                                    } else {
                                        // Transition to GuiAuth so subsequent GUI_ TY packets are routed.
                                        auth_state = new_auth;
                                        tracing::warn!("Auth failed for {addr}");
                                        let wait = state.record_auth_failure_by_addr(&client_ip, now);
                                        if wait.is_some() { break; }
                                    }
                                }
                            }
                        }
                        AuthState::GuiAuth(ref mut step) => {
                            // C# ref: Session.GUI routes to auth.CallAction(button) when auth is active.
                            if ev == "TY" {
                                if let Some(ty) = TyPacket::decode(&packet.payload) {
                                    if ty.event_str() == "GUI_" {
                                        if let Some(button) = decode_gui_button(&ty.sub_payload) {
                                            let res = handle_gui_auth_flow(&state, &tx, &button, step).await?;
                                            if let Some(id) = res {
                                                pid = Some(id);
                                                auth_state = AuthState::Authenticated;
                                                tracing::info!("Player {addr} registered/logged via GUI (id={:?})", pid);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        AuthState::Authenticated => {
                            if let Some(id) = pid {
                                if ev == "TY" {
                                    if let Some(ty) = TyPacket::decode(&packet.payload) {
                                        tracing::debug!("<<< [TY] enqueued event={} pid={} time={}", ty.event_str(), id, ty.time);
                                        state.incoming_actions.push(id, tx.clone(), ty);
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
        on_disconnect(&state, id).await;
    }
    Ok(())
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum AuthState {
    PreAuth,
    GuiAuth(GuiAuthStep),
    Authenticated,
}

/// Sub-state of the GUI auth flow (registration / login through client GUI).
/// 1:1 with C# `Auth` class state machine.
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum GuiAuthStep {
    /// Default window: "Новый акк" / "ok" (nick input).
    MainMenu,
    /// Player found by nick, waiting for password input.
    LoginPassword { nick: String },
    /// Creating new account — waiting for nick input.
    RegisterNick,
    /// Creating new account — nick accepted, waiting for password.
    RegisterPassword { nick: String },
}
