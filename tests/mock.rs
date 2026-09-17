//! Cases that a regtest node cannot produce on demand, run against a mocked node.
mod common;
use common::*;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use bitcoin::consensus::serialize;
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Transaction};
use bitcoin_indexer::{FullBlock, Indexer, IndexerError, TransactionStatus};
use bitcoincore_rpc::json::{GetBlockHeaderResult, GetRawTransactionResult};
use bitvmx_bitcoin_rpc::{
    bitcoin_client::MockBitcoinClientApi,
    errors::BitcoinClientError,
    types::{BlockHeight, BlockInfo},
};

fn block_hash(seed: u8) -> BlockHash {
    BlockHash::from_byte_array([seed; 32])
}

/// The block the mocked chain has at `height`: its hash is the height, and its parent is the height below.
fn chain_block(height: BlockHeight, txs: Vec<Transaction>) -> BlockInfo {
    BlockInfo {
        height,
        hash: block_hash(height as u8),
        prev_hash: block_hash(height.saturating_sub(1) as u8),
        txs,
    }
}

fn full_block(height: BlockHeight, txs: Vec<Transaction>) -> FullBlock {
    let block = chain_block(height, txs);
    FullBlock {
        height,
        hash: block.hash,
        prev_hash: block.prev_hash,
        txs: block.txs,
        estimated_fee_rate: 0,
    }
}

/// A node with -txindex whose tip is at `tip`. Every other call panics unless the test expects it.
fn mock_node(tip: BlockHeight) -> MockBitcoinClientApi {
    let mut node = MockBitcoinClientApi::new();
    node.expect_is_txindex_enabled().returning(|| Ok(true));
    node.expect_get_tip_height().returning(move || Ok(tip));
    node
}

fn rpc_error() -> BitcoinClientError {
    BitcoinClientError::FailedToGetTransactionDetails {
        error: "connection refused".to_string(),
    }
}

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

// Invalid setups are refused before anything is written.
#[test]
fn new_rejects_invalid_setup() {
    let storage = TestStorage::new();

    // A node without -txindex.
    let mut node = MockBitcoinClientApi::new();
    node.expect_is_txindex_enabled().returning(|| Ok(false));
    let result = Indexer::new(node, storage.store(), settings(5, true));
    assert!(matches!(result, Err(IndexerError::InvalidConfiguration(_))));

    // A retention depth below the minimum. The node has no expectations, so asking it anything panics.
    let result = Indexer::new(
        MockBitcoinClientApi::new(),
        storage.store(),
        settings(1, true),
    );
    assert!(matches!(result, Err(IndexerError::InvalidConfiguration(_))));

    assert_eq!(storage.store().get_cursor().unwrap(), None);
    assert_eq!(storage.store().get_block(0).unwrap(), None);
}

// A mempool snapshot from the previous run is not used: the node answers until the first tick refreshes it.
#[test]
fn restart_drops_mempool_snapshot() {
    let storage = TestStorage::new();
    let store = storage.store();
    let tx_id = dummy_tx(1).compute_txid();
    store.save_block(&full_block(10, vec![])).unwrap();
    store.save_cursor(10).unwrap();
    store.add_mempool_watch(tx_id).unwrap();
    store.save_mempool_snapshot(vec![tx_id]).unwrap();

    // While the indexer was down the node dropped the transaction, which is what step 3 answers.
    let mut node = mock_node(10);
    node.expect_get_raw_transaction_info()
        .returning(|_| Ok(None));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();

    assert!(!store.is_in_mempool_snapshot(&tx_id).unwrap());
    assert_eq!(
        indexer.get_transaction(&tx_id, true).unwrap(),
        TransactionStatus::NotFound
    );
    // The watch list keeps its entries, so no node calls are added.
    assert_eq!(store.get_mempool_watch_list().unwrap(), vec![(tx_id, None)]);
}

// A next block that does not build on the block at the cursor is not stored. This is the node switching chains.
#[test]
fn tick_prev_hash_mismatch() {
    let storage = TestStorage::new();
    let store = storage.store();
    store.save_block(&full_block(10, vec![])).unwrap();
    store.save_cursor(10).unwrap();

    let mut node = mock_node(11);
    node.expect_get_block_by_height()
        .returning(|height| match *height {
            // The block at the cursor still matches, so no reorg is visible yet.
            10 => Ok(Some(chain_block(10, vec![]))),
            // The next block comes from another chain: its parent is not the stored block.
            _ => Ok(Some(BlockInfo {
                prev_hash: block_hash(99),
                ..chain_block(11, vec![])
            })),
        });
    node.expect_check_in_mempool().returning(|_| Ok(false));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();

    assert!(!indexer.tick().unwrap());
    assert_eq!(indexer.get_indexed_height().unwrap(), 10);
    assert_eq!(store.get_block(11).unwrap(), None);
}

// Node answers to step 3 of get_transaction.
#[test]
fn get_transaction_from_node() {
    let tx = dummy_tx(42);
    let tx_id = tx.compute_txid();

    // Each case is the node's transaction answer, the height its header call returns, and the expected status
    // with and without include_mempool. A case with no header height panics if the header is asked for.
    let cases = [
        // The node's transaction index still points at a block a reorg removed, with zero confirmations.
        (
            raw_tx_info(&tx, Some(BlockHash::all_zeros()), Some(0)),
            None,
            TransactionStatus::NotFound,
            TransactionStatus::NotFound,
        ),
        // The node's new chain has the transaction at height 10, where the indexer still holds the old block.
        (
            raw_tx_info(&tx, Some(block_hash(77)), Some(1)),
            Some(10),
            TransactionStatus::InMempool,
            TransactionStatus::NotFound,
        ),
    ];

    for (answer, header_height, with_mempool, without_mempool) in cases {
        let storage = TestStorage::new();
        storage.store().save_block(&full_block(10, vec![])).unwrap();
        storage.store().save_cursor(10).unwrap();

        let mut node = mock_node(10);
        node.expect_get_raw_transaction_info()
            .returning(move |_| Ok(Some(answer.clone())));
        if let Some(height) = header_height {
            node.expect_get_block_header_info()
                .returning(move |hash| Ok(header_at(height, *hash)));
        }

        let indexer = Indexer::new(node, storage.store(), settings(5, true)).unwrap();

        assert_eq!(indexer.get_transaction(&tx_id, true).unwrap(), with_mempool);
        assert_eq!(
            indexer.get_transaction(&tx_id, false).unwrap(),
            without_mempool
        );
    }

    // A node that fails to answer, on the transaction lookup or on the header call,
    // returns the error instead of NotFound.
    for header_fails in [false, true] {
        let storage = TestStorage::new();
        storage.store().save_block(&full_block(10, vec![])).unwrap();
        storage.store().save_cursor(10).unwrap();

        let mut node = mock_node(10);
        let answer = raw_tx_info(&tx, Some(block_hash(5)), Some(6));
        node.expect_get_raw_transaction_info()
            .returning(move |_| match header_fails {
                true => Ok(Some(answer.clone())),
                false => Err(rpc_error()),
            });
        node.expect_get_block_header_info()
            .returning(|_| Err(rpc_error()));

        let indexer = Indexer::new(node, storage.store(), settings(5, true)).unwrap();

        for include_mempool in [false, true] {
            assert!(matches!(
                indexer.get_transaction(&tx_id, include_mempool),
                Err(IndexerError::BitcoinClientError(_))
            ));
        }
    }

    // A transaction the node does not know.
    let storage = TestStorage::new();
    storage.store().save_block(&full_block(10, vec![])).unwrap();
    storage.store().save_cursor(10).unwrap();

    let mut node = mock_node(10);
    node.expect_get_raw_transaction_info()
        .returning(|_| Ok(None));

    let indexer = Indexer::new(node, storage.store(), settings(5, true)).unwrap();

    assert_eq!(
        indexer.get_transaction(&tx_id, true).unwrap(),
        TransactionStatus::NotFound
    );
}

// A watched transaction whose mempool check fails makes the tick fail, instead of reading as not in the mempool.
#[test]
fn mempool_check_error() {
    let storage = TestStorage::new();
    let store = storage.store();
    store.save_block(&full_block(10, vec![])).unwrap();
    store.save_cursor(10).unwrap();
    store.add_mempool_watch(dummy_tx(1).compute_txid()).unwrap();

    let mut node = mock_node(10);
    node.expect_get_block_by_height()
        .returning(|height| Ok(Some(chain_block(*height, vec![]))));
    node.expect_check_in_mempool()
        .returning(|_| Err(rpc_error()));

    let indexer = Indexer::new(node, store, settings(5, true)).unwrap();

    assert!(matches!(
        indexer.tick(),
        Err(IndexerError::BitcoinClientError(_))
    ));
}

// A tick that was interrupted, by a crash or by a failed node call, leaves a state the next start or tick continues from.
#[test]
fn interrupted_tick_recovers() {
    // A crash after the block was written and before the cursor moved: the block is written again and the cursor advances.
    let storage = TestStorage::new();
    let store = storage.store();
    store.save_block(&full_block(10, vec![])).unwrap();
    store.save_block(&full_block(11, vec![])).unwrap();
    store.save_cursor(10).unwrap();

    let mut node = mock_node(11);
    node.expect_get_block_by_height()
        .returning(|height| Ok(Some(chain_block(*height, vec![]))));
    node.expect_check_in_mempool().returning(|_| Ok(false));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();
    assert_eq!(indexer.get_indexed_height().unwrap(), 10);

    assert!(indexer.tick().unwrap());
    assert_eq!(indexer.get_indexed_height().unwrap(), 11);
    assert_eq!(store.get_block(11).unwrap(), Some(full_block(11, vec![])));
    drop(indexer);

    // A crash on the very first start, after block 0 was written and before the cursor was saved: a start with no
    // cursor is a fresh start, which writes block 0 again and saves the cursor.
    let storage = TestStorage::new();
    let store = storage.store();
    store.save_block(&full_block(0, vec![])).unwrap();

    let mut node = mock_node(3);
    node.expect_get_block_by_height()
        .returning(|height| Ok(Some(chain_block(*height, vec![]))));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();
    assert_eq!(indexer.get_indexed_height().unwrap(), 0);
    assert_eq!(store.get_block(0).unwrap(), Some(full_block(0, vec![])));
    drop(indexer);

    // The fee estimation of the next block fails once: the tick returns the error and stores nothing, and the next
    // tick indexes the block. Seven transactions make the estimation ask the node about the middle one.
    let storage = TestStorage::new();
    let store = storage.store();
    store.save_block(&full_block(10, vec![])).unwrap();
    store.save_cursor(10).unwrap();

    let txs: Vec<Transaction> = (0..7).map(dummy_tx).collect();
    let next_txs = txs.clone();
    let mut node = mock_node(11);
    node.expect_get_block_by_height()
        .returning(move |height| match *height {
            10 => Ok(Some(chain_block(10, vec![]))),
            _ => Ok(Some(chain_block(11, next_txs.clone()))),
        });
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_node = calls.clone();
    node.expect_get_raw_transaction_verbosity_two()
        .returning(
            move |_| match calls_in_node.fetch_add(1, Ordering::Relaxed) {
                0 => Err(rpc_error()),
                _ => Ok(serde_json::json!({ "fee": 0.00001, "vsize": 200 })),
            },
        );
    node.expect_check_in_mempool().returning(|_| Ok(false));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();

    assert!(matches!(
        indexer.tick(),
        Err(IndexerError::BitcoinClientError(_))
    ));
    assert_eq!(indexer.get_indexed_height().unwrap(), 10);
    assert_eq!(store.get_block(11).unwrap(), None);
    assert_eq!(store.get_tx_height(&txs[0].compute_txid()).unwrap(), None);

    assert!(indexer.tick().unwrap());
    assert_eq!(indexer.get_indexed_height().unwrap(), 11);
    assert_eq!(store.get_block(11).unwrap().unwrap().estimated_fee_rate, 5);
    assert_eq!(
        store.get_tx_height(&txs[0].compute_txid()).unwrap(),
        Some(11)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}

// Storage that contradicts itself is reported as an error, never answered around.
#[test]
fn inconsistent_storage_errors() {
    let storage = TestStorage::new();
    let store = storage.store();

    // A transaction's height entry points at a block that does not contain it: block 9 was overwritten.
    let tx = dummy_tx(1);
    store.save_block(&full_block(9, vec![tx.clone()])).unwrap();
    store.save_block(&full_block(9, vec![])).unwrap();

    // A cursor with no block at its height.
    store.save_cursor(10).unwrap();

    // No block reads are expected: the indexer must fail before asking the node for one.
    let indexer = Indexer::new(mock_node(12), store.clone(), settings(5, true)).unwrap();

    assert!(matches!(
        indexer.tick(),
        Err(IndexerError::BlockNotFound(10))
    ));
    assert!(matches!(
        indexer.get_last_indexed_block(),
        Err(IndexerError::BlockNotFound(10))
    ));
    assert!(matches!(
        indexer.get_estimated_fee_rate(),
        Err(IndexerError::BlockNotFound(10))
    ));
    assert!(matches!(
        indexer.get_transaction(&tx.compute_txid(), true),
        Err(IndexerError::InvariantViolation(_))
    ));
    assert_eq!(indexer.get_indexed_height().unwrap(), 10);
}

// A reorg of a block with many transactions only touches the mempool watch list entries it confirmed, and asks the
// node about each watched entry once, not about every transaction of the block.
#[test]
fn reorg_of_big_block() {
    let storage = TestStorage::new();
    let store = storage.store();

    let txs: Vec<Transaction> = (0..300).map(dummy_tx).collect();
    let watched = txs[150].compute_txid();
    store.save_block(&full_block(9, vec![])).unwrap();
    store.save_block(&full_block(10, txs.clone())).unwrap();
    store.save_cursor(10).unwrap();
    store
        .save_mempool_watch_list(vec![(watched, Some(10))])
        .unwrap();

    let mut node = mock_node(10);
    // The node's chain has a different block at height 10.
    node.expect_get_block_by_height().returning(|height| {
        Ok(Some(BlockInfo {
            hash: block_hash(77),
            ..chain_block(*height, vec![])
        }))
    });
    node.expect_check_in_mempool()
        .times(1)
        .returning(|_| Ok(true));

    let indexer = Indexer::new(node, store.clone(), settings(5, true)).unwrap();

    assert!(!indexer.tick().unwrap());
    assert_eq!(indexer.get_indexed_height().unwrap(), 9);
    assert_eq!(
        store.get_mempool_watch_list().unwrap(),
        vec![(watched, None)]
    );
    assert!(txs
        .iter()
        .all(|tx| store.get_tx_height(&tx.compute_txid()).unwrap().is_none()));
    assert_eq!(
        indexer.get_transaction(&watched, true).unwrap(),
        TransactionStatus::InMempool
    );
}
