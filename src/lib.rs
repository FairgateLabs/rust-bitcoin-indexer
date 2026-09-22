use bitvmx_bitcoin_rpc::bitcoin_client::BitcoinClient;

pub mod config;
pub mod errors;
pub mod indexer;
pub mod store; // Only the indexer writes to storage. The module stays reachable so the integration tests can read what a tick wrote.
pub mod types;

pub(crate) mod helper;

#[cfg(test)]
pub mod test_utils;

// Re-exported so consumers can reach the core types from the crate root instead of through the module they
// happen to live in today.
pub use config::IndexerSettings;
pub use errors::IndexerError;
pub use indexer::Indexer;
pub use types::{FullBlock, TransactionStatus};

pub type IndexerType = Indexer<BitcoinClient>;
