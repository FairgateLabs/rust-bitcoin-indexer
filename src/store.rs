use std::rc::Rc;

use crate::errors::IndexerStoreError;
use crate::types::BlockHeight;
use crate::types::BlockInfo;
use crate::types::FullBlock;
use crate::types::TransactionInfo;
use bitcoin::hash_types::BlockHash;
use bitcoin::Transaction;
use bitcoin::Txid;
use mockall::automock;
use storage_backend::storage::KeyValueStore;
use storage_backend::storage::Storage;
pub struct IndexerStore {
    store: Rc<Storage>,
}

enum StoreKey {
    BlockByHash(BlockHash),
    BlockByHeight(BlockHeight),
    TransactionById(Txid),
    BlockTxsByHash(BlockHash),
    BestBlock,
}

impl IndexerStore {
    pub fn new(store: Rc<Storage>) -> Result<Self, IndexerStoreError> {
        Ok(Self { store })
    }

    fn get_key(&self, key: StoreKey) -> String {
        let prefix = "indexer";
        match key {
            StoreKey::BlockByHash(block_hash) => format!("{prefix}/block/hash/{block_hash}"),
            StoreKey::BlockByHeight(block_height) => {
                format!("{prefix}/block/height/{block_height}")
            }
            StoreKey::TransactionById(tx_id) => format!("{prefix}/block/tx/{tx_id}"),
            StoreKey::BlockTxsByHash(block_hash) => format!("{prefix}/block/{block_hash}/txs"),
            StoreKey::BestBlock => format!("{prefix}/meta/best_block_height"),
        }
    }
}

pub trait StoreClient {
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerStoreError>;
    fn get_block_hash_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHash>, IndexerStoreError>;
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<FullBlock>, IndexerStoreError>;
    fn save_block(&self, block: &BlockInfo) -> Result<(), IndexerStoreError>;
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerStoreError>;
}

#[automock]
impl StoreClient for IndexerStore {
    fn save_block(&self, block: &BlockInfo) -> Result<(), IndexerStoreError> {
        let existing_block_at_height = self.get_block_hash_by_height(block.height)?;

        if let Some(block_hash) = existing_block_at_height {
            // update block as an orphan.
            let mut block = match self.get_block_by_hash(&block_hash)? {
                Some(block) => block,
                None => return Err(IndexerStoreError::BlockNotFound),
            };

            block.orphan = true;

            // save block
            let key = self.get_key(StoreKey::BlockByHash(block.hash));
            self.store.set(key, block, None)?;
        }

        //Create new entry for the new block
        let new_block = FullBlock {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs.clone(),
            orphan: false,
        };

        // 1. Save the block itself under its hash.

        let block_key = self.get_key(StoreKey::BlockByHash(block.hash));

        self.store.set(block_key, new_block, None)?;
        // 2. Save the block hash by its height. This operation updates the best block at each height,
        // ensuring that all best blocks at a given height are stored.
        let height_key = self.get_key(StoreKey::BlockByHeight(block.height));
        self.store.set(height_key, block.hash, None)?;

        // 3. Save block hash under each transaction ID (this is to know if tx exists).
        for tx in &block.txs {
            let tx_key = self.get_key(StoreKey::TransactionById(tx.compute_txid()));
            self.store.set(tx_key, (tx, block.hash), None)?;
        }

        // 4. Save transactions IDs by block hash.
        let txs_key = self.get_key(StoreKey::BlockTxsByHash(block.hash));
        self.store.set(txs_key, &block.txs, None)?;

        // 5. Update the best block height if this is the latest block.
        let key = self.get_key(StoreKey::BestBlock);
        let best_block_height: Option<BlockHeight> = self.store.get(key.clone())?;

        if best_block_height.is_none() || best_block_height < Some(block.height) {
            self.store.set(key, block.height, None)?;
        }
        Ok(())
    }

    // Retrieve the height of the best block.
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerStoreError> {
        let key = self.get_key(StoreKey::BestBlock);
        let best_block_height: Option<BlockHeight> = self.store.get(key)?;

        if let Some(height) = best_block_height {
            match self.get_block_hash_by_height(height)? {
                Some(block_hash) => match self.get_block_by_hash(&block_hash)? {
                    Some(block) => Ok(Some(block)),
                    None => Err(IndexerStoreError::BlockNotFound),
                },
                None => Err(IndexerStoreError::BlockNotFound),
            }
        } else {
            Ok(None)
        }
    }

    // Retrieve the block hash by height.
    fn get_block_hash_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<BlockHash>, IndexerStoreError> {
        let key = self.get_key(StoreKey::BlockByHeight(height));
        let block_hash: Option<BlockHash> = self.store.get(key)?;
        Ok(block_hash)
    }

    // Retrieve the block by its hash.
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<FullBlock>, IndexerStoreError> {
        let key = self.get_key(StoreKey::BlockByHash(*hash));
        let block: Option<FullBlock> = self.store.get(key)?;
        Ok(block)
    }

    // Check if a transaction exists and return its block height if found.
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerStoreError> {
        let key = self.get_key(StoreKey::TransactionById(*tx_id));
        let tx_data = self.store.get::<&str, (Transaction, BlockHash)>(&key)?;

        if let Some((tx, block_hash)) = tx_data {
            let block = match self.get_block_by_hash(&block_hash)? {
                Some(block) => block,
                None => return Err(IndexerStoreError::BlockNotFound),
            };

            let tx = TransactionInfo {
                tx,
                block_height: block.height,
                block_hash,
                orphan: block.orphan,
                confirmations: 0,
            };
            Ok(Some(tx))
        } else {
            Ok(None)
        }
    }
}
