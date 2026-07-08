use anyhow::Result;
use bytes::BytesMut;
use clap::Parser;
use openmines_protocol::Packet;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

#[derive(Parser, Debug)]
#[command(
    name = "openmines-proxy",
    about = "OpenMines TCP Proxy for Zero-Downtime Updates"
)]
struct Args {
    /// Address to bind and listen on
    #[arg(long, default_value = "0.0.0.0:8090", env = "BIND_ADDR")]
    bind: String,

    /// Backend address to forward connections to
    #[arg(long, default_value = "127.0.0.1:8095", env = "BACKEND_ADDR")]
    backend: String,

    /// Timeout in seconds to wait for backend to reconnect before dropping client
    #[arg(long, default_value = "30", env = "RECONNECT_TIMEOUT")]
    reconnect_timeout: u64,
}

struct ProxyPacket {
    raw: Vec<u8>,
    event_name: [u8; 2],
}

impl ProxyPacket {
    fn try_decode(buf: &mut BytesMut) -> Option<Self> {
        let mut probe = buf.clone();
        let packet = match Packet::try_decode(&mut probe) {
            Ok(Some(packet)) => packet,
            Ok(None) => return None,
            Err(_) => {
                // Invalid length/frame: clear buffer to prevent infinite loop or memory blowup.
                buf.clear();
                return None;
            }
        };
        let raw = buf.split_to(packet.wire_len()).to_vec();
        Some(Self {
            raw,
            event_name: packet.event_name,
        })
    }

    const fn event(&self) -> [u8; 2] {
        self.event_name
    }
}

/// Что делать с пакетом нового бэкенда во время swallow-фазы после reconnect.
#[derive(Debug, PartialEq, Eq)]
enum SwallowDecision {
    /// Это часть `OnConnected` нового бэкенда (ST/AU/PI) — НЕ слать клиенту.
    Swallow,
    /// Реальные данные (`cf`/Init/геймплей) — переслать и завершить swallow-фазу.
    Forward,
}

/// Один раз глотаем `ST`, `AU`, `PI` (рукопожатие нового бэкенда после рестарта),
/// всё остальное форвардим клиенту. Повторный `ST`/`AU`/`PI` (флаг уже взведён)
/// трактуется как геймплей и пересылается — иначе подвисли бы навсегда.
const fn classify_handshake(
    ev: [u8; 2],
    swallowed_st: &mut bool,
    swallowed_au: &mut bool,
    swallowed_pi: &mut bool,
) -> SwallowDecision {
    match ev {
        [b'S', b'T'] if !*swallowed_st => {
            *swallowed_st = true;
            SwallowDecision::Swallow
        }
        [b'A', b'U'] if !*swallowed_au => {
            *swallowed_au = true;
            SwallowDecision::Swallow
        }
        [b'P', b'I'] if !*swallowed_pi => {
            *swallowed_pi = true;
            SwallowDecision::Swallow
        }
        _ => SwallowDecision::Forward,
    }
}

async fn read_from_backend(
    reader: &mut Option<OwnedReadHalf>,
    buf: &mut BytesMut,
) -> std::io::Result<usize> {
    if let Some(r) = reader {
        r.read_buf(buf).await
    } else {
        std::future::pending().await
    }
}

/// Один `write` (НЕ `write_all`): cancellation-safe внутри `select!` — если
/// победит другая ветка, ни байта не ушло. Возвращает сколько приняла ОС, чтобы
/// caller вычерпал ровно столько из очереди. `write_all` тут нельзя: при
/// частичной записи + отмене он бы переслал префикс заново → дубликат байт.
async fn write_to_backend(
    writer: &mut Option<OwnedWriteHalf>,
    data: &[u8],
) -> std::io::Result<usize> {
    if let Some(w) = writer {
        w.write(data).await
    } else {
        std::future::pending().await
    }
}

async fn attempt_reconnect(addr: String) -> std::io::Result<TcpStream> {
    loop {
        match TcpStream::connect(&addr).await {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

type ReconnectFuture = Pin<Box<dyn Future<Output = std::io::Result<TcpStream>> + Send>>;

#[allow(clippy::too_many_lines)]
async fn handle_client(
    client_stream: TcpStream,
    addr: SocketAddr,
    backend_addr: String,
    reconnect_timeout: u64,
) -> Result<()> {
    if let Err(e) = client_stream.set_nodelay(true) {
        tracing::warn!(session = %addr, error = ?e, "Failed to set nodelay");
    }

    tracing::info!(session = %addr, backend = %backend_addr, "Connecting to backend");
    let initial_backend = match TcpStream::connect(&backend_addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(session = %addr, error = ?e, "Initial backend connection failed");
            return Ok(());
        }
    };
    if let Err(e) = initial_backend.set_nodelay(true) {
        tracing::warn!(session = %addr, error = ?e, "Failed to set nodelay on backend");
    }

    // Split streams into owned halves to avoid borrow conflicts in select
    let (mut client_reader, mut client_writer) = client_stream.into_split();
    let (init_r, init_w) = initial_backend.into_split();
    let mut backend_reader = Some(init_r);
    let mut backend_writer = Some(init_w);

    let mut client_buf = BytesMut::with_capacity(8192);
    let mut backend_buf = BytesMut::with_capacity(8192);
    let mut to_backend_queue = Vec::<u8>::new();
    let mut to_client_queue = Vec::<u8>::new();

    let mut saved_au_packet: Option<Vec<u8>> = None;

    let mut swallow_handshake = false;
    let mut swallowed_st = false;
    let mut swallowed_au = false;
    let mut swallowed_pi = false;

    let mut reconnect_fut: Option<ReconnectFuture> = None;
    let mut reconnect_timeout_instant: Option<tokio::time::Instant> = None;

    loop {
        let timeout_sleep = async {
            if let Some(inst) = reconnect_timeout_instant {
                tokio::time::sleep_until(inst).await;
                true
            } else {
                std::future::pending().await
            }
        };

        tokio::select! {
            // Read from client
            res = client_reader.read_buf(&mut client_buf), if client_buf.len() < 32768 => {
                let n = res?;
                if n == 0 {
                    tracing::info!(session = %addr, "Client disconnected");
                    break;
                }
                // Save AU packet if we haven't yet
                if saved_au_packet.is_none() {
                    let mut temp_buf = client_buf.clone();
                    while let Some(packet) = ProxyPacket::try_decode(&mut temp_buf) {
                        if packet.event() == [b'A', b'U'] {
                            saved_au_packet = Some(packet.raw.clone());
                            tracing::info!(
                                session = %addr,
                                len = packet.raw.len(),
                                "Captured AU packet"
                            );
                            break;
                        }
                    }
                }
                // Forward client bytes to backend queue
                to_backend_queue.extend_from_slice(&client_buf.split());
            }

            // Write to client. `write` (cancel-safe), вычерпываем ровно n байт:
            // при отмене другой веткой ничего не ушло, дубликата не будет.
            res = client_writer.write(&to_client_queue), if !to_client_queue.is_empty() => {
                let n = res?;
                if n == 0 {
                    tracing::warn!(session = %addr, "Client write returned 0 — closing");
                    break;
                }
                to_client_queue.drain(0..n);
            }

            // Read from backend
            res = read_from_backend(&mut backend_reader, &mut backend_buf), if backend_reader.is_some() => {
                let n = res?;
                if n == 0 {
                    tracing::warn!(session = %addr, "Backend disconnected. Reconnecting...");
                    backend_reader = None;
                    backend_writer = None;
                    swallow_handshake = true;
                    swallowed_st = false;
                    swallowed_au = false;
                    swallowed_pi = false;

                    reconnect_timeout_instant = Some(tokio::time::Instant::now() + Duration::from_secs(reconnect_timeout));
                    reconnect_fut = Some(Box::pin(attempt_reconnect(backend_addr.clone())));
                    continue;
                }

                if swallow_handshake {
                    while swallow_handshake && !backend_buf.is_empty() {
                        if let Some(packet) = ProxyPacket::try_decode(&mut backend_buf) {
                            let ev = packet.event();
                            match classify_handshake(
                                ev,
                                &mut swallowed_st,
                                &mut swallowed_au,
                                &mut swallowed_pi,
                            ) {
                                SwallowDecision::Swallow => {
                                    tracing::debug!(
                                        session = %addr,
                                        event = %format!("{}{}", ev[0] as char, ev[1] as char),
                                        "Swallowed event from new backend"
                                    );
                                }
                                SwallowDecision::Forward => {
                                    // Реальные данные — завершаем swallow-фазу.
                                    swallow_handshake = false;
                                    to_client_queue.extend_from_slice(&packet.raw);
                                }
                            }
                        } else {
                            break; // Wait for more backend data
                        }
                    }
                    if !swallow_handshake {
                        to_client_queue.extend_from_slice(&backend_buf.split());
                    }
                } else {
                    to_client_queue.extend_from_slice(&backend_buf.split());
                }
            }

            // Write to backend. Тоже `write`+drain(n). ВНИМАНИЕ: после этого
            // to_backend_queue = невыписанный ХВОСТ, не обязательно по границе
            // пакета. Если бэкенд умрёт ровно здесь, при reconnect мы пришлём
            // [AU][обрывок-хвоста] — новый бэкенд распарсит обрывок как длину и
            // рассинхронится. Чистого packet-boundary recovery для client→backend
            // байтов на рестарте нет (старый write_all-вариант вместо этого
            // двойным-применял уже обработанные пакеты — тоже неверно, просто
            // иначе). Полный фикс — фрейминг очереди по пакетам, вне scope.
            res = write_to_backend(&mut backend_writer, &to_backend_queue), if backend_writer.is_some() && !to_backend_queue.is_empty() => {
                let n = res?;
                if n == 0 {
                    tracing::warn!(session = %addr, "Backend write returned 0 — closing");
                    break;
                }
                to_backend_queue.drain(0..n);
            }

            // Reconnect future completed
            reconnect_res = async {
                if let Some(ref mut fut) = reconnect_fut {
                    fut.await
                } else {
                    std::future::pending().await
                }
            } => {
                match reconnect_res {
                    Ok(stream) => {
                        tracing::info!(session = %addr, "Reconnected to new backend instance");
                        let (new_r, new_w) = stream.into_split();
                        backend_reader = Some(new_r);
                        backend_writer = Some(new_w);
                        reconnect_fut = None;
                        reconnect_timeout_instant = None;

                        // Replay saved AU packet immediately
                        if let Some(ref au) = saved_au_packet {
                            tracing::info!(session = %addr, "Replaying saved AU packet");
                            to_backend_queue.splice(0..0, au.iter().copied());
                        }
                    }
                    Err(e) => {
                        tracing::error!(session = %addr, error = ?e, "Reconnect attempt failed; retrying");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        reconnect_fut = Some(Box::pin(attempt_reconnect(backend_addr.clone())));
                    }
                }
            }

            // Reconnect timeout
            _timed_out = timeout_sleep => {
                tracing::error!(
                    session = %addr,
                    timeout_secs = reconnect_timeout,
                    "Reconnect timed out. Closing connection."
                );
                break;
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(bind = %args.bind, "Starting openmines-proxy");
    tracing::info!(backend = %args.backend, "Forwarding target backend");
    tracing::info!(
        reconnect_timeout = args.reconnect_timeout,
        "Reconnect timeout configured"
    );

    let listener = tokio::net::TcpListener::bind(&args.bind).await?;

    loop {
        let (stream, addr) = match listener.accept().await {
            Ok(val) => val,
            Err(e) => {
                tracing::error!(error = ?e, "Accept failed; retrying in 200ms");
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
        };

        tracing::info!(client_ip = %addr, "New connection accepted by proxy");
        let backend_addr = args.backend.clone();
        let reconnect_timeout = args.reconnect_timeout;

        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, addr, backend_addr, reconnect_timeout).await {
                tracing::error!(session = %addr, error = ?e, "Session error");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyPacket, SwallowDecision, classify_handshake};
    use bytes::{BufMut, BytesMut};

    /// Собрать wire-кадр: `[len u32 LE (вкл. эти 4B)][type][2B event][payload]`.
    fn frame(event: [u8; 2], payload: &[u8]) -> Vec<u8> {
        let len = 4 + 1 + 2 + payload.len();
        let mut b = BytesMut::new();
        b.put_u32_le(u32::try_from(len).expect("test frame length must fit u32"));
        b.put_u8(b'U');
        b.put_slice(&event);
        b.put_slice(payload);
        b.to_vec()
    }

    #[test]
    fn try_decode_needs_full_length_then_splits() {
        let pkt = frame(*b"AU", b"hello");
        let mut buf = BytesMut::from(&pkt[..pkt.len() - 1]); // на 1 байт короче
        assert!(
            ProxyPacket::try_decode(&mut buf).is_none(),
            "неполный кадр ждёт"
        );
        buf.extend_from_slice(&pkt[pkt.len() - 1..]); // дослали хвост
        let p = ProxyPacket::try_decode(&mut buf).expect("полный кадр декодится");
        assert_eq!(p.event(), [b'A', b'U']);
        assert!(buf.is_empty(), "буфер вычерпан ровно на один кадр");
    }

    #[test]
    fn try_decode_leaves_trailing_bytes_for_next_packet() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&frame(*b"ST", b"x"));
        buf.extend_from_slice(&frame(*b"PI", b"0:0:"));
        let first = ProxyPacket::try_decode(&mut buf).unwrap();
        assert_eq!(first.event(), [b'S', b'T']);
        let second = ProxyPacket::try_decode(&mut buf).unwrap();
        assert_eq!(second.event(), [b'P', b'I']);
        assert!(ProxyPacket::try_decode(&mut buf).is_none());
    }

    #[test]
    fn try_decode_clears_buffer_on_bogus_length() {
        let mut buf = BytesMut::new();
        buf.put_u32_le(3); // < минимума 7
        buf.put_slice(b"garbage");
        assert!(ProxyPacket::try_decode(&mut buf).is_none());
        assert!(buf.is_empty(), "битая длина чистит буфер, без зацикливания");
    }

    #[test]
    fn handshake_swallows_st_au_pi_once_then_forwards_gameplay() {
        let (mut st, mut au, mut pi) = (false, false, false);
        // Новый бэкенд после рестарта: ST, AU, PI глотаем; cf — форвардим.
        assert_eq!(
            classify_handshake(*b"ST", &mut st, &mut au, &mut pi),
            SwallowDecision::Swallow
        );
        assert_eq!(
            classify_handshake(*b"AU", &mut st, &mut au, &mut pi),
            SwallowDecision::Swallow
        );
        assert_eq!(
            classify_handshake(*b"PI", &mut st, &mut au, &mut pi),
            SwallowDecision::Swallow
        );
        assert_eq!(
            classify_handshake(*b"cf", &mut st, &mut au, &mut pi),
            SwallowDecision::Forward
        );
    }

    #[test]
    fn handshake_second_pi_is_forwarded_not_swallowed_forever() {
        let (mut st, mut au, mut pi) = (false, false, false);
        assert_eq!(
            classify_handshake(*b"PI", &mut st, &mut au, &mut pi),
            SwallowDecision::Swallow
        );
        // Второй PI (сервер шлёт PI в ответ на PO) — уже геймплей, не глотать.
        assert_eq!(
            classify_handshake(*b"PI", &mut st, &mut au, &mut pi),
            SwallowDecision::Forward
        );
    }
}
