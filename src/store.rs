use crate::types::Block;
use crate::types::BlockHeight;
use crate::types::BlockInfo;
use anyhow::Ok;
use anyhow::Result;
use bitcoin::hash_types::BlockHash;
use bitcoin::Txid;
use mockall::automock;
use rust_bitvmx_storage_backend::storage::KeyValueStore;
use rust_bitvmx_storage_backend::storage::Storage;
use std::path::PathBuf;

pub struct Store {
    db: Storage,
}

impl Store {
    pub fn new(file_path: &str) -> Result<Self> {
        let db = Storage::new_with_path(&PathBuf::from(file_path))?;
        Ok(Self { db })
    }
}

pub trait StoreClient {
    fn get_best_block_height(&self) -> Result<Option<BlockHeight>>;
    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>>;
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>>;
    fn save_block(&self, block: &BlockInfo) -> Result<()>;
    fn tx_exists(&self, tx_id: &Txid) -> Result<(bool, Option<BlockHeight>)>;
}

#[automock]
impl StoreClient for Store {
    fn save_block(&self, block: &BlockInfo) -> Result<()> {
        // 1. Save the block itself under its hash.
        let block_key = format!("block/hash/{}", block.hash);
        let new_block = Block {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs.clone(),
            orphan: false,
        };
        self.db.set(block_key, &new_block)?;

        // 2. Save the block hash by its height.
        let height_key = format!("block/height/{}", block.height);
        self.db.set(height_key, block.hash)?;

        // 3. Save block hash under each transaction ID (this is to know if tx exists).
        for tx in &block.txs {
            let tx_key = format!("block/tx/{}", tx);
            self.db.set(tx_key, (block.hash, block.height))?;
        }

        // 4. Save transactions by block hash.
        let txs_key = format!("block/{}/txs", block.hash);
        self.db.set(txs_key, &block.txs)?;

        // 5. Update the best block height if this is the latest block.
        let best_block_height: Option<BlockHeight> = self.db.get("meta/best_block_height")?;
        if best_block_height.is_none() || best_block_height.unwrap() < block.height {
            self.db.set("meta/best_block_height", block.height)?;
        }

        Ok(())
    }

    // Retrieve the height of the best block.
    fn get_best_block_height(&self) -> Result<Option<BlockHeight>> {
        let best_block_height: Option<BlockHeight> = self.db.get("meta/best_block_height")?;
        Ok(best_block_height)
    }

    // Retrieve the block hash by height.
    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>> {
        let height_key = format!("block/height/{}", height);
        let block_hash: Option<BlockHash> = self.db.get(height_key)?;
        Ok(block_hash)
    }

    // Retrieve the block by its hash.
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>> {
        let block_key = format!("block/hash/{}", hash);
        let block: Option<Block> = self.db.get(block_key)?;
        Ok(block)
    }

    // Check if a transaction exists and return its block height if found.
    fn tx_exists(&self, tx_id: &Txid) -> Result<(bool, Option<BlockHeight>)> {
        let tx_key = format!("block/tx/{}", tx_id);
        let block_data = self.db.get::<&str, (BlockHash, BlockHeight)>(&tx_key)?;

        if let Some(block_info) = block_data {
            Ok((true, Some(block_info.1)))
        } else {
            Ok((false, None))
        }
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    #[ignore]
    fn get_data() -> Result<(), anyhow::Error> {
        //This is not a test, is just a way to call methods easily.
        let store = Store::new("data")?;
        let height = store.get_best_block_height()?;
        println!("best block {:?}", height);

        let block_hash = store.get_block_hash_by_height(7000)?.unwrap();
        println!("block hash {:?}", block_hash);

        let block = store.get_block_by_hash(&block_hash)?;

        println!("block hash {:?}", block);

        let txid =
            Txid::from_str(&"91c1acedb27109016bb3a177372cdbb5f8f9d9c32fd4c2506ebb564ac0a61eaf")
                .unwrap();

        let block_3 = BlockInfo {
            height: 4,
            hash: block_hash,
            prev_hash: block_hash,
            txs: vec![txid],
        };

        store.save_block(&block_3)?;

        let height = store.get_best_block_height()?;
        println!("best block {:?}", height);

        let block_hash = store.get_block_hash_by_height(4)?;
        println!("block hash {:?}", block_hash);

        let block_hash =
            BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f")
                .unwrap();
        let block = store.get_block_by_hash(&block_hash)?;

        println!("block {:?}", block);

        let tx_height = store.tx_exists(&txid)?;

        println!("tx exist at height {:?}", tx_height);

        Ok(())
    }
}
