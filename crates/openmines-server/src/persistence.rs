pub mod handle;
pub mod runtime;
pub mod store;
#[cfg(test)]
pub mod tests;
pub mod worker;

pub use handle::{PersistenceAdmissionError, PersistenceHandle, PersistencePermit};
pub use runtime::PersistenceRuntime;

pub use crate::game::{SaveCommand, SaveKind};
pub use std::sync::Arc;

pub use handle::{PersistenceEnvelope, PersistenceStatus};
pub use store::{PersistenceStore, PersistenceStoreFailure};

#[allow(dead_code)]
pub const QUEUE_CAPACITY: usize = 4_096;
pub const BATCH_LIMIT: usize = 128;
pub const RETRY_INITIAL_BACKOFF: std::time::Duration = std::time::Duration::from_millis(25);
pub const RETRY_MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(2);
pub const BUILDING_DELETE_MAX_ATTEMPTS: u64 = 8;
