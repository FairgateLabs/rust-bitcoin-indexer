use bitvmx_bitcoin_rpc::rpc_config::RpcConfig;
use serde::Deserialize;
use storage_backend::storage_config::StorageConfig;

#[derive(Deserialize, Debug)]
pub struct ConfigIndexer {
    pub storage: StorageConfig,
    pub bitcoin: RpcConfig,
    pub checkpoint_height: Option<u32>,
    pub log_level: Option<String>,
}
