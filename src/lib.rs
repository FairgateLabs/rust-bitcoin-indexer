use bitvmx_bitcoin_rpc::bitcoin_client::BitcoinClient;

use crate::indexer::Indexer;

pub mod config;
pub mod errors;
pub mod indexer;
pub mod store;

pub type IndexerType = Indexer<BitcoinClient>;
