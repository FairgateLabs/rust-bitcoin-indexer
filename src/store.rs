use std::rc::Rc;

use crate::errors::IndexerError;
use crate::types::FullBlock;
use bitcoin::{Transaction, Txid};
use bitvmx_bitcoin_rpc::types::BlockHeight;
use storage_backend::storage::KeyValueStore;
use storage_backend::storage::Storage;

/// One entry of the mempool watch list: a txid a consumer asked to follow, and where it stands.
/// `None` is pending, `Some(height)` is confirmed in the block the indexer holds at that height.
pub type MempoolWatchEntry = (Txid, Option<BlockHeight>);

pub struct IndexerStore {
    store: Rc<Storage>,
}

enum StoreKey {
    Block(BlockHeight), // The block indexed at this height. Deleted when it leaves the retention window, and when a reorg takes it off the chain.
    TxHeight(Txid), // Which block height holds a transaction. Written with its block and deleted with it.
    Cursor,         // Height of the highest indexed block, which is also the highest block stored.
    MempoolWatchList, // Txids a consumer asked to follow in the mempool, each with its confirmation height once mined.
    MempoolSnapshot, // Which watched transactions the node held in its mempool when the last tick ended.
}

impl IndexerStore {
    pub fn new(store: Rc<Storage>) -> Result<Self, IndexerError> {
        Ok(Self { store })
    }

    fn get_key(&self, key: StoreKey) -> String {
        let prefix = "indexer";
        match key {
            StoreKey::Block(height) => format!("{prefix}/block/{height}"),
            StoreKey::TxHeight(tx_id) => format!("{prefix}/tx/{tx_id}"),
            StoreKey::Cursor => format!("{prefix}/cursor"),
            StoreKey::MempoolWatchList => format!("{prefix}/mempool_watch_list"),
            StoreKey::MempoolSnapshot => format!("{prefix}/mempool_snapshot"),
        }
    }

    /// Stores a block and the height entry of each of its transactions.
    pub fn save_block(&self, block: &FullBlock) -> Result<(), IndexerError> {
        for tx in &block.txs {
            let key = self.get_key(StoreKey::TxHeight(tx.compute_txid()));
            self.store.set(key, block.height, None)?;
        }

        let key = self.get_key(StoreKey::Block(block.height));
        self.store.set(key, block, None)?;

        Ok(())
    }

    /// Deletes the block at this height together with the height entries of its transactions.
    pub fn delete_block(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let block = match self.get_block(height)? {
            Some(block) => block,
            None => return Ok(()),
        };

        for tx in &block.txs {
            let key = self.get_key(StoreKey::TxHeight(tx.compute_txid()));
            self.store.remove(key, None)?;
        }

        let key = self.get_key(StoreKey::Block(height));
        self.store.remove(key, None)?;

        Ok(())
    }

    /// Retrieves the block at this height, if the indexer still holds it.
    pub fn get_block(&self, height: BlockHeight) -> Result<Option<FullBlock>, IndexerError> {
        let key = self.get_key(StoreKey::Block(height));
        Ok(self.store.get(key, None)?)
    }

    /// Retrieves the block at this height. Fails with `BlockNotFound` when the indexer does not hold it.
    pub fn get_block_or_err(&self, height: BlockHeight) -> Result<FullBlock, IndexerError> {
        self.get_block(height)?
            .ok_or(IndexerError::BlockNotFound(height))
    }

    /// Height of the block that holds this transaction, if the indexer still holds that block.
    pub fn get_tx_height(&self, tx_id: &Txid) -> Result<Option<BlockHeight>, IndexerError> {
        let key = self.get_key(StoreKey::TxHeight(*tx_id));
        Ok(self.store.get(key, None)?)
    }

    /// The transaction and the block that holds it, if the indexer holds that block.
    pub fn get_indexed_tx(
        &self,
        tx_id: &Txid,
    ) -> Result<Option<(Transaction, FullBlock)>, IndexerError> {
        let height = match self.get_tx_height(tx_id)? {
            Some(height) => height,
            None => return Ok(None),
        };

        let block = self.get_block(height)?.ok_or_else(|| {
            IndexerError::InvariantViolation(format!(
                "transaction {tx_id} points at height {height}, where no block is stored"
            ))
        })?;

        let tx = block
            .txs
            .iter()
            .find(|tx| tx.compute_txid() == *tx_id)
            .cloned()
            .ok_or_else(|| {
                IndexerError::InvariantViolation(format!(
                    "transaction {tx_id} points at height {height}, whose block does not contain it"
                ))
            })?;

        Ok(Some((tx, block)))
    }

    /// Retrieves the height of the block at the cursor, if any.
    pub fn get_cursor(&self) -> Result<Option<BlockHeight>, IndexerError> {
        let key = self.get_key(StoreKey::Cursor);
        Ok(self.store.get(key, None)?)
    }

    /// Retrieves the height of the block at the cursor. Fails when the cursor was never saved.
    pub fn get_cursor_or_err(&self) -> Result<BlockHeight, IndexerError> {
        self.get_cursor()?.ok_or_else(|| {
            IndexerError::InvariantViolation("the cursor was never saved".to_string())
        })
    }

    /// Saves the height of the block at the cursor.
    pub fn save_cursor(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let key = self.get_key(StoreKey::Cursor);
        self.store.set(key, height, None)?;
        Ok(())
    }

    /// Retrieves the list of watched txids and their confirmation heights.
    pub fn get_mempool_watch_list(&self) -> Result<Vec<MempoolWatchEntry>, IndexerError> {
        let key = self.get_key(StoreKey::MempoolWatchList);
        Ok(self.store.get(key, None)?.unwrap_or_default())
    }

    /// Saves the list of watched txids and their confirmation heights.
    pub fn save_mempool_watch_list(
        &self,
        list: Vec<MempoolWatchEntry>,
    ) -> Result<(), IndexerError> {
        let key = self.get_key(StoreKey::MempoolWatchList);
        self.store.set(key, list, None)?;
        Ok(())
    }

    /// Adds a txid to the mempool watch list as pending.
    pub fn add_mempool_watch(&self, tx_id: Txid) -> Result<(), IndexerError> {
        let mut list = self.get_mempool_watch_list()?;

        // If the txid is already being watched, do not add it again.
        if list.iter().any(|(watched, _)| *watched == tx_id) {
            return Ok(());
        }

        list.push((tx_id, None));
        self.save_mempool_watch_list(list)
    }

    /// Removes a txid from the mempool watch list.
    pub fn remove_mempool_watch(&self, tx_id: &Txid) -> Result<(), IndexerError> {
        let mut list = self.get_mempool_watch_list()?;
        list.retain(|(watched, _)| watched != tx_id);
        self.save_mempool_watch_list(list)
    }

    /// Saves the list of txids the node held in its mempool when the last tick ended.
    pub fn save_mempool_snapshot(&self, tx_ids: Vec<Txid>) -> Result<(), IndexerError> {
        let key = self.get_key(StoreKey::MempoolSnapshot);
        self.store.set(key, tx_ids, None)?;
        Ok(())
    }

    /// Checks whether a txid is in the list of txids the node held in its mempool.
    pub fn is_in_mempool_snapshot(&self, tx_id: &Txid) -> Result<bool, IndexerError> {
        let key = self.get_key(StoreKey::MempoolSnapshot);
        let list: Vec<Txid> = self.store.get(key, None)?.unwrap_or_default();
        Ok(list.contains(tx_id))
    }
}

#[cfg(test)]
mod tests {
    use crate::errors::IndexerError;
    use crate::test_utils::{dummy_tx, full_block, temp_store};

    // Blocks are saved and deleted together with the height entries of their transactions.
    #[test]
    fn blocks() {
        let store = temp_store();
        let block = full_block(10, [1u8; 32], [0u8; 32], vec![dummy_tx(1), dummy_tx(2)]);

        // A read that must succeed fails with BlockNotFound before the block is saved.
        assert!(matches!(
            store.get_block_or_err(10),
            Err(IndexerError::BlockNotFound(10))
        ));

        // A saved block comes back, with a height entry for each of its transactions.
        store.save_block(&block).unwrap();
        assert_eq!(store.get_block(10).unwrap(), Some(block.clone()));
        assert_eq!(store.get_block_or_err(10).unwrap(), block);
        for tx in &block.txs {
            assert_eq!(store.get_tx_height(&tx.compute_txid()).unwrap(), Some(10));
        }

        // Deleting it deletes the height entries too.
        store.delete_block(10).unwrap();
        assert_eq!(store.get_block(10).unwrap(), None);
        for tx in &block.txs {
            assert_eq!(store.get_tx_height(&tx.compute_txid()).unwrap(), None);
        }

        // Deleting a height with no block is not an error.
        assert!(store.delete_block(42).is_ok());
    }

    // A cursor read that must succeed fails until the cursor is saved.
    #[test]
    fn cursor() {
        let store = temp_store();
        assert_eq!(store.get_cursor().unwrap(), None);
        assert!(matches!(
            store.get_cursor_or_err(),
            Err(IndexerError::InvariantViolation(_))
        ));

        store.save_cursor(7).unwrap();
        assert_eq!(store.get_cursor().unwrap(), Some(7));
        assert_eq!(store.get_cursor_or_err().unwrap(), 7);
    }

    // An indexed transaction comes back with the block that holds it, and follows the block that is kept after a reorg.
    #[test]
    fn indexed_tx() {
        let store = temp_store();
        let tx = dummy_tx(1);
        let block = full_block(10, [1u8; 32], [0u8; 32], vec![dummy_tx(2), tx.clone()]);
        store.save_block(&block).unwrap();

        // A held transaction, and one that was never indexed.
        assert_eq!(
            store.get_indexed_tx(&tx.compute_txid()).unwrap(),
            Some((tx.clone(), block))
        );
        assert_eq!(
            store.get_indexed_tx(&dummy_tx(3).compute_txid()).unwrap(),
            None
        );

        // The block that loses a reorg is deleted before the winning one is stored, whatever their heights.
        store.delete_block(10).unwrap();
        assert_eq!(store.get_indexed_tx(&tx.compute_txid()).unwrap(), None);

        let winner = full_block(11, [2u8; 32], [0u8; 32], vec![tx.clone()]);
        store.save_block(&winner).unwrap();
        assert_eq!(store.get_tx_height(&tx.compute_txid()).unwrap(), Some(11));

        // A height entry pointing where no block is stored is a contradiction, not an answer.
        let orphan_entry = dummy_tx(4);
        store
            .save_block(&full_block(12, [3u8; 32], [2u8; 32], vec![orphan_entry.clone()]))
            .unwrap();
        store
            .save_block(&full_block(12, [4u8; 32], [2u8; 32], vec![]))
            .unwrap();
        store.delete_block(12).unwrap();

        assert_eq!(
            store.get_tx_height(&orphan_entry.compute_txid()).unwrap(),
            Some(12)
        );
        assert!(matches!(
            store.get_indexed_tx(&orphan_entry.compute_txid()),
            Err(IndexerError::InvariantViolation(_))
        ));
    }

    // A mempool watch list can be added to, saved, and removed from.
    #[test]
    fn mempool_watch_list() {
        let store = temp_store();
        let first = dummy_tx(1).compute_txid();
        let second = dummy_tx(2).compute_txid();

        // Add a txid to the mempool watch list, and it comes back as pending.
        store.add_mempool_watch(first).unwrap();
        store.add_mempool_watch(first).unwrap();
        assert_eq!(store.get_mempool_watch_list().unwrap(), vec![(first, None)]);

        // Save a confirmation height for it, and it comes back with that height.
        store
            .save_mempool_watch_list(vec![(first, Some(9))])
            .unwrap();
        store.add_mempool_watch(first).unwrap();
        assert_eq!(
            store.get_mempool_watch_list().unwrap(),
            vec![(first, Some(9))]
        );

        // Add a second txid, and remove the first one by txid.
        store.add_mempool_watch(second).unwrap();
        store.remove_mempool_watch(&first).unwrap();
        assert_eq!(
            store.get_mempool_watch_list().unwrap(),
            vec![(second, None)]
        );
    }

    // Each save replaces the whole mempool snapshot.
    #[test]
    fn mempool_snapshot() {
        let store = temp_store();
        let first = dummy_tx(1).compute_txid();
        let second = dummy_tx(2).compute_txid();

        store.save_mempool_snapshot(vec![first]).unwrap();
        assert!(store.is_in_mempool_snapshot(&first).unwrap());

        store.save_mempool_snapshot(vec![second]).unwrap();
        assert!(!store.is_in_mempool_snapshot(&first).unwrap());
        assert!(store.is_in_mempool_snapshot(&second).unwrap());
    }
}
