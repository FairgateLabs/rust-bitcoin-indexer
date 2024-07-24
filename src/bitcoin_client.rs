use crate::types::BlockHeight;
use anyhow::Result;
use bitcoin::{Block, BlockHash};
use bitcoincore_rpc::{Auth, Client, RpcApi};
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

        let client = Client::new(&url.to_string(), auth)?;

        Ok(Self { client })
    }
}

pub trait RpcClient {
    fn get_best_block(&self) -> Result<BlockHeight>;

    fn get_block_id_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>>;

    /// Get the block by id, along with id of the previous block hash
    fn get_block_by_id(&self, hash: &BlockHash) -> Result<Option<(Box<Block>, BlockHash)>>;
}

impl RpcClient for bitcoincore_rpc::Client {
    fn get_best_block(&self) -> Result<BlockHeight> {
        Ok(self.get_block_count()? as u32)
    }

    fn get_block_id_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>> {
        match self.get_block_hash(u64::from(height)) {
            Err(e) => {
                if e.to_string().contains("Block height out of range") {
                    Ok(None)
                } else {
                    Err(e.into())
                }
            }
            Ok(o) => Ok(Some(o)),
        }
    }

    fn get_block_by_id(&self, hash: &BlockHash) -> Result<Option<(Box<Block>, BlockHash)>> {
        let block: Box<Block> = match self.get_by_id(hash) {
            Err(e) => {
                if e.to_string().contains("Block height out of range") {
                    return Ok(None);
                } else {
                    return Err(e.into());
                }
            }
            Ok(o) => Box::new(o),
        };
        let prev_id = block.header.prev_blockhash;
        Ok(Some((block, prev_id)))
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
        let count = connector.client.get_best_block()?;
        let block = connector.client.get_block_id_by_height(2).unwrap();

        println!("Display count {} ", count);
        println!("Display hash {:?} ", block);
        const STR_HASH: &str = "12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d";
        let block_hash = BlockHash::from_str(&STR_HASH)?;

        let block = connector.client.get_block_by_id(&block_hash)?;
        println!("Display block {:?} ", block);
        Ok(())
    }
}
