use crate::indexer::Indexer;
use bitvmx_bitcoin_rpc::bitcoin_client::BitcoinClient;

pub mod config;
pub mod constants;
pub mod errors;
pub mod indexer;
pub mod store;
pub mod types;

pub type IndexerType = Indexer<BitcoinClient>;
