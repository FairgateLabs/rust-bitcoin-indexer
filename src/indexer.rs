use crate::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi}, errors::IndexerError, store::{Store, StoreClient}, types::{BlockHeight, FullBlock, TransactionInfo}
};
use bitcoin::Txid;
use log::{info, warn};
use mockall::automock;
pub struct Indexer<B, S>
where
    B: BitcoinClientApi,
    S: StoreClient,
{
    pub bitcoin_client: B,
    pub store: S,
}

#[automock]
pub trait IndexerApi {
    // This method indexes the block height received as a parameter
    fn tick(&mut self, height_to_index: &BlockHeight) -> Result<BlockHeight, IndexerError>;
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError>;
    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError>;
}

impl<B, S> Indexer<B, S>
where
    B: BitcoinClientApi,
    S: StoreClient,
{
    pub fn new(bitcoin_indexer_client: B, store: S) -> Self {
        Self {
            bitcoin_client: bitcoin_indexer_client,
            store,
        }
    }
}

impl Indexer<BitcoinClient, Store> {
    pub fn new_with_path(bitcoin_client: BitcoinClient, store_path: &str) -> Result<Self, IndexerError> {
        let store = Store::new(store_path)?;
        Ok(Self {
            bitcoin_client,
            store,
        })
    }
}

#[automock]
impl<B, S> IndexerApi for Indexer<B, S>
where
    B: BitcoinClientApi,
    S: StoreClient,
{
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError> {
        let block = self.store.get_best_block()?;
        Ok(block)
    }

    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError> {
        let tx_info = self.store.get_tx_info(tx_id)?;

        if let Some(mut tx_info) = tx_info {
            let best_block = self.get_best_block()?;
            if let Some(best_block) = best_block {
                if !tx_info.orphan {
                    tx_info.confirmations = best_block.height - tx_info.block_height + 1;
                }
            }

            return Ok(Some(tx_info));
        }

        Ok(tx_info)
    }

    // After index blockchain given a height_to_index it returns the following index to index
    fn tick(&mut self, height_to_index: &BlockHeight) -> Result<BlockHeight, IndexerError> {
        // Get new block at height_to_sync
        //   Check if new block prev hash is correct
        //     If not, there is reorg
        //       We go back to the previous blcok, then height_to_sync is height_to_sync - 1
        // Save new block
        // Increment height_to_sync
        let blockchain_height = self.bitcoin_client.get_best_block()? as BlockHeight;

        if blockchain_height < *height_to_index {
            // If the current blockchain height is lower than height_to_sync and
            // the block to be indexed is beyond the range of blocks already indexed,
            // return the same height for indexing.
            return Ok(*height_to_index);
        }

        info!(
            "Indexing block at height {}H/{}H",
            height_to_index, blockchain_height
        );

        let block = self.bitcoin_client.get_block_by_height(height_to_index)?;

        let block = match block {
            Some(block) => block,
            None => {
                //Block does not exist in blockchain, then return same height.
                return Ok(*height_to_index);
            },
        };

        let prev_height = height_to_index.saturating_sub(1);
        let prev_block_hash = self.store.get_block_hash_by_height(prev_height)?;

        // Is Genesis block or a checkpoint block
        if *height_to_index == 0
            || prev_block_hash.is_none()
            || Some(block.prev_hash) == prev_block_hash
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
        warn!(
            "Block height mismatch. Block at height {}H is not matching prev_hash {:?}",
            height_to_index,
            prev_block_hash
        );

        Ok(height_to_index - 1)
    }
}
