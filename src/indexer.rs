use crate::{
    config::IndexerSettings,
    errors::IndexerError,
    helper::{confirmations, estimate_fee_rate, height_to_prune, is_below_window, window_start},
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

        let tip = indexer.bitcoin_client.get_tip_height()?;
        let window_start = window_start(tip, indexer.settings.retention_depth);

        // A reorg that happened while the indexer was down is not handled here. tick() is the only place that unwinds one.
        match indexer.store.get_cursor()? {
            // A fresh database has nothing to resume from. Start one window below the tip.
            None => {
                info!("No cursor stored, starting at height {window_start} (tip {tip})",);
                indexer.index_first_block(window_start)?;
            }
            // A restart that is catching up from a stored cursor reads every block in between.
            Some(cursor) if indexer.settings.catch_up => {
                info!("Resuming from height {cursor} (tip {tip})");
            }
            // A restart that is not catching up jumps straight to tip - retention_depth.
            Some(cursor) if window_start > cursor.saturating_add(1) => {
                warn!(
                    "catch_up is disabled, skipping blocks {}..={}. Output pattern and spending UTXO events in that range are lost",
                    cursor.saturating_add(1),
                    window_start.saturating_sub(1)
                );

                indexer.delete_window(cursor)?;
                indexer.index_first_block(window_start)?;
            }
            // A restart that is not catching up, but the cursor is already at or above tip - retention_depth, so no blocks are skipped.
            Some(cursor) => {
                info!("Resuming from height {cursor} (tip {tip})");
            }
        }

        // The stored snapshot describes the mempool as the previous run left it, so nothing 
        //in it is trusted until the first tick refreshes it.
        indexer.store.save_mempool_snapshot(vec![])?;

        Ok(indexer)
    }

    // =========================================================================
    // Public API
    // =========================================================================

    /// True once the cursor has reached the node's tip, so there is nothing left to read.
    pub fn is_ready(&self) -> Result<bool, IndexerError> {
        Ok(self.get_indexed_height()? >= self.bitcoin_client.get_tip_height()?)
    }

    /// Height of the highest block the indexer has read. Always present once the indexer is built.
    pub fn get_indexed_height(&self) -> Result<BlockHeight, IndexerError> {
        self.store.get_cursor_or_err()
    }

    /// The highest block the indexer has read. Always present once the indexer is built.
    pub fn get_last_indexed_block(&self) -> Result<FullBlock, IndexerError> {
        self.store.get_block_or_err(self.get_indexed_height()?)
    }

    /// Returns the block with this height and hash.
    /// - If the indexer holds a block at `height` with that hash, it is returned from storage.
    /// - If `height` is below every block the indexer holds and the node's block at `height` has that hash, it is
    ///   downloaded from the node.
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

        // Check that the height is below everything the indexer holds.
        let cursor = self.get_indexed_height()?;
        if !is_below_window(height, cursor, false) {
            info!("Block {hash} at height {height} is above the indexed height {cursor}");
            return Ok(None);
        }

        // A downloaded block does not carry its height, so the node's block at that height must be the one asked for.
        let node_hash = self.bitcoin_client.get_block_id_by_height(&height)?;
        if node_hash != *hash {
            info!("Block {hash} is not the node's block at height {height}, which is {node_hash}");
            return Ok(None);
        }

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
        self.refresh_mempool_watch_list()?;
        Ok(added)
    }

    /// Registers a txid to follow in the mempool on every tick.
    pub fn add_mempool_watch(&self, tx_id: Txid) -> Result<(), IndexerError> {
        self.store.add_mempool_watch(tx_id)
    }

    /// Stops following a txid. Only the consumer can decide this.
    pub fn remove_mempool_watch(&self, tx_id: &Txid) -> Result<(), IndexerError> {
        self.store.remove_mempool_watch(tx_id)
    }

    /// What the indexer knows about a transaction, in three steps:
    /// 1. In a block the indexer holds, which answers from storage.
    /// 2. In the mempool snapshot of the last tick that completed, when the caller asked about the mempool.
    /// 3. Otherwise the node is asked once, which covers a transaction mined in a block below the window
    ///    and one in the mempool that nobody watches.
    ///
    /// `include_mempool` suppresses mempool answers from the indexer's snapshot, it does not stop the node being asked.
    pub fn get_transaction(
        &self,
        tx_id: &Txid,
        include_mempool: bool,
    ) -> Result<TransactionStatus, IndexerError> {
        if let Some((tx, block)) = self.store.get_indexed_tx(tx_id)? {
            let cursor = self.get_indexed_height()?;

            return Ok(TransactionStatus::new(
                tx,
                block.height,
                block.hash,
                confirmations(cursor, block.height),
            ));
        }

        if include_mempool && self.store.is_in_mempool_snapshot(tx_id)? {
            return Ok(TransactionStatus::InMempool);
        }

        self.get_transaction_from_node(tx_id, include_mempool)
    }

    /// Fee rate estimated from the most recently indexed block.
    pub fn get_estimated_fee_rate(&self) -> Result<u64, IndexerError> {
        let last_block = self.get_last_indexed_block()?;

        if last_block.height != self.bitcoin_client.get_tip_height()? {
            return Err(IndexerError::NotSynced);
        }

        if last_block.estimated_fee_rate == 0 {
            return Err(IndexerError::FeeRateNotEstimated);
        }

        Ok(last_block.estimated_fee_rate)
    }

    /// Live RPC check for UTXO spendability, bypassing everything the indexer stores.
    /// True when the UTXO is unspent, counting the mempool when `include_mempool` is true.
    pub fn rpc_is_utxo_unspent(
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
    pub fn rpc_get_tx_confirmations(&self, tx_id: &Txid) -> Result<Option<u32>, IndexerError> {
        Ok(self.bitcoin_client.get_tx_confirmations(tx_id)?)
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Moves the indexer at most one block. Returns true only when a block was added.
    fn advance(&self) -> Result<bool, IndexerError> {
        let cursor = self.get_indexed_height()?;
        let tip = self.bitcoin_client.get_tip_height()?;

        // The node's chain is shorter than the indexed one.
        if cursor > tip {
            // Nothing is deleted when the indexer holds no block at the node's tip to continue from.
            if self.store.get_block(tip)?.is_none() {
                return Err(IndexerError::ReorgDeeperThanWindow(tip));
            }

            self.remove_blocks_above(tip, cursor)?;
            return Ok(false);
        }

        let last_block = self.store.get_block_or_err(cursor)?;
        let node_block = self.rpc_get_block_at(cursor)?;

        // The last indexed block was reorged out: the node has a different block at that height.
        if node_block.hash != last_block.hash {
            warn!(
                "Reorg detected at height {}. Indexed {}, node {}",
                cursor, last_block.hash, node_block.hash
            );

            // The block is kept when there is no block below it to continue from, and below genesis there is none.
            let below = cursor.saturating_sub(1);
            if cursor == 0 || self.store.get_block(below)?.is_none() {
                return Err(IndexerError::ReorgDeeperThanWindow(below));
            }

            self.remove_last_indexed_block(cursor)?;
            return Ok(false);
        }

        // No new block on the node.
        if cursor == tip {
            return Ok(false);
        }

        // Cursor < tip, so the node has a new block.
        let next_height = cursor.saturating_add(1);
        let next_block = self.rpc_get_block_at(next_height)?;

        // The next block does not build on the last indexed block, so that block was reorged out between the two reads above.
        if next_block.prev_hash != last_block.hash {
            warn!(
                "Block {} does not build on the indexed block at {}. Storing nothing, the next tick will handle the reorg",
                next_height, cursor
            );

            return Ok(false);
        }

        info!("Indexing block {} of {}", next_height, tip);
        self.index_block(next_block)?;

        Ok(true)
    }

    /// Deletes the indexed blocks above the node's tip, puts the mempool watch entries they confirmed back
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

        self.reset_mempool_watch_list_from(tip.saturating_add(1))?;
        self.store.save_cursor(tip)
    }

    /// Deletes the block at the cursor with the height entries of its transactions, puts the mempool watch entries
    /// it confirmed back to pending, and moves the cursor one block back.
    fn remove_last_indexed_block(&self, cursor: BlockHeight) -> Result<(), IndexerError> {
        self.store.delete_block(cursor)?;
        self.reset_mempool_watch_list_from(cursor)?;
        self.store.save_cursor(cursor.saturating_sub(1))
    }

    /// Stores a block with its estimated fee rate, moves the cursor onto it, and deletes the block
    /// that falls out of the retention window.
    fn index_block(&self, block: BlockInfo) -> Result<(), IndexerError> {
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
    fn refresh_mempool_watch_list(&self) -> Result<(), IndexerError> {
        let mut mempool_watch_list = self.store.get_mempool_watch_list()?;
        let mut in_mempool = Vec::new();
        let mut mempool_watch_list_changed = false;

        for (tx_id, confirmed_at) in mempool_watch_list.iter_mut() {
            // Already confirmed in a block the indexer holds. No block read and no RPC call.
            if confirmed_at.is_some() {
                continue;
            }

            if let Some(height) = self.store.get_tx_height(tx_id)? {
                *confirmed_at = Some(height);
                mempool_watch_list_changed = true;
                continue;
            }

            if self.bitcoin_client.check_in_mempool(tx_id)? {
                in_mempool.push(*tx_id);
            }
        }

        if mempool_watch_list_changed {
            self.store.save_mempool_watch_list(mempool_watch_list)?;
        }

        self.store.save_mempool_snapshot(in_mempool)?;

        Ok(())
    }

    /// The rpc node's block at this height. Fails with `BlockNotFound` when the node has none.
    fn rpc_get_block_at(&self, height: BlockHeight) -> Result<BlockInfo, IndexerError> {
        self.bitcoin_client
            .get_block_by_height(&height)?
            .ok_or(IndexerError::BlockNotFound(height))
    }

    /// Reads the block that a fresh start, or a jump, begins from, and puts the cursor on it.
    fn index_first_block(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let block = self.rpc_get_block_at(height)?;

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

        self.reset_mempool_watch_list_from(0)
    }

    /// Puts every mempool watch entry confirmed at `height` or above back to pending, because the blocks that
    /// confirmed them are no longer held. The next refresh checks them again.
    fn reset_mempool_watch_list_from(&self, height: BlockHeight) -> Result<(), IndexerError> {
        let mut mempool_watch_list = self.store.get_mempool_watch_list()?;
        let mut changed = false;

        for (_, confirmed_at) in mempool_watch_list.iter_mut() {
            if confirmed_at.is_some_and(|confirmed| confirmed >= height) {
                *confirmed_at = None;
                changed = true;
            }
        }

        if changed {
            self.store.save_mempool_watch_list(mempool_watch_list)?;
        }

        Ok(())
    }

    /// The node is asked once whether the transaction is mined, in the mempool, or unknown. A mined
    /// transaction costs one more call, for the height of its block, and is only reported `Confirmed` when that
    /// block is below everything the indexer holds.
    fn get_transaction_from_node(
        &self,
        tx_id: &Txid,
        include_mempool: bool,
    ) -> Result<TransactionStatus, IndexerError> {
        // A transaction ahead of the indexer cannot be evaluated for confirmation, so it is reported as pending.
        let not_confirmed = || {
            if include_mempool {
                TransactionStatus::InMempool
            } else {
                TransactionStatus::NotFound
            }
        };

        let info = match self.bitcoin_client.get_raw_transaction_info(tx_id)? {
            Some(info) => info,
            None => return Ok(TransactionStatus::NotFound),
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
        let cursor = self.get_indexed_height()?;
        let height_is_stored = self.store.get_block(height)?.is_some();

        // A block above the cursor, or at a height the indexer holds with a different block, is one the indexer has not processed.
        if !is_below_window(height, cursor, height_is_stored) {
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
