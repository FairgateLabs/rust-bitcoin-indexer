use thiserror::Error;

#[derive(Error, Debug)]
pub enum BitcoinClientError {
    #[error("Invalid block height")]
    InvalidHeight,

    #[error("Error parsing URL")]
    UrlParseError(#[from] url::ParseError),

    #[error("Error creating client")]
    ClientError(#[from] bitcoincore_rpc::Error),
}

#[derive(Error, Debug)]
pub enum IndexerStoreError {
    #[error("Error with Store client")]
    StoreError(#[from] storage_backend::error::StorageError),
}

#[derive(Error, Debug)]
pub enum IndexerError {
    #[error("Error with Bitcoin client")]
    BitcoinClientError(#[from] BitcoinClientError),

    #[error("Error with Store")]
    StoreError(#[from] IndexerStoreError),
}

