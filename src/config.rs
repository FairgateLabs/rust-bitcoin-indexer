use bitvmx_bitcoin_rpc::rpc_config::RpcConfig;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ConfigIndexer {
    pub db_file_path: String,
    pub rpc: RpcConfig,
    pub checkpoint_height: Option<u32>,
}
