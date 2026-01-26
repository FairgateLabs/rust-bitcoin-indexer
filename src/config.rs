use crate::settings::{DEFAULT_CHECKPOINT_HEIGHT, DEFAULT_CONFIRMATION_THRESHOLD};
use bitvmx_bitcoin_rpc::rpc_config::RpcConfig;
use serde::Deserialize;
use storage_backend::storage_config::StorageConfig;

#[derive(Deserialize, Debug)]
pub struct IndexerConfig {
    pub storage: StorageConfig,
    pub bitcoin: RpcConfig,
    pub settings: Option<IndexerSettings>,
    pub log_level: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct IndexerSettings {
    pub checkpoint_height: Option<u32>,
    pub confirmation_threshold: u32,
}

impl IndexerSettings {
    pub fn new(checkpoint_height: Option<u32>) -> Self {
        Self {
            checkpoint_height,
            confirmation_threshold: DEFAULT_CONFIRMATION_THRESHOLD,
        }
    }
}

impl Default for IndexerSettings {
    fn default() -> Self {
        IndexerSettings {
            checkpoint_height: Some(DEFAULT_CHECKPOINT_HEIGHT),
            confirmation_threshold: DEFAULT_CONFIRMATION_THRESHOLD,
        }
    }
}
