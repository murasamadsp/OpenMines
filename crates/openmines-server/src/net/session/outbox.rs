use anyhow::Result;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};

pub const OUTBOX_CAPACITY: usize = 2_048;
const FLUSH_PACKET_BUDGET: usize = 256;
const FLUSH_BYTE_BUDGET: usize = 1024 * 1024;
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Clone, Debug)]
pub struct Outbox {
    tx: mpsc::Sender<Vec<u8>>,
    overflow_tx: watch::Sender<bool>,
    overflowed: Arc<AtomicBool>,
}

#[derive(Debug)]
pub enum OutboxSendError {
    Closed,
    Full,
}

impl Outbox {
    pub fn send(&self, packet: Vec<u8>) -> std::result::Result<(), OutboxSendError> {
        match self.tx.try_send(packet) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(OutboxSendError::Closed),
            Err(mpsc::error::TrySendError::Full(_)) => {
                if !self.overflowed.swap(true, Ordering::Relaxed) {
                    self.overflow_tx.send_replace(true);
                }
                Err(OutboxSendError::Full)
            }
        }
    }

    pub fn overflow_receiver(&self) -> watch::Receiver<bool> {
        self.overflow_tx.subscribe()
    }
}

pub fn channel() -> (Outbox, mpsc::Receiver<Vec<u8>>) {
    let (tx, rx) = mpsc::channel(OUTBOX_CAPACITY);
    let (overflow_tx, _) = watch::channel(false);
    (
        Outbox {
            tx,
            overflow_tx,
            overflowed: Arc::new(AtomicBool::new(false)),
        },
        rx,
    )
}

pub async fn flush_outbox(
    stream: &mut TcpStream,
    first_packet: Vec<u8>,
    rx: &mut mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let mut packets = 1usize;
    let mut bytes = first_packet.len();
    tokio::time::timeout(WRITE_TIMEOUT, stream.write_all(&first_packet)).await??;
    while packets < FLUSH_PACKET_BUDGET && bytes < FLUSH_BYTE_BUDGET {
        let Ok(packet) = rx.try_recv() else {
            break;
        };
        bytes = bytes.saturating_add(packet.len());
        packets += 1;
        tokio::time::timeout(WRITE_TIMEOUT, stream.write_all(&packet)).await??;
    }
    tokio::time::timeout(WRITE_TIMEOUT, stream.flush()).await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OUTBOX_CAPACITY, OutboxSendError, channel};

    #[test]
    fn bounded_outbox_signals_overflow() {
        let (outbox, _receiver) = channel();
        let overflow = outbox.overflow_receiver();
        for _ in 0..OUTBOX_CAPACITY {
            outbox.send(vec![1]).expect("outbox capacity");
        }

        assert!(matches!(outbox.send(vec![2]), Err(OutboxSendError::Full)));
        assert!(*overflow.borrow());
    }
}
