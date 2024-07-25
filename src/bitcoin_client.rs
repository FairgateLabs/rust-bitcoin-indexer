use crate::types::BlockHeight;
use anyhow::{Ok, Result};
use bitcoin::{Block, BlockHash};
use bitcoincore_rpc::{Auth, Client, RpcApi};
use mockall::automock;
use url::Url;

#[derive(Debug)]
pub struct BitcoinClient {
    pub client: Client,
}

impl BitcoinClient {
    pub fn new(url_str: &str) -> Result<Self> {
        let url = Url::parse(url_str)?;

        let auth = match url.password() {
            Some(p) => Auth::UserPass(url.username().to_owned(), p.to_owned()),
            None => Auth::None,
        };

        let client = Client::new(url.as_ref(), auth)?;

        Ok(Self { client })
    }
}

#[automock]
pub trait BitcoinClientApi {
    fn get_best_block(&self) -> Result<BlockHeight>;

    fn get_block_id_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>>;

    /// Get the block by id, along with id of the previous block hash
    fn get_block_by_id(&self, hash: &BlockHash) -> Result<Option<Block>>;

    fn get_blockchain_info(&self) -> Result<String>;
}

#[automock]
impl BitcoinClientApi for BitcoinClient {
    fn get_blockchain_info(&self) -> Result<String> {
        let network = self.client.get_blockchain_info()?.chain;
        Ok(network.to_string().to_uppercase())
    }

    fn get_best_block(&self) -> Result<BlockHeight> {
        Ok(self.client.get_block_count()? as u32)
    }

    fn get_block_id_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>> {
        let block_hash = self.client.get_block_hash(u64::from(height));

        if block_hash.is_err() {
            return Ok(None);
        }

        Ok(Some(block_hash.unwrap()))
    }

    fn get_block_by_id(&self, hash: &BlockHash) -> Result<Option<Block>> {
        let block = self.client.get_by_id(hash);

        if block.is_err() {
            return Ok(None);
        }

        Ok(Some(block.unwrap()))
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    #[ignore]
    fn get_data() -> Result<(), anyhow::Error> {
        let connector = BitcoinClient::new("http://user:password@localhost:18443")?;
        let count = connector.get_best_block()?;
        let block = connector.get_block_id_by_height(2).unwrap();

        println!("Display count {} ", count);
        println!("Display hash {:?} ", block);
        const STR_HASH: &str = "12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d";
        let block_hash = BlockHash::from_str(&STR_HASH)?;

        let block = connector.get_block_by_id(&block_hash)?;
        println!("Display block {:?} ", block);
        Ok(())
    }
}
