use crate::types::{BlockHeight, BlockInfo};
use anyhow::{Ok, Result};
use bitcoin::{Block, BlockHash, Txid};
use bitcoincore_rpc::bitcoin::consensus::encode;
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

    fn get_block_by_height(&self, height: &BlockHeight) -> Result<Option<BlockInfo>>;

    fn get_block_id_by_height(&self, height: &BlockHeight) -> Result<Option<BlockHash>>;

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>>;

    fn get_blockchain_info(&self) -> Result<String>;

    fn tx_exists(&self, tx_id: &Txid) -> Result<bool>;

    fn get_tx_hex(&self, tx_id: &Txid) -> Result<String>;
}

#[automock]
impl BitcoinClientApi for BitcoinClient {
    fn tx_exists(&self, tx_id: &Txid) -> Result<bool> {
        let tx = self.client.get_raw_transaction_info(tx_id, None);
        Ok(tx.is_ok())
    }

    fn get_tx_hex(&self, tx_id: &Txid) -> Result<String> {
        let tx = self
            .client
            .get_raw_transaction(tx_id, None)
            .expect("Transaction not found");

        let hex = encode::serialize_hex(&tx);

        Ok(hex)
    }

    fn get_blockchain_info(&self) -> Result<String> {
        let network = self.client.get_blockchain_info()?.chain;
        Ok(network.to_string().to_uppercase())
    }

    fn get_best_block(&self) -> Result<BlockHeight> {
        let block_height = self.client.get_block_count();
        Ok(block_height? as u32)
    }

    fn get_block_by_height(&self, height: &BlockHeight) -> Result<Option<BlockInfo>> {
        let block_hash = self.get_block_id_by_height(height)?;

        if block_hash.is_none() {
            return Ok(None);
        }

        let block_hash = block_hash.unwrap();
        let block = self.get_block_by_hash(&block_hash)?.unwrap();

        let block_info = BlockInfo {
            hash: block_hash,
            height: *height,
            prev_hash: block.header.prev_blockhash,
            txs: block.txdata.iter().map(|tx| tx.compute_txid()).collect(),
        };

        Ok(Some(block_info))
    }

    fn get_block_id_by_height(&self, height: &BlockHeight) -> Result<Option<BlockHash>> {
        let block_hash = self.client.get_block_hash(u64::from(*height));

        if block_hash.is_err() {
            return Ok(None);
        }

        Ok(Some(block_hash.unwrap()))
    }

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>> {
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
        let bitcoin_client = BitcoinClient::new("http://user:password@localhost:18443")?;
        let count = bitcoin_client.get_best_block()?;
        let block = bitcoin_client.get_block_id_by_height(&2).unwrap();

        println!("Display count {} ", count);
        println!("Display hash {:?} ", block);
        let block_hash = BlockHash::from_str(
            &"12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d",
        )?;

        let _block = bitcoin_client.get_block_by_hash(&block_hash)?;

        let tx_id =
            Txid::from_str(&"0e099c2c53d69dc6f570f889a39ad918d7555f95492990a9e7d0392a68fbdbaf")
                .unwrap();

        let tx = bitcoin_client.get_tx_hex(&tx_id).unwrap();

        println!("Display tx {:?}", tx);
        Ok(())
    }
}
