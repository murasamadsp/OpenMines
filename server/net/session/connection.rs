//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::auth::gui_flow::handle_gui_auth_flow;
use crate::net::session::auth::login::handle_auth;
use crate::net::session::player::init::on_disconnect;
use crate::net::session::prelude::*;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tracing::instrument(
    name = "session",
    skip(state, stream),
    fields(
        client_ip = %addr.ip(),
        session_id = tracing::field::Empty,
        player_id = tracing::field::Empty
    )
)]
pub async fn handle(state: Arc<GameState>, mut stream: TcpStream, addr: SocketAddr) -> Result<()> {
    tracing::info!(ip = %addr.ip(), "New connection");
    // Важно для маленьких handshake-пакетов: не ждём Nagle на первых байтах.
    if let Err(err) = stream.set_nodelay(true) {
        tracing::warn!(error = %err, "set_nodelay failed");
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (kick_tx, mut kick_rx) = tokio::sync::oneshot::channel::<()>();
    let mut kick_tx_opt = Some(kick_tx);
    let client_ip = addr.ip();
    let mut auth_state = AuthState::PreAuth;
    let mut pid: Option<PlayerId> = None;
    // Токен этого сеанса — guard от reconnect-гонки в lifecycle-очереди.
    let session_token = state.next_session_token();
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
    let mut heartbeat_enabled = false;
    let mut auth_response_pending_flush = false;
    // 400мс: норм. разрыв клиента ≈ RTT/2+400 ≈ 440мс; даже
    // пропущенный/сдвоенный тик под нагрузкой (~800-1200мс) остаётся
    // < 1500мс (клиентский порог «FREEZE»). 2.5 PI/с — НЕ шторм.
    let mut heartbeat = tokio::time::interval(Duration::from_millis(400));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // `tokio::time::interval` fires its first tick immediately. If we do not
    // consume it here, pre-auth heartbeat can race ahead of AU handling and send
    // `PI` before the mandatory first post-auth `cf` packet.
    heartbeat.tick().await;

    // Референс: OnConnected шлёт ST → AU → PI (именно в таком порядке).
    let sid = GameState::generate_session_id();
    tracing::Span::current().record("session_id", &sid);

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
    tracing::debug!("Sent ST+AU+PI handshake");

    loop {
        let blocked_remaining = state.auth_blocked_remaining_by_addr(&client_ip, Instant::now());

        tokio::select! {
            _ = &mut kick_rx => {
                tracing::info!(player_id = ?pid, "Player kicked via admin console");
                break;
            }
            _ = heartbeat.tick() => {
                if !heartbeat_enabled {
                    continue;
                }
                // Дисконнект мёртвого клиента: нет PO >30s (ref-порог
                // `Session.CheckDisconnected`). Реальный разрыв также
                // ловится `read_buf`==0 ниже.
                let idle = Instant::now().saturating_duration_since(last_pong);
                if idle > Duration::from_secs(30) {
                    tracing::warn!("Pong timeout (>30s). Closing connection");
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
                    tracing::debug!("Connection closed by remote");
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
                                    let res = handle_auth(&state, &tx, &au, &sid, session_token, &mut new_auth).await?;
                                    if let Some(id) = res {
                                        pid = Some(id);
                                        tracing::Span::current().record("player_id", id.0);
                                        auth_state = new_auth;
                                        auth_response_pending_flush = true;
                                        if let Some(kt) = kick_tx_opt.take() {
                                            state.kick_channels.insert(id, kt);
                                        }
                                        tracing::info!(player_id = %id, "Player authenticated");
                                    } else {
                                        // Transition to GuiAuth so subsequent GUI_ TY packets are routed.
                                        auth_state = new_auth;
                                        auth_response_pending_flush = true;
                                        tracing::warn!("Auth failed");
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
                                            let res = handle_gui_auth_flow(&state, &tx, button.as_ref(), session_token, step).await?;
                                            if let Some(id) = res {
                                                pid = Some(id);
                                                tracing::Span::current().record("player_id", id.0);
                                                auth_state = AuthState::Authenticated;
                                                auth_response_pending_flush = true;
                                                if let Some(kt) = kick_tx_opt.take() {
                                                    state.kick_channels.insert(id, kt);
                                                }
                                                tracing::info!(player_id = %id, "Player registered/logged via GUI");
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
                                        let event = ty.event_str();
                                        if is_tick_action(event) {
                                            tracing::debug!(
                                                event = event,
                                                time = ty.time,
                                                "<<< [TY] enqueued"
                                            );
                                            state.incoming_actions.push(id, tx.clone(), ty);
                                        } else {
                                            let event_owned = event.to_string();
                                            tracing::debug!(
                                                event = %event_owned,
                                                time = ty.time,
                                                "<<< [TY] spawned async"
                                            );
                                            let state_c = state.clone();
                                            let tx_c = tx.clone();
                                            tokio::spawn(async move {
                                                if let Err(e) = crate::net::session::dispatch::dispatch_ty_packet(
                                                    &state_c, &tx_c, id, &ty
                                                ).await {
                                                    tracing::error!(
                                                        player_id = %id,
                                                        event = %event_owned,
                                                        error = ?e,
                                                        "Failed to dispatch async TY packet"
                                                    );
                                                }
                                            });
                                        }
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
                                error = %err,
                                buf_len = buf.len(),
                                preview = %preview,
                                "Wire decode error"
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
                if auth_response_pending_flush {
                    auth_response_pending_flush = false;
                    heartbeat_enabled = true;
                    last_pong = Instant::now();
                }
            }
        }
        if let Some(rem) = blocked_remaining {
            if rem > Duration::from_secs(0) {
                tracing::warn!(ip = %client_ip, "IP blocked, closing connection");
                break;
            }
        }
    }

    if let Some(id) = pid {
        state.kick_channels.remove(&id);
        on_disconnect(&state, id, session_token);
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

fn is_tick_action(event: &str) -> bool {
    matches!(
        event,
        "Xmov" | "Xdig" | "Xbld" | "Xgeo" | "Xhea" | "INVN" | "INCL" | "TADG" | "RESP" | "PROG"
    )
}
