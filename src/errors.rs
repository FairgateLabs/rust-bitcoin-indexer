use bitvmx_bitcoin_rpc::errors::BitcoinClientError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum IndexerStoreError {
    #[error("Error with Store client")]
    StoreError(#[from] storage_backend::error::StorageError),

    #[error("Block not found")]
    BlockNotFound,
}

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error("Error with Bitcoin client")]
    BitcoinClientError(#[from] BitcoinClientError),

    #[error("Error with Store")]
    StoreError(#[from] IndexerStoreError),
}
