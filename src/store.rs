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
enum StoreKey {
    BlockByHash(BlockHash),
    BlockByHeight(BlockHeight),
    TransactionById(Txid),
    BlockTxsByHash(BlockHash),
    BestBlock,
}

impl Store {
    pub fn new(file_path: &str) -> Result<Self> {
        let db = Storage::new_with_path(&PathBuf::from(format!("{}/indexer", file_path)))?;
        Ok(Self { db })
    }

    fn get_key(&self, key: StoreKey) -> String {
        match key {
            StoreKey::BlockByHash(block_hash) => format!("block/hash/{}", block_hash),
            StoreKey::BlockByHeight(block_height) => {
                format!("block/height/{}", block_height)
            }
            StoreKey::TransactionById(tx_id) => format!("block/tx/{}", tx_id),
            StoreKey::BlockTxsByHash(block_hash) => format!("block/{}/txs", block_hash),
            StoreKey::BestBlock => "meta/best_block_height".to_string(),
        }
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
            let key = self.get_key(StoreKey::BlockByHash(block.hash));
            self.db.set(key, block)?;
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

        let block_key = self.get_key(StoreKey::BlockByHash(block.hash));

        self.db.set(block_key, new_block)?;
        // 2. Save the block hash by its height. This operation updates the best block at each height,
        // ensuring that all best blocks at a given height are stored.
        let height_key = self.get_key(StoreKey::BlockByHeight(block.height));
        self.db.set(height_key, block.hash)?;

        // 3. Save block hash under each transaction ID (this is to know if tx exists).
        for tx_id in &block.txs {
            let tx_key = self.get_key(StoreKey::TransactionById(*tx_id));
            self.db.set(tx_key, block.hash)?;
        }

        // 4. Save transactions by block hash.
        let txs_key = self.get_key(StoreKey::BlockTxsByHash(block.hash));
        self.db.set(txs_key, &block.txs)?;

        // 5. Update the best block height if this is the latest block.
        let key = self.get_key(StoreKey::BestBlock);
        let best_block_height: Option<BlockHeight> = self.db.get(key.clone())?;
        if best_block_height.is_none() || best_block_height.unwrap() < block.height {
            self.db.set(key, block.height)?;
        }

        Ok(())
    }

    // Retrieve the height of the best block.
    fn get_best_block_height(&self) -> Result<Option<BlockHeight>> {
        let key = self.get_key(StoreKey::BestBlock);
        let best_block_height: Option<BlockHeight> = self.db.get(key)?;
        Ok(best_block_height)
    }

    // Retrieve the block hash by height.
    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>> {
        let key = self.get_key(StoreKey::BlockByHeight(height));
        let block_hash: Option<BlockHash> = self.db.get(key)?;
        Ok(block_hash)
    }

    // Retrieve the block by its hash.
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>> {
        let key = self.get_key(StoreKey::BlockByHash(*hash));
        let block: Option<Block> = self.db.get(key)?;
        Ok(block)
    }

    // Check if a transaction exists and return its block height if found.
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>> {
        let key = self.get_key(StoreKey::TransactionById(*tx_id));
        let tx_info = self.db.get::<&str, BlockHash>(&key)?;

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
