use std::rc::Rc;

use crate::errors::IndexerStoreError;
use crate::types::{FullBlock, TransactionInfo, TransactionStatus};
use bitcoin::hash_types::BlockHash;
use bitcoin::Transaction;
use bitcoin::Txid;
use bitvmx_bitcoin_rpc::types::{BlockHeight, BlockInfo};
use mockall::automock;
use storage_backend::storage::KeyValueStore;
use storage_backend::storage::Storage;
use tracing::warn;
pub struct IndexerStore {
    store: Rc<Storage>,
    confirmation_threshold: u32,
}

enum StoreKey {
    BlockByHash(BlockHash),
    BlockByHeight(BlockHeight),
    TransactionById(Txid),
    BlockTxsByHash(BlockHash),
    BestBlock,
    CheckpointHeight,
}

impl IndexerStore {
    pub fn new(store: Rc<Storage>, confirmation_threshold: u32) -> Result<Self, IndexerStoreError> {
        Ok(Self {
            store,
            confirmation_threshold,
        })
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
            StoreKey::CheckpointHeight => format!("{prefix}/meta/checkpoint_height"),
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
    fn get_block_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<FullBlock>, IndexerStoreError>;
    fn save_new_best_block(
        &self,
        block: &BlockInfo,
        estimated_fee_rate: u64,
    ) -> Result<(), IndexerStoreError>;
    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerStoreError>;

    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerStoreError>;
    fn save_best_height(&self, height: BlockHeight) -> Result<(), IndexerStoreError>;
    fn mark_following_blocks_as_orphan(&self, height: BlockHeight)
        -> Result<(), IndexerStoreError>;
    fn get_checkpoint_height(&self) -> Result<Option<BlockHeight>, IndexerStoreError>;
    fn save_checkpoint_height(&self, height: BlockHeight) -> Result<(), IndexerStoreError>;
}

#[automock]
impl StoreClient for IndexerStore {
    fn save_new_best_block(
        &self,
        block: &BlockInfo,
        estimated_fee_rate: u64,
    ) -> Result<(), IndexerStoreError> {
        let existing_block_at_height = self.get_block_hash_by_height(block.height)?;

        if let Some(block_hash) = existing_block_at_height {
            // update block as an orphan.
            let mut saved_block = match self.get_block_by_hash(&block_hash)? {
                Some(block) => block,
                None => return Err(IndexerStoreError::BlockNotFound),
            };

            if saved_block.hash == block.hash {
                warn!("Block already saved at height {}", block.height);
                return Ok(());
            }

            saved_block.orphan = true;

            // save previous block as an orphan.
            let key = self.get_key(StoreKey::BlockByHash(saved_block.hash));
            self.store.set(key, saved_block, None)?;
        }

        //Create new entry for the new block
        let new_block = FullBlock {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs.clone(),
            orphan: false,
            estimated_fee_rate,
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
        self.save_best_height(block.height)?;

        Ok(())
    }

    // Retrieve the height of the best block.
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerStoreError> {
        let best_block_height = self.get_best_height()?;

        if best_block_height.is_none() {
            return Ok(None);
        }

        match self.get_block_hash_by_height(best_block_height.unwrap())? {
            Some(block_hash) => match self.get_block_by_hash(&block_hash)? {
                Some(block) => Ok(Some(block)),
                None => Err(IndexerStoreError::BlockNotFound),
            },
            None => Err(IndexerStoreError::BlockNotFound),
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

    fn get_tx_info(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerStoreError> {
        let key = self.get_key(StoreKey::TransactionById(*tx_id));
        let tx_data = self.store.get::<&str, (Transaction, BlockHash)>(&key)?;

        if let Some((tx, block_hash)) = tx_data {
            let block_info = match self.get_block_by_hash(&block_hash)? {
                Some(block) => block,
                None => return Err(IndexerStoreError::BlockNotFound),
            };

            let best_block_height = self.get_best_height()?.unwrap_or(0);

            let mut confirmations = best_block_height
                .saturating_sub(block_info.height)
                .saturating_add(1);

            // If the block is orphaned or its height is greater than the best block height,
            // this indicates a reorg or block invalidation where the blockchain has reverted.
            let status = if block_info.orphan || block_info.height > best_block_height {
                confirmations = 0;
                TransactionStatus::Orphan
            } else {
                TransactionStatus::Confirmed // Will be updated in indexer based on confirmations and threshold
            };

            Ok(Some(TransactionInfo {
                tx: Some(tx),
                block_info: Some(block_info),
                confirmations,
                status,
                confirmation_threshold: self.confirmation_threshold, // Will be updated in indexer with actual threshold
            }))
        } else {
            Ok(None)
        }
    }

    fn get_block_by_height(
        &self,
        height: BlockHeight,
    ) -> Result<Option<FullBlock>, IndexerStoreError> {
        let hash = self.get_block_hash_by_height(height)?;

        if let Some(hash) = hash {
            let block = self.get_block_by_hash(&hash)?;
            Ok(block)
        } else {
            Ok(None)
        }
    }

    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerStoreError> {
        let key = self.get_key(StoreKey::BestBlock);
        let height: Option<BlockHeight> = self.store.get(key)?;
        Ok(height)
    }

    fn save_best_height(&self, height: BlockHeight) -> Result<(), IndexerStoreError> {
        let key = self.get_key(StoreKey::BestBlock);
        self.store.set(key, height, None)?;
        Ok(())
    }

    fn mark_following_blocks_as_orphan(
        &self,
        start_height_to_mark: BlockHeight,
    ) -> Result<(), IndexerStoreError> {
        let best_height = self.get_best_height()?.unwrap_or(0);

        let mut current_height = start_height_to_mark;

        while current_height <= best_height {
            if let Some(mut block) = self.get_block_by_height(current_height)? {
                block.orphan = true;

                println!("marking block as orphan:");

                let block_key = self.get_key(StoreKey::BlockByHash(block.hash));
                self.store.set(block_key, block, None)?;
            }

            current_height += 1;
        }
        Ok(())
    }

    fn get_checkpoint_height(&self) -> Result<Option<BlockHeight>, IndexerStoreError> {
        let key = self.get_key(StoreKey::CheckpointHeight);
        let height = self.store.get(key)?;
        Ok(height)
    }

    fn save_checkpoint_height(&self, height: BlockHeight) -> Result<(), IndexerStoreError> {
        let key = self.get_key(StoreKey::CheckpointHeight);
        self.store.set(key, height, None)?;
        Ok(())
    }
}
