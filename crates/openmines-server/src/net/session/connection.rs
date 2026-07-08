//! Обработка TCP-подключений и жизненного цикла сессии.
use crate::net::session::auth::gui_flow::handle_gui_auth_flow;
use crate::net::session::auth::login::handle_auth;
use crate::net::session::handshake::InitialHandshake;
use crate::net::session::heartbeat::SessionHeartbeat;
use crate::net::session::outbox::flush_outbox;
use crate::net::session::player::init::on_disconnect;
use crate::net::session::prelude::*;
use crate::net::session::state::HeartbeatGate;
use crate::net::session::wire;
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub use crate::net::session::state::{AuthState, GuiAuthStep};

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
    let mut heartbeat_gate = HeartbeatGate::WaitingForAuthResponse;
    // Токен этого сеанса — guard от reconnect-гонки в lifecycle-очереди.
    let session_token = state.next_session_token();
    let mut buf = BytesMut::with_capacity(4096);
    let mut heartbeat_state = SessionHeartbeat::new(Instant::now());
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
    // 400мс: норм. разрыв клиента ≈ RTT/2+400 ≈ 440мс; даже
    // пропущенный/сдвоенный тик под нагрузкой (~800-1200мс) остаётся
    // < 1500мс (клиентский порог «FREEZE»). 2.5 PI/с — НЕ шторм.
    let mut heartbeat = tokio::time::interval(Duration::from_millis(400));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // `tokio::time::interval` fires its first tick immediately. If we do not
    // consume it here, pre-auth heartbeat can race ahead of AU handling and send
    // `PI` before the mandatory first post-auth `cf` packet.
    heartbeat.tick().await;

    let handshake = InitialHandshake::build();
    let sid = handshake.session_id;
    tracing::Span::current().record("session_id", &sid);

    for packet in &handshake.packets {
        stream.write_all(packet).await?;
    }
    stream.flush().await?;
    tracing::debug!("Sent ST+AU+PI handshake");

    loop {
        let blocked_remaining = state.auth_blocked_remaining_by_addr(&client_ip, Instant::now());

        tokio::select! {
            _ = &mut kick_rx => {
                tracing::info!(player_id = ?auth_state.player_id(), "Player kicked via admin console");
                break;
            }
            _ = heartbeat.tick() => {
                if !heartbeat_gate.is_enabled() {
                    continue;
                }
                // Дисконнект мёртвого клиента: нет PO >30s (ref-порог
                // `Session.CheckDisconnected`). Реальный разрыв также
                // ловится `read_buf`==0 ниже.
                let now = Instant::now();
                let disconnect_timeout = std::time::Duration::from_secs(
                    state.config.gameplay.schedules.session_disconnect_timeout_secs,
                );
                if heartbeat_state.is_timed_out(now, disconnect_timeout) {
                    tracing::warn!(
                        timeout_secs = state.config.gameplay.schedules.session_disconnect_timeout_secs,
                        "Pong timeout. Closing connection"
                    );
                    break;
                }
                // 1 PI / тик → клиент 1 PO / PI → нет шторма. `num2` =
                // ЭКСТРАПОЛИРОВАННЫЕ текущие часы клиента (анти-FREEZE,
                // см. коммент у last_pi_sent_at): last_pong_ct + мс с
                // момента того PO. `text` = реальный RTT.
                let pi = heartbeat_state.next_ping_packet(now);
                send_u_packet(&tx, pi.0, &pi.1);
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
                    crate::metrics::PACKETS_IN_TOTAL.with_label_values(&[ev]).inc();
                    // PO (Pong) обрабатывается в любом состоянии — как в референсе Session.Ping()
                    if ev == "PO" {
                        if let Some(pong) = PongClient::decode(&packet.payload) {
                            // PO НЕ триггерит ответный PI (иначе tight-loop
                            // PO↔PI на RTT = шторм ~17/с). Тут только:
                            // liveness, измерение РЕАЛЬНОГО RTT (now −
                            // момент отправки того PI) для показа, и время
                            // клиента для num2 след. PI. PI шлёт heartbeat.
                            heartbeat_state.record_pong(&pong, Instant::now());
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
                                        tracing::Span::current().record("player_id", id.0);
                                        auth_state = new_auth;
                                        heartbeat_gate.mark_auth_response_queued();
                                        if let Some(kt) = kick_tx_opt.take() {
                                            state.register_kick_channel(id, kt);
                                        }
                                        tracing::info!(player_id = %id, "Player authenticated");
                                    } else {
                                        // Transition to GuiAuth so subsequent GUI_ TY packets are routed.
                                        auth_state = new_auth;
                                        heartbeat_gate.mark_auth_response_queued();
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
                                                tracing::Span::current().record("player_id", id.0);
                                                auth_state = AuthState::Authenticated { player_id: id };
                                                heartbeat_gate.mark_auth_response_queued();
                                                if let Some(kt) = kick_tx_opt.take() {
                                                    state.register_kick_channel(id, kt);
                                                }
                                                tracing::info!(player_id = %id, "Player registered/logged via GUI");
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        AuthState::Authenticated { player_id: id } => {
                                if ev == "TY" {
                                    if let Some(ty) = TyPacket::decode(&packet.payload) {
                                        let event = ty.event_str();
                                        if is_tick_action(event) {
                                            tracing::debug!(
                                                event = event,
                                                time = ty.client_timestamp(),
                                                "<<< [TY] enqueued"
                                            );
                                            let _ = state.commands_tx.send(crate::game::PlayerCommand::Ty {
                                                player_id: id,
                                                tx: tx.clone(),
                                                packet: ty,
                                            });
                                        } else {
                                            let event_owned = event.to_string();
                                            tracing::debug!(
                                                event = %event_owned,
                                                time = ty.client_timestamp(),
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
                                decoded = %wire::describe_wire_packet(&buf[..preview_len]),
                                "Wire decode error"
                            );
                            break;
                        }
                    }
                }
            }
            Some(out_packet) = rx.recv() => {
                flush_outbox(&mut stream, out_packet, &mut rx).await?;
                if heartbeat_gate.enable_if_auth_response_flushed() {
                    heartbeat_state.reset_liveness(Instant::now());
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

    if let Some(id) = auth_state.player_id() {
        state.unregister_kick_channel(id);
        on_disconnect(&state, id, session_token);
    }
    Ok(())
}

fn is_tick_action(event: &str) -> bool {
    matches!(
        event,
        "Xmov" | "Xdig" | "Xbld" | "Xgeo" | "Xhea" | "INVN" | "INCL" | "TADG" | "RESP" | "PROG"
    )
}
