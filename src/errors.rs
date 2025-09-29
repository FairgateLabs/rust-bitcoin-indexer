use bitvmx_bitcoin_rpc::errors::BitcoinClientError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexerStoreError {
    #[error("Error with the store client")]
    StoreError(#[from] storage_backend::error::StorageError),

    #[error("Block not found")]
    BlockNotFound,
}

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error("Error with the Bitcoin client")]
    BitcoinClientError(#[from] BitcoinClientError),

    #[error("Error with the store")]
    StoreError(#[from] IndexerStoreError),

    #[error("Inconsistent blockchain state")]
    InconsistentBlockchain,

    #[error("Indexed block hash does not match blockchain hash")]
    IndexedBlockHashMismatch,

    #[error("Database is corrupted")]
    DatabaseCorrupted,

    #[error("Checkpoint height is ahead of blockchain height")]
    CheckpointHeightAheadOfBlockchainHeight,

    #[error("Block not found")]
    BlockNotFound,

    #[error("Fee rate can't be estimated")]
    FeeRateNotEstimated,

    #[error("Indexer is not synchronized")]
    IndexerNotSynced,
}
