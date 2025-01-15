use crate::errors::BitcoinClientError;
use crate::types::{BlockHeight, BlockInfo};
use bitcoin::{Block, BlockHash, Txid};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use mockall::automock;
use url::Url;

#[derive(Debug)]
pub struct BitcoinClient {
    pub client: Client,
}

impl BitcoinClient {
    pub fn new(url_str: &str) -> Result<Self, BitcoinClientError> {
        let url = Url::parse(url_str)?;

        let auth = match url.password() {
            Some(p) => Auth::UserPass(url.username().to_owned(), p.to_owned()),
            None => Auth::None,
        };

        let client = Client::new(url.as_ref(), auth).map_err(BitcoinClientError::NewClientError)?;

        Ok(Self { client })
    }

    pub fn new_with_parts(url: &str, user: &str, pass: &str) -> Result<Self, BitcoinClientError> {
        let auth = Auth::UserPass(user.to_owned(), pass.to_owned());

        let client = Client::new(url.as_ref(), auth).map_err(BitcoinClientError::NewClientError)?;

        Ok(Self { client })
    }
}

#[automock]
pub trait BitcoinClientApi {
    fn get_best_block(&self) -> Result<BlockHeight, BitcoinClientError>;

    fn get_block_by_height(
        &self,
        height: &BlockHeight,
    ) -> Result<Option<BlockInfo>, BitcoinClientError>;

    fn get_block_id_by_height(&self, height: &BlockHeight)
        -> Result<BlockHash, BitcoinClientError>;

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Block, BitcoinClientError>;

    fn get_blockchain_info(&self) -> Result<String, BitcoinClientError>;

    fn tx_exists(&self, tx_id: &Txid) -> bool;
}

#[automock]
impl BitcoinClientApi for BitcoinClient {
    fn tx_exists(&self, tx_id: &Txid) -> bool {
        let tx = self.client.get_raw_transaction_info(tx_id, None);
        tx.is_ok()
    }

    fn get_blockchain_info(&self) -> Result<String, BitcoinClientError> {
        let network = self.client.get_blockchain_info()?.chain;
        Ok(network.to_string().to_uppercase())
    }

    fn get_best_block(&self) -> Result<BlockHeight, BitcoinClientError> {
        let block_height = self.client.get_block_count()?;
        Ok(block_height as u32)
    }

    fn get_block_by_height(
        &self,
        height: &BlockHeight,
    ) -> Result<Option<BlockInfo>, BitcoinClientError> {
        let block_hash = self.get_block_id_by_height(height)?;

        let block = self.get_block_by_hash(&block_hash)?;

        let block_info = BlockInfo {
            hash: block_hash,
            height: *height,
            prev_hash: block.header.prev_blockhash,
            txs: block.txdata,
        };

        Ok(Some(block_info))
    }

    fn get_block_id_by_height(
        &self,
        height: &BlockHeight,
    ) -> Result<BlockHash, BitcoinClientError> {
        let block_hash = self
            .client
            .get_block_hash(u64::from(*height))
            .map_err(BitcoinClientError::ClientError)?;
        Ok(block_hash)
    }

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Block, BitcoinClientError> {
        let block = self
            .client
            .get_by_id(hash)
            .map_err(BitcoinClientError::ClientError)?;
        Ok(block)
    }
}
