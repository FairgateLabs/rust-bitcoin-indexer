//! Integration tests against a regtest bitcoind in Docker. Each test starts its own node, and the tests run one at a time.
mod common;
use common::*;

use bitcoin::{Block, Transaction, Txid};
use bitcoin_indexer::store::IndexerStore;
use bitcoin_indexer::{IndexerError, TransactionStatus};
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::BlockHeight};

/// The transaction as the node has it.
fn node_tx(node: &TestNode, tx_id: &Txid) -> anyhow::Result<Transaction> {
    node.client
        .get_transaction(tx_id)?
        .ok_or_else(|| anyhow::anyhow!("the node does not know {tx_id}"))
}

/// The `Confirmed` answer expected for a transaction in the node's block at `height`.
fn confirmed(
    node: &TestNode,
    tx_id: &Txid,
    height: BlockHeight,
    confirmations: u32,
) -> anyhow::Result<TransactionStatus> {
    Ok(TransactionStatus::new(
        node_tx(node, tx_id)?,
        height,
        node.hash_at(height)?,
        confirmations,
    ))
}

/// How many blocks the store holds from genesis to `tip`.
fn stored_blocks(store: &IndexerStore, tip: BlockHeight) -> anyhow::Result<usize> {
    let mut count = 0;
    for height in 0..=tip {
        if store.get_block(height)?.is_some() {
            count += 1;
        }
    }
    Ok(count)
}

// =============================================================================
// Startup and restarts
// =============================================================================

// A fresh database starts one window below the tip, or at genesis on a chain shorter than the window.
#[test]
fn fresh_start() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(3)?;

    // A chain shorter than the window starts at genesis and indexes every block.
    let storage = TestStorage::new();
    let indexer = node.indexer(storage.storage(), 5, true)?;

    // Until the first tick there is no cursor, so nothing can be answered from it.
    assert!(!indexer.is_ready()?);
    assert!(matches!(
        indexer.get_indexed_height(),
        Err(IndexerError::NotSynced)
    ));

    // The first tick places the cursor, here at genesis because the chain is shorter than the window.
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 0);
    assert_eq!(indexer.get_last_indexed_block()?.hash, node.hash_at(0)?);
    assert!(!indexer.is_ready()?);

    for height in 1..=3 {
        assert!(indexer.tick()?);
        assert_eq!(indexer.get_indexed_height()?, height);
    }
    assert!(indexer.is_ready()?);
    node.assert_window_matches(&storage.store(), 0)?;
    drop(indexer);

    // A chain longer than the window starts at tip - retention_depth, and the next tick indexes the next block.
    node.mine(17)?;
    let storage = TestStorage::new();
    let store = storage.store();
    let indexer = node.indexer(storage.storage(), 5, true)?;

    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 15);
    assert_eq!(indexer.get_last_indexed_block()?.hash, node.hash_at(15)?);
    assert_eq!(store.get_block(14)?, None);

    assert!(indexer.tick()?);
    assert_eq!(indexer.get_last_indexed_block()?.height, 16);

    Ok(())
}

// A restart with catch_up resumes from its cursor and indexes every block it missed, however long the gap.
#[test]
fn restart_catch_up() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(101)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 3, true)?;
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 98);
    node.sync(&indexer)?;
    drop(indexer);

    // A restart with the cursor at the tip has nothing to do.
    let indexer = node.indexer(storage.storage(), 3, true)?;
    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 101);
    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 101);
    drop(indexer);

    // A gap longer than the window: every block is indexed, and only the last window stays stored.
    node.mine(10)?;
    let indexer = node.indexer(storage.storage(), 3, true)?;
    assert!(!indexer.is_ready()?);

    let mut indexed = 0;
    for _ in 0..10 {
        if indexer.tick()? {
            indexed += 1;
        }
    }
    assert_eq!(indexed, 10);
    assert!(node.is_synced(&indexer)?);

    assert_eq!(stored_blocks(&store, 111)?, 3);
    node.assert_window_matches(&store, 109)?;
    for height in 102..=108 {
        assert_eq!(store.get_tx_height(&node.coinbase_txid_at(height)?)?, None);
    }
    for height in 109..=111 {
        assert_eq!(
            store.get_tx_height(&node.coinbase_txid_at(height)?)?,
            Some(height)
        );
    }

    Ok(())
}

// A restart without catch_up jumps to tip - retention_depth only when that skips blocks, and deletes the old window.
#[test]
fn restart_jump() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 3, false)?;
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 107);
    node.sync(&indexer)?;
    drop(indexer);

    // Four new blocks with a window of 3: the window starts right after the cursor, so nothing is skipped and the
    // first tick reads the next block instead of jumping.
    node.mine(4)?;
    let indexer = node.indexer(storage.storage(), 3, false)?;
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 111);
    node.sync(&indexer)?;

    // A watched transaction confirmed in the window.
    let watched = node.coinbase_txid_at(114)?;
    indexer.add_mempool_watch(watched)?;
    indexer.tick()?;
    assert_eq!(store.get_mempool_watch_list()?, vec![(watched, Some(114))]);
    drop(indexer);

    // Ten new blocks: the restart jumps, deletes the old window with its height entries, and resets the watch list.
    node.mine(10)?;
    let indexer = node.indexer(storage.storage(), 3, false)?;

    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 121);
    for height in 112..=114 {
        assert_eq!(store.get_block(height)?, None);
    }
    assert_eq!(store.get_tx_height(&watched)?, None);
    assert_eq!(store.get_mempool_watch_list()?, vec![(watched, None)]);
    assert_eq!(indexer.get_last_indexed_block()?.hash, node.hash_at(121)?);

    // The skipped range is still answered by the node.
    node.sync(&indexer)?;
    node.assert_window_matches(&store, 122)?;
    assert_eq!(
        indexer.get_transaction(&watched, false)?,
        confirmed(&node, &watched, 114, 11)?
    );

    Ok(())
}

// =============================================================================
// Advancing and pruning
// =============================================================================

// Each tick indexes at most one block, a tick at the tip does nothing, and storage never holds more than the window.
#[test]
fn tick_and_prune() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(101)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 3, true)?;
    node.sync(&indexer)?;
    assert!(indexer.is_ready()?);

    // At the tip, repeated ticks change nothing.
    for _ in 0..3 {
        assert!(!indexer.tick()?);
        assert_eq!(indexer.get_indexed_height()?, 101);
    }

    // Two blocks between ticks take two ticks.
    node.mine(2)?;
    assert!(!indexer.is_ready()?);
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 102);
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 103);
    assert!(!indexer.tick()?);
    assert!(indexer.is_ready()?);

    // Storage stops growing: only the window is stored, and pruned blocks take their height entries with them.
    node.mine(20)?;
    node.sync(&indexer)?;

    assert_eq!(stored_blocks(&store, 123)?, 3);
    node.assert_window_matches(&store, 121)?;
    for height in 102..=120 {
        assert_eq!(store.get_tx_height(&node.coinbase_txid_at(height)?)?, None);
    }

    let last = indexer.get_last_indexed_block()?;
    let node_block = node.block_at(123)?;
    assert_eq!(last.height, 123);
    assert_eq!(last.hash, node_block.block_hash());
    assert_eq!(last.prev_hash, node_block.header.prev_blockhash);
    assert_eq!(last.txs, node_block.txdata);

    Ok(())
}

// =============================================================================
// Reorgs
// =============================================================================

// A reorg is unwound one block per tick and the new chain is indexed, however it happens.
#[test]
fn reorg_unwinds() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    node.sync(&indexer)?;

    // One block replaced by a longer chain: the first tick deletes it, the next one indexes the new block.
    let old_hash = node.invalidate(110)?;
    node.mine(2)?;

    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 109);
    assert_eq!(store.get_block(110)?, None);

    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 110);
    assert_ne!(store.get_block(110)?.unwrap().hash, old_hash);
    node.sync(&indexer)?;
    node.assert_window_matches(&store, 102)?;

    // Five blocks replaced by seven.
    node.invalidate(107)?;
    node.mine(7)?;
    node.sync(&indexer)?;
    assert_eq!(indexer.get_indexed_height()?, 113);
    node.assert_window_matches(&store, 104)?;

    // A reorg at the cursor while the indexer is still behind the tip.
    node.mine(5)?;
    indexer.tick()?;
    indexer.tick()?;
    assert_eq!(indexer.get_indexed_height()?, 115);
    node.invalidate(115)?;
    node.mine(6)?;
    node.sync(&indexer)?;
    node.assert_window_matches(&store, 111)?;

    // The chain drops its tip block and then switches back to that same block: it is indexed again and its
    // transaction reads Confirmed. This is the shape of the testnet cursor stuck incident.
    let tx_id = node.send(10_000)?;
    node.mine(1)?;
    node.sync(&indexer)?;
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 121, 1)?
    );

    let tip_hash = node.invalidate(121)?;
    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 120);
    assert_eq!(store.get_block(121)?, None);
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::InMempool
    );

    node.reconsider(&tip_hash)?;
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 121);
    assert_eq!(store.get_block(121)?.unwrap().hash, tip_hash);
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 121, 1)?
    );

    // The cursor keeps moving.
    assert!(!indexer.tick()?);
    node.mine(1)?;
    assert!(indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 122);

    Ok(())
}

// A chain that becomes shorter than the indexed one drops the blocks above its tip in one tick, while the indexer
// runs and while it is down.
#[test]
fn chain_shrinks() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    node.sync(&indexer)?;

    let tx_id = node.send(10_000)?;
    indexer.add_mempool_watch(tx_id)?;
    node.mine(3)?;
    node.sync(&indexer)?;
    let coinbase = node.coinbase_txid_at(113)?;
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, Some(111))]);

    // The last three blocks are removed with no replacement.
    node.invalidate(111)?;
    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 110);
    for height in 111..=113 {
        assert_eq!(store.get_block(height)?, None);
    }

    // Their transactions went back to the mempool and read InMempool, not NotFound.
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, None)]);
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::InMempool
    );
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        TransactionStatus::NotFound
    );

    // A coinbase never returns to the mempool.
    assert_eq!(
        indexer.get_transaction(&coinbase, true)?,
        TransactionStatus::NotFound
    );

    // The next block confirms the transaction again.
    node.mine(1)?;
    assert!(indexer.tick()?);
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 111, 1)?
    );
    drop(indexer);

    // The chain shrinks while the indexer is down, so a restart finds its cursor above the node's tip. The wallet set
    // the transaction's locktime to 110, so it can only be mined again from height 111.
    node.invalidate(111)?;
    let indexer = node.indexer(storage.storage(), 10, true)?;
    assert_eq!(store.get_cursor()?, Some(111));

    assert!(!indexer.tick()?);
    assert_eq!(indexer.get_indexed_height()?, 110);
    assert_eq!(store.get_block(111)?, None);

    node.mine(3)?;
    node.sync(&indexer)?;
    node.assert_window_matches(&store, 104)?;
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 111, 3)?
    );

    Ok(())
}

// A reorg shallower than the window is unwound. A deeper one fails every tick with ReorgDeeperThanWindow once the
// indexer has no block left to continue from, and the block it cannot continue from stays stored.
#[test]
fn reorg_deeper_than_window() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 3, true)?;
    node.sync(&indexer)?;

    // Depth retention_depth - 1.
    node.invalidate(109)?;
    node.mine(3)?;
    node.sync(&indexer)?;
    assert_eq!(indexer.get_indexed_height()?, 111);
    node.assert_window_matches(&store, 109)?;

    // Depth retention_depth: the three held blocks are all replaced.
    node.invalidate(109)?;
    node.mine(4)?;

    // Blocks 111 and 110 are unwound, then block 109 cannot be, because block 108 was pruned.
    for expected in [110, 109] {
        assert!(!indexer.tick()?);
        assert_eq!(indexer.get_indexed_height()?, expected);
    }
    for _ in 0..2 {
        assert!(matches!(
            indexer.tick(),
            Err(IndexerError::ReorgDeeperThanWindow(108))
        ));
        assert_eq!(indexer.get_indexed_height()?, 109);
        assert!(store.get_block(109)?.is_some());
    }

    // A chain that shrinks below the held blocks fails the same way, before deleting anything.
    node.invalidate(108)?;
    assert!(matches!(
        indexer.tick(),
        Err(IndexerError::ReorgDeeperThanWindow(107))
    ));
    assert_eq!(indexer.get_indexed_height()?, 109);
    assert!(store.get_block(109)?.is_some());

    Ok(())
}

// =============================================================================
// Transactions and the mempool watch list
// =============================================================================

// A watched transaction from before its broadcast until it is confirmed, including a reorg that mines it again at the same height.
#[test]
fn transaction_lifecycle() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let outpoint = node.fund_utxo(100_000)?;
    let tx = node.sign_spend(outpoint, 90_000)?;
    let tx_id = tx.compute_txid();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    node.sync(&indexer)?;

    // Watched before it is broadcast.
    indexer.add_mempool_watch(tx_id)?;
    indexer.tick()?;
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::NotFound
    );

    // Broadcast: the node answers before the next tick, and the snapshot after it.
    node.client.send_transaction(&tx)?;
    for tick in [false, true] {
        if tick {
            indexer.tick()?;
        }
        assert_eq!(
            indexer.get_transaction(&tx_id, true)?,
            TransactionStatus::InMempool
        );
        assert_eq!(
            indexer.get_transaction(&tx_id, false)?,
            TransactionStatus::NotFound
        );
    }

    // Mined in the first of two blocks. Before the indexer reaches it, it is not confirmed yet.
    node.mine(2)?;
    let height = indexer.get_indexed_height()? + 1;
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::InMempool
    );
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        TransactionStatus::NotFound
    );

    // The first tick confirms it, and each block adds a confirmation.
    assert!(indexer.tick()?);
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, height, 1)?
    );
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, Some(height))]);

    assert!(indexer.tick()?);
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        confirmed(&node, &tx_id, height, 2)?
    );
    assert_eq!(indexer.rpc_get_tx_confirmations(&tx_id)?, Some(2));
    assert!(indexer.rpc_is_utxo_unspent(&tx_id, 0, false)?);

    // A reorg that mines it again at the same height, in a different block.
    let old_hash = node.invalidate(height)?;
    node.mine(2)?;
    node.sync(&indexer)?;

    let status = indexer.get_transaction(&tx_id, false)?;
    assert_eq!(status, confirmed(&node, &tx_id, height, 2)?);
    assert_ne!(node.hash_at(height)?, old_hash);

    // Only remove_mempool_watch removes the entry.
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, Some(height))]);
    indexer.remove_mempool_watch(&tx_id)?;
    assert!(store.get_mempool_watch_list()?.is_empty());

    Ok(())
}

// A reorg that mines a transaction one block later: it reads InMempool in between, then Confirmed at the new height.
#[test]
fn transaction_changes_height() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    node.sync(&indexer)?;

    let tx_id = node.send(10_000)?;
    indexer.add_mempool_watch(tx_id)?;
    node.mine(1)?;
    node.sync(&indexer)?;
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 111, 1)?
    );

    // Block 111 is replaced by an empty one, and the transaction goes back to the mempool.
    node.invalidate(111)?;
    node.mine_empty()?;
    node.sync(&indexer)?;

    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, None)]);
    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::InMempool
    );
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        TransactionStatus::NotFound
    );

    // The next block mines it.
    node.mine(1)?;
    node.sync(&indexer)?;
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, 112, 1)?
    );
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, Some(112))]);

    Ok(())
}

// A reorg mines a conflicting transaction, and later the old chain wins again.
#[test]
fn double_spend_and_flip_back() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let (tx, conflicting) = node.conflicting_txs()?;
    let tx_id = tx.compute_txid();
    let conflicting_id = conflicting.compute_txid();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    indexer.add_mempool_watch(tx_id)?;

    // The transaction has two confirmations.
    node.client.send_transaction(&tx)?;
    node.mine(2)?;
    node.sync(&indexer)?;
    let height = indexer.get_indexed_height()? - 1;
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, height, 2)?
    );

    // A shorter chain replaces both blocks with one that mines the conflicting transaction.
    let old_hash = node.invalidate(height)?;
    node.mine_with(&[conflicting])?;
    node.sync(&indexer)?;

    assert_eq!(
        indexer.get_transaction(&tx_id, true)?,
        TransactionStatus::NotFound
    );
    assert_eq!(
        indexer.get_transaction(&conflicting_id, false)?,
        confirmed(&node, &conflicting_id, height, 1)?
    );

    // The node's transaction index still points at the removed block, with zero confirmations.
    let info = node
        .client
        .get_raw_transaction_info(&tx_id)?
        .expect("the node still knows the transaction");
    assert_eq!(info.blockhash, Some(old_hash));
    assert_eq!(info.confirmations.unwrap_or(0), 0);

    // The entry stays on the watch list, pending, however many ticks read NotFound.
    for _ in 0..3 {
        assert!(!indexer.tick()?);
    }
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, None)]);

    // The old chain has more work, and wins again.
    node.reconsider(&old_hash)?;
    node.sync(&indexer)?;

    assert_eq!(node.hash_at(height)?, old_hash);
    assert_eq!(
        indexer.get_transaction(&tx_id, false)?,
        confirmed(&node, &tx_id, height, 2)?
    );
    assert_eq!(
        indexer.get_transaction(&conflicting_id, true)?,
        TransactionStatus::NotFound
    );
    assert_eq!(store.get_mempool_watch_list()?, vec![(tx_id, Some(height))]);

    Ok(())
}

// Unwatched and unknown transactions, a replacement by fee, and entries that stay until the consumer removes them.
#[test]
fn mempool_watch() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let (replacement, original) = node.conflicting_txs()?;
    let replacement_id = replacement.compute_txid();
    let original_id = original.compute_txid();

    let indexer = node.indexer(storage.storage(), 10, true)?;
    node.sync(&indexer)?;

    // An unwatched transaction in the mempool is answered by the node, and the flag decides whether it is reported.
    // The indexer holds nothing about it, since only watched txids reach the snapshot.
    let unwatched = node.send(10_000)?;
    for tick in [false, true] {
        if tick {
            indexer.tick()?;
        }
        assert_eq!(
            indexer.get_stored_transaction(&unwatched, true)?,
            TransactionStatus::NotFound
        );
        assert_eq!(
            indexer.get_transaction(&unwatched, true)?,
            TransactionStatus::InMempool
        );
        assert_eq!(
            indexer.get_transaction(&unwatched, false)?,
            TransactionStatus::NotFound
        );
    }

    // A transaction the node never saw.
    let unknown = dummy_tx(1).compute_txid();
    assert_eq!(
        indexer.get_transaction(&unknown, true)?,
        TransactionStatus::NotFound
    );

    // A replacement by fee: the original reads NotFound once it is replaced, and the replacement is confirmed.
    indexer.add_mempool_watch(original_id)?;
    indexer.add_mempool_watch(replacement_id)?;

    node.client.send_transaction(&original)?;
    indexer.tick()?;
    assert_eq!(
        indexer.get_transaction(&original_id, true)?,
        TransactionStatus::InMempool
    );

    node.client.send_transaction(&replacement)?;
    indexer.tick()?;
    assert_eq!(
        indexer.get_transaction(&original_id, true)?,
        TransactionStatus::NotFound
    );
    assert_eq!(
        indexer.get_transaction(&replacement_id, true)?,
        TransactionStatus::InMempool
    );

    node.mine(1)?;
    node.sync(&indexer)?;
    let height = indexer.get_indexed_height()?;
    assert_eq!(
        indexer.get_transaction(&replacement_id, false)?,
        confirmed(&node, &replacement_id, height, 1)?
    );
    assert_eq!(
        indexer.get_transaction(&original_id, true)?,
        TransactionStatus::NotFound
    );

    // The indexer never removes an entry on its own, only remove_mempool_watch does.
    for _ in 0..5 {
        indexer.tick()?;
    }
    assert_eq!(
        store.get_mempool_watch_list()?,
        vec![(original_id, None), (replacement_id, Some(height))]
    );
    indexer.remove_mempool_watch(&original_id)?;
    assert_eq!(
        store.get_mempool_watch_list()?,
        vec![(replacement_id, Some(height))]
    );

    Ok(())
}

// =============================================================================
// The window boundary
// =============================================================================

// A transaction in the oldest held block is answered from storage, and once that block is pruned, by the node with the same answer.
#[test]
fn window_boundary() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    let indexer = node.indexer(storage.storage(), 3, true)?;
    node.sync(&indexer)?;

    let tx_id = node.send(10_000)?;
    node.mine(3)?;
    node.sync(&indexer)?;

    // Block 111 is the oldest of the three held blocks, so the indexer answers on its own.
    assert_eq!(store.get_tx_height(&tx_id)?, Some(111));
    for include_mempool in [false, true] {
        assert_eq!(
            indexer.get_stored_transaction(&tx_id, include_mempool)?,
            confirmed(&node, &tx_id, 111, 3)?
        );
        assert_eq!(
            indexer.get_transaction(&tx_id, include_mempool)?,
            confirmed(&node, &tx_id, 111, 3)?
        );
    }

    // One more block prunes it. The indexer no longer holds it, and only the node can answer.
    node.mine(1)?;
    node.sync(&indexer)?;
    assert_eq!(store.get_block(111)?, None);
    assert_eq!(store.get_tx_height(&tx_id)?, None);
    for include_mempool in [false, true] {
        assert_eq!(
            indexer.get_stored_transaction(&tx_id, include_mempool)?,
            TransactionStatus::NotFound
        );
        assert_eq!(
            indexer.get_transaction(&tx_id, include_mempool)?,
            confirmed(&node, &tx_id, 111, 4)?
        );
    }

    Ok(())
}

// get_block answers from storage, from the node below the window, and with None for a block the indexer has not processed.
#[test]
fn get_block() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();
    let store = storage.store();

    // Block 100 is replaced before the indexer starts, so the old one is a block the node no longer has on its chain.
    let stale = node.invalidate(100)?;
    node.mine(11)?;

    let indexer = node.indexer(storage.storage(), 3, true)?;
    node.sync(&indexer)?;

    // Below the window, a hash is only accepted at its own height and on the node's chain.
    assert_eq!(indexer.get_block(50, &node.hash_at(51)?)?, None);
    assert_eq!(indexer.get_block(100, &stale)?, None);
    assert!(indexer.get_block(100, &node.hash_at(100)?)?.is_some());

    // A held block with its hash, and a held height asked with another hash. Both are answered from storage alone.
    assert_eq!(
        indexer.get_stored_block(110, &node.hash_at(110)?)?,
        store.get_block(110)?
    );
    assert_eq!(
        indexer.get_block(110, &node.hash_at(110)?)?,
        store.get_block(110)?
    );
    assert_eq!(indexer.get_stored_block(110, &node.hash_at(109)?)?, None);
    assert_eq!(indexer.get_block(110, &node.hash_at(109)?)?, None);

    // A block below the window is not held, so only get_block can answer.
    assert_eq!(indexer.get_stored_block(50, &node.hash_at(50)?)?, None);

    // A block the indexer has not reached yet.
    node.mine(1)?;
    assert_eq!(indexer.get_stored_block(111, &node.hash_at(111)?)?, None);
    assert_eq!(indexer.get_block(111, &node.hash_at(111)?)?, None);

    // Blocks below the window, including genesis and low heights, are downloaded with the node's content.
    for height in [0, 1, 50] {
        let hash = node.hash_at(height)?;
        let block = indexer
            .get_block(height, &hash)?
            .unwrap_or_else(|| panic!("block {height} below the window"));
        let node_block = node.block_at(height)?;

        assert_eq!(block.height, height);
        assert_eq!(block.hash, hash);
        assert_eq!(block.prev_hash, node_block.header.prev_blockhash);
        assert_eq!(block.txs, node_block.txdata);
        assert_eq!(block.estimated_fee_rate, 0);

        // The transactions rebuild the header's merkle root.
        let rebuilt = Block {
            header: node_block.header,
            txdata: block.txs,
        };
        assert!(rebuilt.check_merkle_root());
    }

    Ok(())
}

// =============================================================================
// Fee rate
// =============================================================================

// The fee rate is the one of the middle transaction of the last indexed block, and is refused when it cannot be trusted.
#[test]
fn fee_rate() -> anyhow::Result<()> {
    init_trace();
    let node = TestNode::start(110)?;
    let storage = TestStorage::new();

    // Seven outputs to spend, one per fee rate.
    let outpoints = (0..7)
        .map(|_| node.fund_utxo(100_000))
        .collect::<anyhow::Result<Vec<_>>>()?;

    let indexer = node.indexer(storage.storage(), 3, true)?;
    node.sync(&indexer)?;

    // A block with only its coinbase has no fee rate.
    assert!(matches!(
        indexer.get_estimated_fee_rate(),
        Err(IndexerError::FeeRateNotEstimated)
    ));

    // A node block the indexer has not reached.
    node.mine(1)?;
    assert!(matches!(
        indexer.get_estimated_fee_rate(),
        Err(IndexerError::NotSynced)
    ));
    indexer.tick()?;
    assert!(matches!(
        indexer.get_estimated_fee_rate(),
        Err(IndexerError::FeeRateNotEstimated)
    ));

    // A block with the coinbase and seven transactions paying 10 to 70 sat/vB, in that order. The middle one pays 40.
    let mut txs = Vec::new();
    for (outpoint, fee_rate) in outpoints.into_iter().zip([10u64, 20, 30, 40, 50, 60, 70]) {
        // Sign to learn the size, then again with the exact fee for that size, until the signature keeps the size.
        let mut vsize = node.sign_spend(outpoint, 99_000)?.vsize() as u64;
        let tx = loop {
            let tx = node.sign_spend(outpoint, 100_000 - fee_rate * vsize)?;
            if tx.vsize() as u64 == vsize {
                break tx;
            }
            vsize = tx.vsize() as u64;
        };
        txs.push(tx);
    }
    node.mine_with(&txs)?;
    node.sync(&indexer)?;

    let block = indexer.get_last_indexed_block()?;
    assert_eq!(block.txs.len(), 8);
    assert!(block.txs[0].is_coinbase());
    assert_eq!(block.estimated_fee_rate, 40);
    assert_eq!(indexer.get_estimated_fee_rate()?, 40);

    Ok(())
}
