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
        error!( "Block height mismatch. Expected block at height {}H with hash {}, but got block at height {}H with hash {:?}",
                        self.height_to_sync,
                        block.hash,
                        prev_height,
                        prev_block_hash.unwrap()
                    );

        self.height_to_sync -= 1;

        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::str::FromStr;

    use bitcoin::{BlockHash, Txid};
    use mockall::predicate::eq;

    use crate::bitcoin_client::MockBitcoinClient;
    use crate::store::MockStore;
    use crate::types::BlockInfo;

    use super::*;

    #[test]
    fn reorg_1_block() -> Result<(), anyhow::Error> {
        let mut bitcoin_client = MockBitcoinClient::new();
        let mut store = MockStore::new();

        let txid =
            Txid::from_str(&"91c1acedb27109016bb3a177372cdbb5f8f9d9c32fd4c2506ebb564ac0a61eaf")
                .unwrap();

        // Reorg 1 block, block_1002 prev hash is different than block_1001 hash
        // Then get again block_1001 and this is different, check that block_1001 prev hash is correct
        // Thne get again block_1002

        // block_1000 -> block_1001 -- reorg -- block_1002 -> block_1001 -> block_1002

        let hash_1000 = BlockHash::from_str(
            "12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d",
        )?;

        let hash_1001 = BlockHash::from_str(
            "e987bd2b973073b86b83901b03f6d16711452ab634cd8b2f3915e22cdcfa39b2",
        )?;

        let hash_1002 = BlockHash::from_str(
            "3c4389fd5a12aa686b546bf5ab2168e6149e21a6a20fcf9272ebc541bd2eed67",
        )?;

        let prev_hash_1000 = BlockHash::from_str(
            "4c136e0b24dc517809eabb6b6e6d5ec8f0087a49356be1f2de485d45ab26d2e3",
        )?;

        let hash_1001_reorg = BlockHash::from_str(
            "3aee099f9f5102e52767d9289b7a628e61d911d2f74f42c8835006c45d331713",
        )?;

        let block_1000 = BlockInfo {
            height: 1000,
            hash: hash_1000,
            prev_hash: prev_hash_1000,
            txs: vec![txid],
        };

        let block_1001 = BlockInfo {
            height: 1001,
            hash: hash_1001,
            prev_hash: hash_1000,
            txs: vec![txid],
        };

        let block_1002 = BlockInfo {
            height: 1002,
            hash: hash_1002,
            prev_hash: hash_1001_reorg,
            txs: vec![txid],
        };

        let block_1001_reorg = BlockInfo {
            height: 1001,
            hash: hash_1001_reorg,
            prev_hash: hash_1000,
            txs: vec![txid],
        };

        let block_1000_copy = block_1000.clone();
        let block_1001_copy = block_1001.clone();
        let block_1001_reorg_copy = block_1001_reorg.clone();

        bitcoin_client
            .expect_get_best_block()
            .times(4)
            .returning(|| Ok(10000000));

        //Detecting block 1000:
        bitcoin_client
            .expect_get_block_by_height()
            .times(1)
            .with(eq(1000))
            .returning(move |_| Ok(Some(block_1000.clone())));

        store
            .expect_get_block_hash_by_height()
            .with(eq(999))
            .times(1)
            .returning(move |_| Ok(Some(prev_hash_1000.clone())));

        store
            .expect_save_block()
            .with(eq(block_1000_copy.clone()))
            .times(1)
            .returning(move |_| Ok(()));

        //Detecting block 1001:
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(1001))
            .times(1)
            .returning(move |_| Ok(Some(block_1001.clone())));

        store
            .expect_get_block_hash_by_height()
            .with(eq(1000))
            .times(1)
            .returning(move |_| Ok(Some(hash_1000.clone())));

        store
            .expect_save_block()
            .with(eq(block_1001_copy))
            .times(1)
            .returning(move |_| Ok(()));

        //Detecting block 1002 and decrease one block:
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(1002))
            .times(1)
            .returning({
                let block_1002 = block_1002.clone();
                move |_| Ok(Some(block_1002.clone()))
            });

        store
            .expect_get_block_hash_by_height()
            .with(eq(1001))
            .times(1)
            .returning(move |_| Ok(Some(hash_1001.clone())));

        store.expect_save_block().never();

        //Going black to block 1001:
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(1001))
            .times(1)
            .returning(move |_| Ok(Some(block_1001_reorg.clone())));

        store
            .expect_get_block_hash_by_height()
            .with(eq(1000))
            .times(1)
            .returning(move |_| Ok(Some(hash_1000.clone())));

        store
            .expect_save_block()
            .with(eq(block_1001_reorg_copy))
            .times(1)
            .returning(move |_| Ok(()));

        let height_to_sync = 1000;
        let mut indexer = Indexer {
            height_to_sync,
            bitcoin_client: Box::new(bitcoin_client),
            store: Box::new(store),
        };

        // After initialize indexer should have height_to_sync in 1000
        assert_eq!(indexer.height_to_sync, height_to_sync);

        // Firt iteration should detect block 1000 and increment height_to_sync to 1001
        let _ = indexer.sync();
        assert_eq!(indexer.height_to_sync, 1001);

        // Second iteration should detect block 1001 and increment height_to_sync to 1002
        let _ = indexer.sync();
        assert_eq!(indexer.height_to_sync, 1002);

        // Third iteration should detect block 1002 and decrease height_to_sync to 1001
        let _ = indexer.sync();
        assert_eq!(indexer.height_to_sync, 1001);

        // Fourth iteration should detect block 1002 and increase height_to_sync to 1002
        let _ = indexer.sync();
        assert_eq!(indexer.height_to_sync, 1002);

        Ok(())
    }
}
