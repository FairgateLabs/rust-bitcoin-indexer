use crate::types::Block;
use crate::types::BlockHeight;
use crate::types::BlockInfo;
use crate::types::TransactionInfo;
use anyhow::Ok;
use anyhow::Result;
use bitcoin::hash_types::BlockHash;
use bitcoin::Txid;
use mockall::automock;
use std::path::PathBuf;
use storage_backend::storage::KeyValueStore;
use storage_backend::storage::Storage;
pub struct Store {
    db: Storage,
}

impl Store {
    pub fn new(file_path: &str) -> Result<Self> {
        let db = Storage::new_with_path(&PathBuf::from(format!("{}/indexer", file_path)))?;
        Ok(Self { db })
    }
}

pub trait StoreClient {
    fn get_best_block_height(&self) -> Result<Option<BlockHeight>>;
    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>>;
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>>;
    fn save_block(&self, block: &BlockInfo) -> Result<()>;
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>>;
}

#[automock]
impl StoreClient for Store {
    fn save_block(&self, block: &BlockInfo) -> Result<()> {
        let existing_block_at_height = self.get_block_hash_by_height(block.height)?;

        if existing_block_at_height.is_some() {
            // update block as an orphan.
            let block_hash = existing_block_at_height.unwrap();
            let mut block = self.get_block_by_hash(&block_hash)?.unwrap();
            block.orphan = true;

            // save block
            let block_key = format!("block/hash/{}", block.hash);
            self.db.set(block_key, block)?;
        }

        //Create new entry for the new block
        let new_block = Block {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs.clone(),
            orphan: false,
        };

        // 1. Save the block itself under its hash.

        let block_key = format!("block/hash/{}", block.hash);

        self.db.set(block_key, new_block)?;
        // 2. Save the block hash by its height. This operation updates the best block at each height,
        // ensuring that all best blocks at a given height are stored.
        let height_key = format!("block/height/{}", block.height);
        self.db.set(height_key, block.hash)?;

        // 3. Save block hash under each transaction ID (this is to know if tx exists).
        for tx in &block.txs {
            let tx_key = format!("block/tx/{}", tx);
            self.db.set(tx_key, block.hash)?;
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
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>> {
        let tx_key = format!("block/tx/{}", tx_id);
        let tx_info = self.db.get::<&str, BlockHash>(&tx_key)?;

        if let Some(block_info) = tx_info {
            let block = self.get_block_by_hash(&block_info)?.unwrap();

            let tx = TransactionInfo {
                tx_id: *tx_id,
                block_height: block.height,
                block_hash: block_info,
                orphan: block.orphan,
            };
            Ok(Some(tx))
        } else {
            Ok(None)
        }
    }
}
