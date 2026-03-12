#![recursion_limit = "256"]

pub mod crypto;
pub mod dht;
pub mod node;
pub mod sync;

pub use node::HvocNode;
pub use sync::SyncEvent;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum VeilidError {
    #[error("veilid core error: {0}")]
    Core(String),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("DHT error: {0}")]
    Dht(String),

    #[error("core domain error: {0}")]
    Domain(#[from] hvoc_core::CoreError),
}

impl From<veilid_core::VeilidAPIError> for VeilidError {
    fn from(e: veilid_core::VeilidAPIError) -> Self {
        VeilidError::Core(e.to_string())
    }
}
