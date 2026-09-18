use bitvmx_bitcoin_rpc::errors::BitcoinClientError;
use bitvmx_bitcoin_rpc::types::BlockHeight;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error("Bad configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Bitcoin client error: {0}")]
    BitcoinClientError(#[from] BitcoinClientError),

    #[error("Storage backend error: {0}")]
    StorageError(#[from] storage_backend::error::StorageError),

    /// A block the indexer needs is neither stored nor available from the node.
    #[error("Block at height {0} not found")]
    BlockNotFound(BlockHeight),

    /// The node's chain changed below the oldest block the indexer holds, so the indexer cannot continue.
    #[error(
        "Reorg deeper than the retention window: no block stored at height {0} to continue from"
    )]
    ReorgDeeperThanWindow(BlockHeight),

    #[error("Fee rate can't be estimated")]
    FeeRateNotEstimated,

    #[error("Indexer is not synchronized")]
    NotSynced,

    #[error("The transaction is not confirmed")]
    NotConfirmed,

    /// Something the indexer could not do, with no better variant for it.
    #[error("Internal error: {0}")]
    Internal(String),

    /// Storage contradicts itself, which means a bug rather than a chain or node condition.
    #[error("Invariant violated: {0}")]
    InvariantViolation(String),
}
