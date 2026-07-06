use anyhow::Result;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

pub async fn flush_outbox(
    stream: &mut TcpStream,
    first_packet: Vec<u8>,
    rx: &mut mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<()> {
    stream.write_all(&first_packet).await?;
    while let Ok(packet) = rx.try_recv() {
        stream.write_all(&packet).await?;
    }
    stream.flush().await?;
    Ok(())
}
