use crate::constants::DEFAULT_CHECKPOINT_HEIGHT;
use bitvmx_bitcoin_rpc::rpc_config::RpcConfig;
use serde::Deserialize;
use storage_backend::storage_config::StorageConfig;

#[derive(Deserialize, Debug)]
pub struct IndexerConfig {
    pub storage: StorageConfig,
    pub bitcoin: RpcConfig,
    pub constants: Option<IndexerConstants>,
    pub log_level: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct IndexerConstants {
    pub checkpoint_height: Option<u32>,
}

impl IndexerConstants {
    pub fn new(checkpoint_height: Option<u32>) -> Self {
        Self { checkpoint_height }
    }
}

impl Default for IndexerConstants {
    fn default() -> Self {
        IndexerConstants {
            checkpoint_height: Some(DEFAULT_CHECKPOINT_HEIGHT),
        }
    }
}
