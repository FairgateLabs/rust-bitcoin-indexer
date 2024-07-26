use std::{io::stdout, thread::sleep, time::Duration};

use crate::{
    bitcoin_client::BitcoinClientApi,
    store::StoreClient,
    types::{BlockHeight, BlockInfo},
};
use anyhow::{bail, Result};
use log::{error, info, warn};
use std::io::{self, Write};
pub struct Indexer {
    pub checkpoint_height: Option<BlockHeight>,
    height_to_sync: BlockHeight,
    pub bitcoin_client: Box<dyn BitcoinClientApi>,
    pub store: Box<dyn StoreClient>,
}

impl Indexer {
    pub fn new(
        bitcoin_client: Box<dyn BitcoinClientApi>,
        store: Box<dyn StoreClient>,
        checkpoint_height: Option<BlockHeight>,
    ) -> Result<Self> {
        Ok(Self {
            height_to_sync: 0,
            checkpoint_height,
            bitcoin_client,
            store,
        })
    }

    pub fn define_height_to_sync(&mut self, blockchain_height: BlockHeight) -> Result<BlockHeight> {
        // blockchain_height: The current block height of the Bitcoin network.
        // checkpoint_height: The starting block height for synchronization.
        // indexed_height: The highest block height that has already been synchronized and stored in the storage.

        let indexed_height = self.store.get_best_block_height()?;

        if indexed_height.is_some() {
            info!("Last indexed block is {:?}H", indexed_height.unwrap());
        } else {
            info!("No block indexed");
        }

        let mut height_to_sync: u32 = indexed_height.unwrap_or(0);

        if self.checkpoint_height.is_some() {
            let checkpoint = self.checkpoint_height.unwrap();

            if checkpoint < height_to_sync {
                warn!("Passed CHECKPOINT_HEIGHT command line is behind last indexed height");
            }

            info!("Using CHECKPOINT_HEIGHT={}H to start to sync", checkpoint);

            height_to_sync = checkpoint;
        }

        // ERROR if blockchain_height < start_height
        if blockchain_height < height_to_sync {
            let error =  "The current block height of the Bitcoin network is behind the starting block to sync";
            error!("{}", error);
            bail!(error);
        }

        if height_to_sync > 0 && self.checkpoint_height.is_none() {
            height_to_sync += 1
        }

        Ok(height_to_sync)
    }

    pub fn run(&mut self) -> Result<()> {
        let blockchain_height = self.bitcoin_client.get_best_block()? as BlockHeight;
        let network = self.bitcoin_client.get_blockchain_info()?;

        info!("Connected to chain {}", network);
        info!("Chain best block at {}H", blockchain_height);
        self.height_to_sync = self.define_height_to_sync(blockchain_height)?;
        info!("Start synchronizing from {}H", self.height_to_sync);

        loop {
            // Get new block at height_to_sync
            //   Check if new block prev hash is correct
            //     If not, there is reorg
            //       We go back to the previous blcok, then height_to_sync is height_to_sync - 1
            // Save new block
            // Increment height_to_sync

            let block = self
                .bitcoin_client
                .get_block_by_height(self.height_to_sync)?;

            if block.is_none() {
                //run a thread sleep for 2 minutes.
                info!("Waiting for new block...");
                sleep(Duration::from_secs(120));
                continue;
            }

            let block = block.unwrap();
            let prev_height = self.height_to_sync.saturating_sub(1);
            let prev_block_hash = self.bitcoin_client.get_block_id_by_height(prev_height)?;

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
                continue;
            }

            // if current block prev_hash is different than the previous block hash, then we need to reorg
            error!( "Block height mismatch. Expected block at height {}H with hash {}, but got block at height {}H with hash {:?}",
                        self.height_to_sync,
                        block.hash,
                        prev_height,
                        prev_block_hash.unwrap()
                    );

            self.height_to_sync -= 1;
        }
    }
}

#[cfg(test)]
mod test {

    use crate::bitcoin_client::MockBitcoinClient;
    use crate::store::MockStore;

    use super::*;

    #[test]
    fn define_height_to_sync() -> Result<(), anyhow::Error> {
        // Tests:

        // Test
        // checkpoint_height: None | blockchain_height: 100 | indexed_height: None
        let mut indexer = set_up_indexer(None, None);
        let start_height = indexer.define_height_to_sync(100)?;
        // Then start_height should be height 0 (no checkpoint, no block indexed)
        assert_eq!(start_height, 0);

        // Test
        // checkpoint_height: None | blockchain_height: 100 | indexed_height: 40
        let mut indexer = set_up_indexer(None, Some(40));
        let start_height = indexer.define_height_to_sync(100)?;
        // Then start_height should be height 41 (indexed_height + 1 )
        assert_eq!(start_height, 41);

        // Test
        // checkpoint_height: 10000 | blockchain_height: 100 | indexed_height: None
        let mut indexer = set_up_indexer(Some(10000), None);
        let start_height = indexer.define_height_to_sync(100);
        // Checkpoint can not be bigger than blockchain_height
        assert!(start_height.is_err());

        // Test
        // checkpoint_height: 40 | blockchain_height: 100 | indexed_height: None
        let mut indexer = set_up_indexer(Some(40), None);
        let start_height = indexer.define_height_to_sync(100)?;
        // Then start_height should be height 40 (checkpoint_height should rule)
        assert_eq!(start_height, 40);

        // Test
        // checkpoint_height 100 | blockchain_height 100 | indexed_height 100
        let mut indexer = set_up_indexer(Some(100), Some(100));
        let start_height = indexer.define_height_to_sync(100)?;
        // Then start_height should be height 100 (checkpoint should rule)
        assert_eq!(start_height, 100);

        Ok(())
    }

    fn set_up_indexer(
        checkpoint_height: Option<BlockHeight>,
        indexed_height: Option<BlockHeight>,
    ) -> Indexer {
        let bitcoin_client = MockBitcoinClient::new();
        let mut store = MockStore::new();

        store
            .expect_get_best_block_height()
            .once()
            .returning(move || Ok(indexed_height));

        let indexer = Indexer {
            checkpoint_height,
            bitcoin_client: Box::new(bitcoin_client),
            store: Box::new(store),
            height_to_sync: 0,
        };

        indexer
    }
}
