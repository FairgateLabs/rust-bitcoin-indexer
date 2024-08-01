use crate::{bitcoin_client::BitcoinClientApi, store::StoreClient, types::BlockHeight};
use anyhow::Result;
use log::{error, info, warn};
use std::{thread::sleep, time::Duration};
pub struct Indexer {
    pub height_to_sync: BlockHeight,
    pub bitcoin_client: Box<dyn BitcoinClientApi>,
    pub store: Box<dyn StoreClient>,
}

impl Indexer {
    pub fn new(
        bitcoin_client: Box<dyn BitcoinClientApi>,
        store: Box<dyn StoreClient>,
        height_to_sync: BlockHeight,
    ) -> Result<Self> {
        Ok(Self {
            height_to_sync,
            bitcoin_client,
            store,
        })
    }

    pub fn sync(&mut self) -> Result<()> {
        // Get new block at height_to_sync
        //   Check if new block prev hash is correct
        //     If not, there is reorg
        //       We go back to the previous blcok, then height_to_sync is height_to_sync - 1
        // Save new block
        // Increment height_to_sync
        let blockchain_height = self.bitcoin_client.get_best_block()? as BlockHeight;

        let block = self
            .bitcoin_client
            .get_block_by_height(self.height_to_sync)?;

        if block.is_none() {
            //run a thread sleep for 2 minutes.
            info!("Waiting for new block...");
            sleep(Duration::from_secs(120));
            return Ok(());
        }

        let block = block.unwrap();
        let prev_height = self.height_to_sync.saturating_sub(1);
        let prev_block_hash = self.store.get_block_hash_by_height(prev_height)?;

        // Is Genesis block or a checkpoint block
        if self.height_to_sync == 0
            || prev_block_hash.is_none()
            || block.prev_hash == prev_block_hash.unwrap()
        {
            if prev_block_hash.is_none() {
                warn!("Block height not found. Then could be a checkpoint block",);
                // Then we don't need to check prev block because does not exist.
            }

            info!(
                "New block at height {}H/{}H",
                self.height_to_sync, blockchain_height
            );

            self.store.save_block(&block)?;

            self.height_to_sync += 1;

            return Ok(());
        }

        // if current block prev_hash is different than the previous block hash, then we need to reorg
        error!(
            "Block height mismatch. Block at height {}H is not matching prev_hash {:?}",
            self.height_to_sync,
            prev_block_hash.unwrap()
        );

        self.height_to_sync -= 1;

        Ok(())
    }
}
