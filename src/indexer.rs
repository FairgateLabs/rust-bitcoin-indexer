use crate::{bitcoin_client::BitcoinClientApi, store::StoreClient, types::BlockHeight};
use anyhow::Result;
use bitcoin::Txid;
use log::{error, info, warn};
use mockall::automock;
use std::sync::Arc;
pub struct Indexer {
    pub bitcoin_client: Arc<dyn BitcoinClientApi>,
    pub store: Arc<dyn StoreClient>,
}

#[automock]
pub trait IndexerApi {
    fn get_best_block(&self) -> Result<Option<BlockHeight>>;
    fn tx_exists(&self, tx_id: &Txid) -> Result<bool>;
    fn get_tx(&self, tx_id: &Txid) -> Result<String>;
}

impl Indexer {
    pub fn new(
        bitcoin_indexer_client: Arc<dyn BitcoinClientApi>,
        store: Arc<dyn StoreClient>,
    ) -> Result<Self> {
        Ok(Self {
            bitcoin_client: bitcoin_indexer_client,
            store,
        })
    }

    // After index blockchain given a height_to_index it returns the following index to index
    pub fn index_height(&self, height_to_index: &BlockHeight) -> Result<BlockHeight> {
        // Get new block at height_to_sync
        //   Check if new block prev hash is correct
        //     If not, there is reorg
        //       We go back to the previous blcok, then height_to_sync is height_to_sync - 1
        // Save new block
        // Increment height_to_sync
        let blockchain_height = self.bitcoin_client.get_best_block()? as BlockHeight;

        let block = self.bitcoin_client.get_block_by_height(height_to_index)?;

        if block.is_none() {
            //Block does not exist in blockchain.
            return Ok(0);
        }

        let block = block.unwrap();
        let prev_height = height_to_index.saturating_sub(1);
        let prev_block_hash = self.store.get_block_hash_by_height(prev_height)?;

        // Is Genesis block or a checkpoint block
        if *height_to_index == 0
            || prev_block_hash.is_none()
            || block.prev_hash == prev_block_hash.unwrap()
        {
            if prev_block_hash.is_none() {
                warn!("Block height not found. Then could be a checkpoint block",);
                // We don't need to check prev block because does not exist.
            }

            info!(
                "New block at height {}H/{}H",
                height_to_index, blockchain_height
            );

            self.store.save_block(&block)?;

            return Ok(height_to_index + 1);
        }

        // if current block prev_hash is different than the previous block hash, then we need to reorg
        error!(
            "Block height mismatch. Block at height {}H is not matching prev_hash {:?}",
            height_to_index,
            prev_block_hash.unwrap()
        );

        Ok(height_to_index - 1)
    }
}

#[automock]
impl IndexerApi for Indexer {
    fn get_best_block(&self) -> Result<Option<BlockHeight>> {
        let block = self.store.get_best_block_height()?;
        Ok(block)
    }

    fn tx_exists(&self, tx_id: &Txid) -> Result<bool> {
        let exist = self.store.tx_exists(tx_id)?;
        Ok(exist)
    }

    fn get_tx(&self, tx_id: &Txid) -> Result<String> {
        let tx = self.bitcoin_client.get_tx_hex(tx_id)?;
        Ok(tx)
    }
}
