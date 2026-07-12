use std::sync::{Arc, OnceLock};

#[derive(Clone, Default)]
pub struct SimulationWaker {
    thread: Arc<OnceLock<std::thread::Thread>>,
}

impl SimulationWaker {
    pub fn register_current(&self) {
        let current = std::thread::current();
        if let Some(registered) = self.thread.get() {
            assert_eq!(
                registered.id(),
                current.id(),
                "simulation waker cannot be rebound to another thread"
            );
            return;
        }
        let _ = self.thread.set(current);
    }

    pub fn wake(&self) {
        if let Some(thread) = self.thread.get() {
            thread.unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn wake_before_park_is_not_lost() {
        let waker = SimulationWaker::default();
        let worker_waker = waker.clone();
        let registered = Arc::new(AtomicBool::new(false));
        let worker_registered = Arc::clone(&registered);
        let may_park = Arc::new(AtomicBool::new(false));
        let worker_may_park = Arc::clone(&may_park);
        let (unparked_tx, unparked_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            worker_waker.register_current();
            worker_registered.store(true, Ordering::Release);
            while !worker_may_park.load(Ordering::Acquire) {
                std::hint::spin_loop();
            }
            std::thread::park();
            unparked_tx.send(()).unwrap();
        });

        while !registered.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
        waker.wake();
        may_park.store(true, Ordering::Release);

        let result = unparked_rx.recv_timeout(Duration::from_secs(5));
        if result.is_err() {
            waker.wake();
        }
        worker.join().unwrap();
        assert!(result.is_ok(), "wake issued before park was lost");
    }
}
