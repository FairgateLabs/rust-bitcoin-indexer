use bitcoin::BlockHash;
use bitcoin_indexer::{
    config::IndexerSettings,
    indexer::{Indexer, IndexerApi},
    store::StoreClient,
};
use bitvmx_bitcoin_rpc::{bitcoin_client::MockBitcoinClient, types::*};
mod utils;
use crate::utils::clear_output;
use mockall::predicate::eq;
use std::str::FromStr;
use utils::get_indexer_store;

#[test]
fn test_get_estimated_fee_rate_with_seven_transactions() -> Result<(), anyhow::Error> {
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Transaction, TxIn,
        TxOut,
    };
    use bitcoin_indexer::indexer::{Indexer, IndexerApi};
    use serde_json::json;

    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    let hash =
        BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash =
        BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;

    // Create 7 transactions with different fee rates
    // Transaction with feerate 1 sat/vbyte: fee = 1 sat, vsize = 1 vbyte
    let tx1 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(1000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 2 sat/vbyte: fee = 2 sat, vsize = 1 vbyte
    let tx2 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000002",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(2000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 3 sat/vbyte: fee = 3 sat, vsize = 1 vbyte
    let tx3 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000003",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(3000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 4 sat/vbyte: fee = 4 sat, vsize = 1 vbyte (this will be the middle transaction)
    let tx4 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000004",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(4000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 5 sat/vbyte: fee = 5 sat, vsize = 1 vbyte
    let tx5 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000005",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(5000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 6 sat/vbyte: fee = 6 sat, vsize = 1 vbyte
    let tx6 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000006",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(6000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    // Transaction with feerate 7 sat/vbyte: fee = 7 sat, vsize = 1 vbyte
    let tx7 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000007",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(7000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    let block_info = BlockInfo {
        height: 1000,
        hash: hash.clone(),
        prev_hash: prev_hash.clone(),
        txs: vec![
            tx1.clone(),
            tx2.clone(),
            tx3.clone(),
            tx4.clone(),
            tx5.clone(),
            tx6.clone(),
            tx7.clone(),
        ],
    };

    // The middle transaction (index 3) is tx4, which should have feerate 4 sat/vbyte
    let middle_tx_id = tx4.compute_txid();

    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(1000));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_info.clone())));

    // Mock the RPC call for the middle transaction (tx4) to return fee=4 sat, vsize=1 vbyte
    bitcoin_client
        .expect_get_raw_transaction_verbosity_two()
        .with(eq(middle_tx_id))
        .returning(move |_| {
            Ok(json!({
                "fee": 0.00000004, // 4 satoshis in BTC
                "vsize": 1
            }))
        });

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(1000))),
    )?;

    // First tick should process the block and calculate the estimated fee rate
    indexer.tick()?;

    // Get the block and check that the estimated fee rate is 4 (from the middle transaction)
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block.unwrap().estimated_fee_rate, 4);

    // Also test the get_estimated_fee_rate method
    let estimated_fee_rate = indexer.get_estimated_fee_rate()?;
    assert_eq!(estimated_fee_rate, 4);

    clear_output();
    Ok(())
}

#[test]
fn test_get_estimated_fee_rate_indexer_not_synced() -> Result<(), anyhow::Error> {
    use bitcoin::{absolute::LockTime, transaction::Version, Transaction};
    use bitcoin_indexer::errors::IndexerError;

    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    let hash =
        BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash =
        BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;

    // Create a simple transaction for the block
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    let block_info = BlockInfo {
        height: 1000,
        hash: hash.clone(),
        prev_hash: prev_hash.clone(),
        txs: vec![tx],
    };

    // Set up the indexer to be at height 1000
    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(1000));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_info.clone())));

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(1000))),
    )?;

    // Process the block so the indexer is at height 1000
    indexer.tick()?;

    // Now mock the bitcoin client to return a different blockchain height (1001)
    // This will create a scenario where indexer height (1000) != blockchain height (1001)
    let mut new_bitcoin_client = MockBitcoinClient::new();
    new_bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(1001)); // Blockchain is ahead of indexer

    // Create a new indexer instance with the updated bitcoin client
    let new_indexer = Indexer {
        bitcoin_client: new_bitcoin_client,
        store: indexer.store.clone(),
    };

    // Try to get estimated fee rate when indexer is not synced
    let result = new_indexer.get_estimated_fee_rate();

    // Should return IndexerError::IndexerNotSynced
    assert!(result.is_err());
    match result.unwrap_err() {
        IndexerError::IndexerNotSynced => {
            // This is expected - test passes
        }
        other_error => {
            panic!(
                "Expected IndexerError::IndexerNotSynced, but got: {:?}",
                other_error
            );
        }
    }

    clear_output();
    Ok(())
}

#[test]
fn test_get_estimated_fee_rate_not_estimated() -> Result<(), anyhow::Error> {
    use bitcoin::{
        absolute::LockTime, transaction::Version, Amount, OutPoint, ScriptBuf, Transaction, TxIn,
        TxOut,
    };
    use bitcoin_indexer::errors::IndexerError;

    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    let hash =
        BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash =
        BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;

    // Create only 2 transactions - this will be less than MIN_BLOCK_TX (5)
    // This will cause the fee estimation to return 0 (ERROR_FEE_RATE)
    let tx1 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(1000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    let tx2 = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::new(
                bitcoin::hash_types::Txid::from_str(
                    "0000000000000000000000000000000000000000000000000000000000000002",
                )?,
                0,
            ),
            script_sig: ScriptBuf::new(),
            sequence: bitcoin::transaction::Sequence::ENABLE_RBF_NO_LOCKTIME,
            witness: bitcoin::Witness::new(),
        }],
        output: vec![TxOut {
            value: Amount::from_sat(2000),
            script_pubkey: ScriptBuf::new(),
        }],
    };

    let block_info = BlockInfo {
        height: 1000,
        hash: hash.clone(),
        prev_hash: prev_hash.clone(),
        txs: vec![tx1, tx2], // Only 2 transactions - less than MIN_BLOCK_TX (5)
    };

    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(1000));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_info.clone())));

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(1000))),
    )?;

    // Process the block - this will save the block with estimated_fee_rate = 0
    // because the block has only 2 transactions (less than MIN_BLOCK_TX = 5)
    indexer.tick()?;

    // Verify the block was saved with estimated_fee_rate = 0
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block.unwrap().estimated_fee_rate, 0);

    // Now try to get estimated fee rate - should return IndexerError::FeeRateNotEstimated
    // because the estimated_fee_rate is 0
    let result = indexer.get_estimated_fee_rate();

    // Should return IndexerError::FeeRateNotEstimated
    assert!(result.is_err());
    match result.unwrap_err() {
        IndexerError::FeeRateNotEstimated => {
            // This is expected - test passes
        }
        other_error => {
            panic!(
                "Expected IndexerError::FeeRateNotEstimated, but got: {:?}",
                other_error
            );
        }
    }

    clear_output();
    Ok(())
}
