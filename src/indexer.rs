use crate::{
    config::IndexerSettings,
    errors::IndexerError,
    helper::{
        confirmations, estimate_fee_rate, height_to_prune, is_older_than_window, start_height,
    },
    store::IndexerStore,
    types::{FullBlock, TransactionStatus},
};
use bitcoin::Txid;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::*};
use std::rc::Rc;
use tracing::{info, warn};

/// Turns the node's stateless RPC into a stateful, resumable, reorg aware sequential feed.
pub struct Indexer<B>
where
    B: BitcoinClientApi,
{
    bitcoin_client: B,
    store: Rc<IndexerStore>,
    settings: IndexerSettings,
}

impl<B> Indexer<B>
where
    B: BitcoinClientApi,
{
    pub fn new(
        bitcoin_client: B,
        store: Rc<IndexerStore>,
        settings: Option<IndexerSettings>,
    ) -> Result<Self, IndexerError> {
        let settings = settings.unwrap_or_default();
        settings.validate()?;

        // Fee estimation and get_transaction both look a transaction up by its id alone, which only the
        // node's transaction index can answer. Fail here instead of on the first query that needs it.
        if !bitcoin_client.is_txindex_enabled()? {
            return Err(IndexerError::InvalidConfiguration(
                "the bitcoin node must run with -txindex".to_string(),
            ));
        }

        let indexer = Self {
            bitcoin_client,
            store,
            settings,
        };

        let tip = indexer.bitcoin_client.get_best_block()?;
        let start = start_height(tip, indexer.settings.retention_depth);

        // If a reorg that happened while this processwas down tick() is the single place that unwinds one.
        match indexer.store.get_cursor()? {
            // A fresh database has no subscriptions yet. Start one window below the tip.
            None => {
                info!("No cursor stored, starting at height {start} (tip {tip})",);
                indexer.index_first_block(start)?;
            }
            // A restart that is catching up from a stored cursor reads every block in between.
            Some(cursor) if indexer.settings.catch_up => {
                info!("Resuming from height {cursor} (tip {tip})");
            }
            // A restart that is not catching up jumps straight to tip - retention_depth.
            Some(cursor) if start > cursor.saturating_add(1) => {
                warn!(
                    "catch_up is disabled, skipping blocks {}..={}. Output pattern and spending UTXO events in that range are lost",
                    cursor.saturating_add(1),
                    start.saturating_sub(1)
                );

                indexer.delete_window(cursor)?;
                indexer.index_first_block(start)?;
            }
            // A restart that is not catching up, but the cursor is already at or above tip - retention_depth, so no blocks are skipped.
            Some(cursor) => {
                info!("Resuming from height {cursor} (tip {tip})");
            }
        }

        Ok(indexer)
    }

    // =========================================================================
    // Public API
    // =========================================================================

    /// True once the indexer has read every block the node has.
    pub fn is_ready(&self) -> Result<bool, IndexerError> {
        Ok(self.get_best_height()? >= self.bitcoin_client.get_best_block()?)
    }

    /// Height of the highest block the indexer has read. Always present once the indexer is built.
    pub fn get_best_height(&self) -> Result<BlockHeight, IndexerError> {
        self.store.get_cursor_or_err()
    }

    /// The highest block the indexer has read. Always present once the indexer is built.
    pub fn get_best_block(&self) -> Result<FullBlock, IndexerError> {
        self.store.get_block_or_err(self.get_best_height()?)
    }

    /// Returns the block with this height and hash.
    /// - If the indexer holds a block at `height` with that hash, it is returned from storage.
    /// - If `height` is older than every block the indexer holds, the block is downloaded from the node.
    /// - Otherwise it returns `None`, because that block is not on the chain the indexer has processed.
    pub fn get_block(
        &self,
        height: BlockHeight,
        hash: &BlockHash,
    ) -> Result<Option<FullBlock>, IndexerError> {
        // The indexer holds a block at this height. If the hash differs, the node has reorged it out of the chain.
        if let Some(stored) = self.store.get_block(height)? {
            if stored.hash != *hash {
                warn!(
                    "Block {hash} at height {height} differs from the indexed block {}",
                    stored.hash
                );
                return Ok(None);
            }
            return Ok(Some(stored));
        }

        // Check that the height is older than everything the indexer holds.
        let cursor = self.get_best_height()?;
        if !is_older_than_window(height, cursor, false) {
            info!("Block {hash} at height {height} is above the indexed height {cursor}");
            return Ok(None);
        }

        // The indexer does not hold a block at this height, so the node is asked for it.
        let block = self.bitcoin_client.get_block_by_hash(hash)?;
        let estimated_fee_rate = estimate_fee_rate(&self.bitcoin_client, &block.txdata)?;

        Ok(Some(FullBlock {
            height,
            hash: block.block_hash(),
            prev_hash: block.header.prev_blockhash,
            txs: block.txdata,
            estimated_fee_rate,
        }))
    }

    /// Moves the indexer at most one block, then refreshes the mempool snapshot.
    ///
    /// Returns true only when a block was added. Removing blocks after a reorg or a shorter chain, and having no
    /// new block to add, return false.
    pub fn tick(&self) -> Result<bool, IndexerError> {
        let added = self.advance()?;
        self.refresh_mempool_cache()?;
        Ok(added)
    }

    /// Registers a txid to follow in the mempool on every tick.
    pub fn add_mempool_watch(&self, tx_id: Txid) -> Result<(), IndexerError> {
        self.store.add_watch(tx_id)
    }

    /// Stops following a txid. Only the consumer can decide this.
    pub fn remove_mempool_watch(&self, tx_id: &Txid) -> Result<(), IndexerError> {
        self.store.remove_watch(tx_id)
    }

    /// What the indexer knows about a transaction, in three steps:
    /// 1. In a block the indexer holds, which answers from storage.
    /// 2. Watched and in this tick's mempool snapshot, when the caller asked about the mempool.
    /// 3. Otherwise the node is asked once, which covers a transaction mined in a block older than the window
    ///    and one in the mempool that nobody watches.
    ///
    /// `search_in_mempool` suppresses mempool answers from the indexer's cache, it does not stop the node being asked.
    pub fn get_transaction(
        &self,
        tx_id: &Txid,
        search_in_mempool: bool,
    ) -> Result<TransactionStatus, IndexerError> {
        if let Some((tx, block)) = self.store.get_indexed_tx(tx_id)? {
            let cursor = self.get_best_height()?;

            return Ok(TransactionStatus::new(
                tx,
                block.height,
                block.hash,
                confirmations(cursor, block.height),
            ));
        }

        if search_in_mempool && self.store.is_in_mempool_cache(tx_id)? {
            return Ok(TransactionStatus::InMempool);
        }

        self.get_transaction_from_node(tx_id, search_in_mempool)
    }

    /// Fee rate estimated from the most recently indexed block.
    pub fn get_estimated_fee_rate(&self) -> Result<u64, IndexerError> {
        let best_block = self.get_best_block()?;

        if best_block.height != self.bitcoin_client.get_best_block()? {
            return Err(IndexerError::IndexerNotSynced);
        }

        if best_block.estimated_fee_rate == 0 {
            return Err(IndexerError::FeeRateNotEstimated);
        }

        Ok(best_block.estimated_fee_rate)
    }

    /// Live RPC check for UTXO spendability, bypassing everything the indexer stores.
    /// True when the UTXO is unspent, counting the mempool when `include_mempool` is true.
    pub fn is_utxo_unspent_rpc(
        &self,
        tx_id: &Txid,
        vout: u32,
        include_mempool: bool,
    ) -> Result<bool, IndexerError> {
        Ok(self
            .bitcoin_client
            .is_utxo_unspent(tx_id, vout, include_mempool)?)
    }

    /// Live `getrawtransaction` confirmation probe. `None` when the node does not know the transaction,
    /// `Some(0)` when it is in the mempool, `Some(n)` when it is mined with n confirmations.
    pub fn get_tx_confirmations(&self, tx_id: &Txid) -> Result<Option<u32>, IndexerError> {
        Ok(self.bitcoin_client.get_tx_confirmations(tx_id)?)
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Moves the indexer at most one block. Returns true only when a block was added.
    fn advance(&self) -> Result<bool, IndexerError> {
        let cursor = self.get_best_height()?;
        let tip = self.bitcoin_client.get_best_block()?;

        // The node's chain is shorter than the indexed one.
        if cursor > tip {
            self.remove_blocks_above(tip, cursor)?;
            return Ok(false);
        }

        let last_block = self.store.get_block_or_err(cursor)?;
        let node_block = self.node_block_at(cursor)?;

        // The last indexed block was reorged out: the node has a different block at that height.
        if node_block.hash != last_block.hash {
            warn!(
                "Reorg detected at height {}. Indexed {}, node {}",
                cursor, last_block.hash, node_block.hash
            );

            self.remove_last_block(cursor)?;
            return Ok(false);
        }

        // No new block on the node.
        if cursor == tip {
            return Ok(false);
        }

        // Cursor < tip, so the node has a new block.
        let next_height = cursor.saturating_add(1);
        let next_block = self.node_block_at(next_height)?;

        // The next block does not build on the last indexed block, so that block was reorged out between the two reads above.
        if next_block.prev_hash != last_block.hash {
            warn!(
                "Block {} does not build on the indexed block at {}. Storing nothing, the next tick will handle the reorg",
                next_height, cursor
            );

            return Ok(false);
        }

        info!("Indexing block {} of {}", next_height, tip);
        self.add_block(next_block)?;

        Ok(true)
    }

    /// Deletes the indexed blocks above the node's tip, puts the watch entries they confirmed back
    /// to pending, and moves the cursor to the tip.
    fn remove_blocks_above(
        &self,
        tip: BlockHeight,
        cursor: BlockHeight,
    ) -> Result<(), IndexerError> {
        warn!(
            "The chain is shorter than the indexer. Deleting blocks {}..={}",
            tip.saturating_add(1),
            cursor
        );

        for height in (tip.saturating_add(1)..=cursor).rev() {
            self.store.delete_block(height)?;
        }

        self.reset_watch_from(tip.saturating_add(1))?;
        self.store.save_cursor(tip)
    }

    /// Deletes the block at the cursor with the locators of its transactions, puts the watch entries
    /// it confirmed back to pending, and moves the cursor one block back.
    fn remove_last_block(&self, cursor: BlockHeight) -> Result<(), IndexerError> {
        self.store.delete_block(cursor)?;
        self.reset_watch_from(cursor)?;
        self.store.save_cursor(cursor.saturating_sub(1))
    }

    /// Stores a block with its estimated fee rate, moves the cursor onto it, and deletes the block
    /// that falls out of the retention window.
    fn add_block(&self, block: BlockInfo) -> Result<(), IndexerError> {
        let height = block.height;
        let estimated_fee_rate = estimate_fee_rate(&self.bitcoin_client, &block.txs)?;

        self.store.save_block(&FullBlock {
            height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs,
            estimated_fee_rate,
        })?;
        self.store.save_cursor(height)?;

        if let Some(pruned) = height_to_prune(height, self.settings.retention_depth) {
            self.store.delete_block(pruned)?;
        }

        Ok(())
    }

    /// Asks the node about every watched transaction that is not already confirmed in a held block, and records the answers.
    fn refresh_mempool_cache(&self) -> Result<(), IndexerError> {
        let mut watch_list = self.store.get_watch_list()?;
        let mut in_mempool = Vec::new();
        let mut watch_list_changed = false;

        for (tx_id, confirmed_at) in watch_list.iter_mut() {
            // Already confirmed in a block the indexer holds. No block read and no RPC call.
            if confirmed_at.is_some() {
                continue;
            }

            if let Some(height) = self.store.get_tx_height(tx_id)? {
                *confirmed_at = Some(height);
                watch_list_changed = true;
                continue;
            }

            if self.bitcoin_client.check_in_mempool(tx_id) {
                in_mempool.push(*tx_id);
            }
        }

        if watch_list_changed {
            self.store.save_watch_list(watch_list)?;
        }

        self.store.save_mempool_cache(in_mempool)?;

        Ok(())
    }

    /// The rpc node's block at this height. Fails with `BlockNotFound` when the node has none.
    fn node_block_at(&self, height: BlockHeight) -> Result<BlockInfo, IndexerError> {
        self.bitcoin_client
            .get_block_by_height(&height)?
            .ok_or(IndexerError::BlockNotFound(height))
    }

    /// Reads the block that a fresh start, or a jump, begins from, and puts the cursor on it.
    fn index_first_block(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let block = self.node_block_at(height)?;

        let estimated_fee_rate = estimate_fee_rate(&self.bitcoin_client, &block.txs)?;

        self.store.save_block(&FullBlock {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs,
            estimated_fee_rate,
        })?;
        self.store.save_cursor(block.height)?;

        Ok(())
    }

    /// Deletes every block the indexer holds, used when a restart jumps forward instead of catching up.
    /// The window below the cursor is the only place blocks can be, so the range is bounded.
    fn delete_window(&self, cursor: BlockHeight) -> Result<(), IndexerError> {
        let oldest = cursor.saturating_sub(self.settings.retention_depth);

        for height in (oldest..=cursor).rev() {
            self.store.delete_block(height)?;
        }

        self.reset_watch_from(0)
    }

    /// Puts every watch entry confirmed at `height` or above back to pending, because the blocks that
    /// confirmed them are no longer held. The next refresh checks them again.
    fn reset_watch_from(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let mut watch_list = self.store.get_watch_list()?;
        let mut changed = false;

        for (_, confirmed_at) in watch_list.iter_mut() {
            if confirmed_at.is_some_and(|confirmed| confirmed >= height) {
                *confirmed_at = None;
                changed = true;
            }
        }

        if changed {
            self.store.save_watch_list(watch_list)?;
        }

        Ok(())
    }

    /// The node is asked once whether the transaction is mined, in the mempool, or unknown. A mined
    /// transaction costs one more call, for the height of its block, and is only reported `Confirmed` when that
    /// block is older than everything the indexer holds.
    fn get_transaction_from_node(
        &self,
        tx_id: &Txid,
        search_in_mempool: bool,
    ) -> Result<TransactionStatus, IndexerError> {
        // A transaction that is ahed of the indexer is cannot be evaluated for confirmation, so it is reported as pending.
        let not_confirmed = || {
            if search_in_mempool {
                TransactionStatus::InMempool
            } else {
                TransactionStatus::NotFound
            }
        };

        // Any error here means the node does not have the transaction.
        let info = match self.bitcoin_client.get_raw_transaction_info(tx_id) {
            Ok(info) => info,
            Err(_) => return Ok(TransactionStatus::NotFound),
        };

        // No block hash means the node holds it in its mempool.
        let block_hash = match info.blockhash {
            None => return Ok(not_confirmed()),
            Some(block_hash) => block_hash,
        };

        // A block hash with zero confirmations is a block that is not on the chain.
        if info.confirmations.unwrap_or(0) == 0 {
            return Ok(TransactionStatus::NotFound);
        }

        let height = self
            .bitcoin_client
            .get_block_header_info(&block_hash)?
            .height as BlockHeight;
        let cursor = self.get_best_height()?;
        let block_stored_at_height = self.store.get_block(height)?.is_some();

        // A block above the cursor, or at a height the indexer holds with a different block, is one the indexer has not processed.
        if !is_older_than_window(height, cursor, block_stored_at_height) {
            return Ok(not_confirmed());
        }

        let tx = info.transaction().map_err(|e| {
            IndexerError::Internal(format!(
                "the node returned transaction {tx_id} in a form that does not decode: {e}"
            ))
        })?;

        Ok(TransactionStatus::new(
            tx,
            height,
            block_hash,
            confirmations(cursor, height),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use bitcoin::consensus::serialize;
    use bitcoin::hashes::Hash;
    use bitcoin::Transaction;
    use bitcoincore_rpc::json::{GetBlockHeaderResult, GetRawTransactionResult};
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClientApi;

    /// The node answer for a transaction, shaped like `getrawtransaction` verbose.
    fn raw_tx_info(
        tx: &Transaction,
        block_hash: Option<BlockHash>,
        confirmations: Option<u32>,
    ) -> GetRawTransactionResult {
        GetRawTransactionResult {
            in_active_chain: None,
            hex: serialize(tx),
            txid: tx.compute_txid(),
            hash: tx.compute_wtxid(),
            size: 0,
            vsize: 0,
            version: 2,
            locktime: 0,
            vin: vec![],
            vout: vec![],
            blockhash: block_hash,
            confirmations,
            time: None,
            blocktime: None,
        }
    }

    /// The node answer for a block header, built from the JSON `getblockheader` returns.
    fn header_at(height: BlockHeight, hash: BlockHash) -> GetBlockHeaderResult {
        serde_json::from_value(serde_json::json!({
            "hash": hash.to_string(),
            "confirmations": 1,
            "height": height,
            "version": 536870912,
            "versionHex": "20000000",
            "merkleroot": "0000000000000000000000000000000000000000000000000000000000000000",
            "time": 0,
            "mediantime": 0,
            "nonce": 0,
            "bits": "207fffff",
            "difficulty": 1.0,
            "chainwork": "0000000000000000000000000000000000000000000000000000000000000002",
            "nTx": 1
        }))
        .expect("a getblockheader answer")
    }

    fn mock_node_at_tip(tip: BlockHeight) -> MockBitcoinClientApi {
        let mut bitcoin_client = MockBitcoinClientApi::new();
        bitcoin_client
            .expect_is_txindex_enabled()
            .returning(|| Ok(true));
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(tip));

        bitcoin_client
    }

    fn settings(retention_depth: BlockHeight, catch_up: bool) -> Option<IndexerSettings> {
        Some(IndexerSettings::new(retention_depth, catch_up))
    }

    /// A store holding one block at `cursor`, with the cursor on it.
    fn store_at(cursor: BlockHeight) -> Rc<IndexerStore> {
        let store = temp_store();
        store
            .save_block(&full_block(cursor, [cursor as u8; 32], [0u8; 32], vec![]))
            .unwrap();
        store.save_cursor(cursor).unwrap();
        store
    }

    // A fresh database starts one window below the tip.
    #[test]
    fn new_fresh_start() {
        let mut bitcoin_client = mock_node_at_tip(1000);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [90u8; 32], [89u8; 32], vec![]))));

        let store = temp_store();
        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(100, true)).unwrap();

        assert_eq!(indexer.get_best_height().unwrap(), 900);
        assert_eq!(store.get_block(900).unwrap().unwrap().height, 900);
    }

    // A chain shorter than the window starts at genesis.
    #[test]
    fn new_short_chain() {
        let mut bitcoin_client = mock_node_at_tip(3);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [0u8; 32], [0u8; 32], vec![]))));

        let indexer = Indexer::new(bitcoin_client, temp_store(), settings(100, true)).unwrap();

        assert_eq!(indexer.get_best_height().unwrap(), 0);
    }

    // A node without -txindex is refused.
    #[test]
    fn new_requires_txindex() {
        let mut bitcoin_client = MockBitcoinClientApi::new();
        bitcoin_client
            .expect_is_txindex_enabled()
            .returning(|| Ok(false));

        // Indexer holds the client, which is not Debug, so unwrap_err is not available here.
        let result = Indexer::new(bitcoin_client, temp_store(), settings(100, true));

        assert!(matches!(result, Err(IndexerError::InvalidConfiguration(_))));
    }

    // A restart with catch_up resumes from its cursor.
    #[test]
    fn new_catch_up() {
        let store = store_at(10);

        let bitcoin_client = mock_node_at_tip(1000);
        let indexer = Indexer::new(bitcoin_client, store, settings(100, true)).unwrap();

        assert_eq!(indexer.get_best_height().unwrap(), 10);
    }

    // A restart without catch_up jumps to the tip and deletes the old window.
    #[test]
    fn new_jump() {
        let store = temp_store();
        let old = full_block(10, [10u8; 32], [9u8; 32], vec![dummy_tx(1)]);
        store.save_block(&old).unwrap();
        store.save_cursor(10).unwrap();

        let mut bitcoin_client = mock_node_at_tip(1000);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [90u8; 32], [89u8; 32], vec![]))));

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(100, false)).unwrap();

        assert_eq!(indexer.get_best_height().unwrap(), 900);
        assert_eq!(store.get_block(10).unwrap(), None);
        assert_eq!(
            store.get_tx_height(&old.txs[0].compute_txid()).unwrap(),
            None
        );
    }

    // A restart without catch_up that is only one block behind does not jump.
    #[test]
    fn new_no_jump_one_behind() {
        let store = store_at(899);

        let bitcoin_client = mock_node_at_tip(999);
        let indexer = Indexer::new(bitcoin_client, store, settings(100, false)).unwrap();

        assert_eq!(indexer.get_best_height().unwrap(), 899);
    }

    // A tick indexes the next block and prunes the one that falls out of the window.
    #[test]
    fn tick_indexes_and_prunes() {
        let store = temp_store();
        let mut bitcoin_client = mock_node_at_tip(12);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| {
                let h = *height as u8;
                Ok(Some(block_info(*height, [h; 32], [h - 1; 32], vec![])))
            });
        bitcoin_client
            .expect_check_in_mempool()
            .returning(|_| false);

        // Retention 2 keeps the cursor and the block below it.
        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(2, true)).unwrap();
        assert_eq!(indexer.get_best_height().unwrap(), 10);

        assert!(indexer.tick().unwrap());
        assert_eq!(indexer.get_best_height().unwrap(), 11);
        assert!(store.get_block(10).unwrap().is_some());

        assert!(indexer.tick().unwrap());
        assert_eq!(indexer.get_best_height().unwrap(), 12);
        assert_eq!(store.get_block(10).unwrap(), None);
        assert!(store.get_block(11).unwrap().is_some());
    }

    // A tick at the tip only refreshes the mempool snapshot.
    #[test]
    fn tick_at_tip() {
        let store = store_at(10);
        let watched = dummy_tx(1).compute_txid();

        let mut bitcoin_client = mock_node_at_tip(10);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [10u8; 32], [9u8; 32], vec![]))));
        bitcoin_client.expect_check_in_mempool().returning(|_| true);

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(2, true)).unwrap();
        indexer.add_mempool_watch(watched).unwrap();

        assert!(!indexer.tick().unwrap());

        assert_eq!(indexer.get_best_height().unwrap(), 10);
        assert!(store.is_in_mempool_cache(&watched).unwrap());
        assert_eq!(
            indexer.get_transaction(&watched, true).unwrap(),
            TransactionStatus::InMempool
        );
    }

    // A reorg deletes the block that lost, resets its watch entries and steps back one block.
    #[test]
    fn tick_reorg() {
        let store = temp_store();
        let tx = dummy_tx(7);
        store
            .save_block(&full_block(9, [9u8; 32], [8u8; 32], vec![]))
            .unwrap();
        store
            .save_block(&full_block(10, [10u8; 32], [9u8; 32], vec![tx.clone()]))
            .unwrap();
        store.save_cursor(10).unwrap();
        store
            .save_watch_list(vec![(tx.compute_txid(), Some(10))])
            .unwrap();

        let mut bitcoin_client = mock_node_at_tip(10);
        // The chain now has a different block at height 10.
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [77u8; 32], [9u8; 32], vec![]))));
        bitcoin_client.expect_check_in_mempool().returning(|_| true);

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(5, true)).unwrap();
        // Unwinding is the indexer's own work, so the tick reports no new block.
        assert!(!indexer.tick().unwrap());

        assert_eq!(indexer.get_best_height().unwrap(), 9);
        assert_eq!(store.get_block(10).unwrap(), None);
        assert_eq!(store.get_tx_height(&tx.compute_txid()).unwrap(), None);
        // The entry went back to pending and the refresh found the transaction in the mempool.
        assert_eq!(
            store.get_watch_list().unwrap(),
            vec![(tx.compute_txid(), None)]
        );
        assert!(store.is_in_mempool_cache(&tx.compute_txid()).unwrap());
    }

    // A next block that does not build on the block at the cursor is not stored.
    #[test]
    fn tick_prev_hash_mismatch() {
        let store = store_at(10);

        let mut bitcoin_client = mock_node_at_tip(11);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| match *height {
                // The block at the cursor still matches, so no reorg is visible yet.
                10 => Ok(Some(block_info(10, [10u8; 32], [9u8; 32], vec![]))),
                // The next block comes from another chain: its parent is not the stored block.
                _ => Ok(Some(block_info(11, [11u8; 32], [99u8; 32], vec![]))),
            });
        bitcoin_client
            .expect_check_in_mempool()
            .returning(|_| false);

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(5, true)).unwrap();
        assert!(!indexer.tick().unwrap());

        assert_eq!(indexer.get_best_height().unwrap(), 10);
        assert_eq!(store.get_block(11).unwrap(), None);
    }

    // A chain that shrank below the cursor drops the blocks above the new tip.
    #[test]
    fn tick_chain_shrank() {
        let store = temp_store();
        let tx = dummy_tx(3);
        for height in 8..=10 {
            let txs = if height == 10 {
                vec![tx.clone()]
            } else {
                vec![]
            };
            store
                .save_block(&full_block(
                    height,
                    [height as u8; 32],
                    [(height - 1) as u8; 32],
                    txs,
                ))
                .unwrap();
        }
        store.save_cursor(10).unwrap();
        store
            .save_watch_list(vec![(tx.compute_txid(), Some(10))])
            .unwrap();

        let mut bitcoin_client = mock_node_at_tip(8);
        bitcoin_client
            .expect_check_in_mempool()
            .returning(|_| false);

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(5, true)).unwrap();
        assert!(!indexer.tick().unwrap());

        assert_eq!(indexer.get_best_height().unwrap(), 8);
        assert_eq!(store.get_block(9).unwrap(), None);
        assert_eq!(store.get_block(10).unwrap(), None);
        assert_eq!(
            store.get_watch_list().unwrap(),
            vec![(tx.compute_txid(), None)]
        );
    }

    // A transaction in a held block is answered from storage.
    #[test]
    fn tx_in_window() {
        let store = temp_store();
        let tx = dummy_tx(5);
        store
            .save_block(&full_block(10, [10u8; 32], [9u8; 32], vec![tx.clone()]))
            .unwrap();
        store.save_cursor(12).unwrap();

        let bitcoin_client = mock_node_at_tip(12);
        let indexer = Indexer::new(bitcoin_client, store, settings(5, true)).unwrap();

        assert_eq!(
            indexer.get_transaction(&tx.compute_txid(), false).unwrap(),
            TransactionStatus::new(tx, 10, block_hash([10u8; 32]), 3)
        );
    }

    // A transaction mined below the window is confirmed by the node without downloading its block.
    #[test]
    fn tx_below_window() {
        let tx = dummy_tx(42);
        let tx_block_hash = block_hash([50u8; 32]);

        let mut bitcoin_client = mock_node_at_tip(1000);
        let answer = raw_tx_info(&tx, Some(tx_block_hash), Some(951));
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(move |_| Ok(answer.clone()));
        bitcoin_client
            .expect_get_block_header_info()
            .returning(move |hash| Ok(header_at(50, *hash)));
        // No get_block_by_hash expectation: the mock panics if step 3 downloads the block.

        let indexer = Indexer::new(bitcoin_client, store_at(1000), settings(100, true)).unwrap();

        assert_eq!(
            indexer.get_transaction(&tx.compute_txid(), true).unwrap(),
            // Confirmations come from the indexer's cursor, not from the node's count.
            TransactionStatus::new(tx, 50, tx_block_hash, 951)
        );
    }

    // A transaction in a block the indexer has not reached is not confirmed yet.
    #[test]
    fn tx_above_cursor() {
        let tx = dummy_tx(42);

        let mut bitcoin_client = mock_node_at_tip(11);
        let answer = raw_tx_info(&tx, Some(block_hash([11u8; 32])), Some(1));
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(move |_| Ok(answer.clone()));
        bitcoin_client
            .expect_get_block_header_info()
            .returning(|hash| Ok(header_at(11, *hash)));

        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();
        let tx_id = tx.compute_txid();

        assert_eq!(
            indexer.get_transaction(&tx_id, true).unwrap(),
            TransactionStatus::InMempool
        );
        assert_eq!(
            indexer.get_transaction(&tx_id, false).unwrap(),
            TransactionStatus::NotFound
        );
    }

    // A transaction only in a reorg the indexer has not unwound is not confirmed yet.
    #[test]
    fn tx_unwound_reorg() {
        let tx = dummy_tx(42);

        let mut bitcoin_client = mock_node_at_tip(10);
        // The node's new chain has the transaction at height 10, where the indexer still holds the old block.
        let answer = raw_tx_info(&tx, Some(block_hash([77u8; 32])), Some(1));
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(move |_| Ok(answer.clone()));
        bitcoin_client
            .expect_get_block_header_info()
            .returning(|hash| Ok(header_at(10, *hash)));

        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();
        let tx_id = tx.compute_txid();

        assert_eq!(
            indexer.get_transaction(&tx_id, true).unwrap(),
            TransactionStatus::InMempool
        );
        assert_eq!(
            indexer.get_transaction(&tx_id, false).unwrap(),
            TransactionStatus::NotFound
        );
    }

    // A transaction the node still points at a removed block is not found.
    #[test]
    fn tx_stale_block() {
        let tx = dummy_tx(1);

        let mut bitcoin_client = mock_node_at_tip(10);
        // A block hash with zero confirmations: the block was removed by a reorg.
        let answer = raw_tx_info(&tx, Some(BlockHash::all_zeros()), Some(0));
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(move |_| Ok(answer.clone()));

        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();

        assert_eq!(
            indexer.get_transaction(&tx.compute_txid(), true).unwrap(),
            TransactionStatus::NotFound
        );
    }

    // An unwatched transaction in the node mempool is InMempool or NotFound, following the flag.
    #[test]
    fn tx_unwatched_mempool() {
        let tx = dummy_tx(1);

        let mut bitcoin_client = mock_node_at_tip(10);
        let answer = raw_tx_info(&tx, None, None);
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(move |_| Ok(answer.clone()));

        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();
        let tx_id = tx.compute_txid();

        assert_eq!(
            indexer.get_transaction(&tx_id, true).unwrap(),
            TransactionStatus::InMempool
        );
        // The same answer, asked without the mempool, is suppressed.
        assert_eq!(
            indexer.get_transaction(&tx_id, false).unwrap(),
            TransactionStatus::NotFound
        );
    }

    // A transaction the node does not know is not found.
    #[test]
    fn tx_unknown() {
        let mut bitcoin_client = mock_node_at_tip(10);
        bitcoin_client
            .expect_get_raw_transaction_info()
            .returning(|_| {
                // What the node answers for a txid it has never seen: getrawtransaction returns RPC error -5.
                Err(
                    bitvmx_bitcoin_rpc::errors::BitcoinClientError::FailedToGetTransactionDetails {
                        error: "No such mempool or blockchain transaction".to_string(),
                    },
                )
            });

        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();

        assert_eq!(
            indexer
                .get_transaction(&dummy_tx(1).compute_txid(), true)
                .unwrap(),
            TransactionStatus::NotFound
        );
    }

    // A held block with the requested hash comes from storage.
    #[test]
    fn block_from_storage() {
        // No expectations beyond construction: the mock panics if get_block calls the node.
        let bitcoin_client = mock_node_at_tip(10);
        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();

        let block = indexer
            .get_block(10, &block_hash([10u8; 32]))
            .unwrap()
            .unwrap();

        assert_eq!(block.height, 10);
        assert_eq!(block.hash, block_hash([10u8; 32]));
    }

    // A held height with a different hash gives no block.
    #[test]
    fn block_other_hash() {
        let bitcoin_client = mock_node_at_tip(10);
        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();

        assert_eq!(
            indexer.get_block(10, &block_hash([77u8; 32])).unwrap(),
            None
        );
    }

    // A block above the cursor gives no block.
    #[test]
    fn block_above_cursor() {
        let bitcoin_client = mock_node_at_tip(11);
        let indexer = Indexer::new(bitcoin_client, store_at(10), settings(5, true)).unwrap();

        assert_eq!(
            indexer.get_block(11, &block_hash([11u8; 32])).unwrap(),
            None
        );
    }

    // A block below the window is downloaded from the node, with its fee rate.
    #[test]
    fn block_below_window() {
        let old_block = block_at_height(50, [49u8; 32]);
        let old_hash = old_block.block_hash();

        let mut bitcoin_client = mock_node_at_tip(1000);
        bitcoin_client
            .expect_get_block_by_hash()
            .withf(move |hash| *hash == old_hash)
            .returning(move |_| Ok(block_at_height(50, [49u8; 32])));

        let indexer = Indexer::new(bitcoin_client, store_at(1000), settings(100, true)).unwrap();

        let block = indexer.get_block(50, &old_hash).unwrap().unwrap();

        assert_eq!(block.height, 50);
        assert_eq!(block.hash, old_hash);
        assert_eq!(block.txs.len(), 1);
        // A block with a single transaction has no fee rate, and costs no estimation call.
        assert_eq!(block.estimated_fee_rate, 0);
    }

    // The indexer never removes a watch entry on its own, only remove_mempool_watch does.
    #[test]
    fn watch_never_removed() {
        let store = temp_store();
        let tx = dummy_tx(1);

        let mut bitcoin_client = mock_node_at_tip(10);
        bitcoin_client
            .expect_get_block_by_height()
            .returning(|height| Ok(Some(block_info(*height, [10u8; 32], [9u8; 32], vec![]))));
        bitcoin_client
            .expect_check_in_mempool()
            .returning(|_| false);

        let indexer = Indexer::new(bitcoin_client, store.clone(), settings(5, true)).unwrap();
        indexer.add_mempool_watch(tx.compute_txid()).unwrap();

        for _ in 0..3 {
            indexer.tick().unwrap();
        }

        assert_eq!(
            store.get_watch_list().unwrap(),
            vec![(tx.compute_txid(), None)]
        );

        indexer.remove_mempool_watch(&tx.compute_txid()).unwrap();
        assert!(store.get_watch_list().unwrap().is_empty());
    }
}
