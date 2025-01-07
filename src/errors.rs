use thiserror::Error;

#[derive(Error, Debug)]
pub enum BitcoinClientError {
    #[error("Invalid block height")]
    InvalidHeight,

    #[error("Error parsing URL")]
    UrlParseError(#[from] url::ParseError),

    #[error("Error creating client")]
    NewClientError(#[from] bitcoincore_rpc::Error),

    #[error("Error getting blockchain info")] 
    ClientError(bitcoincore_rpc::Error),
}

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

