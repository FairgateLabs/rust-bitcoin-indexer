use bitcoin::BlockHash;
use bitcoin_indexer::{
    config::IndexerSettings,
    errors::IndexerError,
    indexer::{Indexer, IndexerApi},
    store::StoreClient,
    types::FullBlock,
};
use bitvmx_bitcoin_rpc::{bitcoin_client::MockBitcoinClient, types::*};
mod utils;
use crate::utils::clear_output;
use mockall::predicate::eq;
use std::str::FromStr;
use utils::get_indexer_store;

#[test]
fn test_get_best_block() -> Result<(), anyhow::Error> {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    let hash =
        BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash =
        BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;
    let full_block = FullBlock {
        height: 1000,
        hash: hash.clone(),
        orphan: false,
        prev_hash: prev_hash.clone(),
        txs: vec![],
        estimated_fee_rate: 0,
    };

    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(1000));

    let block_info = BlockInfo {
        height: 1000,
        hash: hash.clone(),
        prev_hash: prev_hash.clone(),
        txs: vec![],
    };

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_info.clone())));

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(1000))),
    )?;
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block.unwrap().height, 1000);

    indexer.tick()?;
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block, Some(full_block));

    clear_output();
    Ok(())
}

#[test]
fn indexer_constructor_checkpoint_variants() -> Result<(), anyhow::Error> {
    use crate::utils::get_indexer_store;
    use bitcoin::{absolute::LockTime, transaction::Version, BlockHash, Transaction};
    use bitcoin_indexer::indexer::{Indexer, IndexerApi};
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitvmx_bitcoin_rpc::types::BlockInfo;
    use mockall::predicate::eq;
    use std::str::FromStr;

    // Setup block hashes
    let hash_10 =
        BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000010")?;
    let hash_11 =
        BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000011")?;
    let hash_12 =
        BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000012")?;
    let prev_hash_9 =
        BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000009")?;

    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    let block_0 = BlockInfo {
        height: 0,
        hash: hash_10.clone(),
        prev_hash: prev_hash_9.clone(),
        txs: vec![tx.clone()],
    };

    // Block at height 10
    let block_10 = BlockInfo {
        height: 10,
        hash: hash_10,
        prev_hash: prev_hash_9,
        txs: vec![tx.clone()],
    };

    let block_10_clone = block_10.clone();

    // Block at height 11
    let block_11 = BlockInfo {
        height: 11,
        hash: hash_11,
        prev_hash: hash_10,
        txs: vec![],
    };

    // Block at height 12
    let block_12 = BlockInfo {
        height: 12,
        hash: hash_12,
        prev_hash: hash_11,
        txs: vec![],
    };

    // 1. No indexed block, no checkpoint (should start from genesis)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(0))
            .returning(move |_| Ok(Some(block_0.clone())));

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        // Should have saved height_to_sync = 0
        assert_eq!(indexer.get_best_height()?, Some(0));
    }

    // 2. No indexed block, checkpoint = 11 (should start from 11)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        let block_11_clone = block_11.clone();

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(11))
            .returning(move |_| Ok(Some(block_11_clone.clone())));

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(11))))?;
        assert_eq!(indexer.get_best_height()?, Some(11));
    }

    // 3. No indexed block, checkpoint > blockchain height (should error)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(10));

        let block_10_clone = block_10.clone();

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_clone.clone())));

        let result = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(20))));
        assert!(result.is_err());
    }

    // 4. Indexed block exists, checkpoint is None (should start from indexed height)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        // Save block_10 as already indexed
        store.save_new_best_block(&block_10.clone(), 0)?;

        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        let block_10_clone = block_10.clone();

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_clone.clone())));

        store.save_best_height(10)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        assert_eq!(indexer.get_best_height()?, Some(10));
    }

    // 5. Indexed block exists, checkpoint does not exist in the database and passing a checkpoint height (should use indexed height) and warn user
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_new_best_block(&block_11, 0)?;
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(11))
            .returning(move |_| Ok(Some(block_11.clone())));

        let block_10_clone = block_10.clone();

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_clone.clone())));

        store.save_best_height(11)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(10))))?;
        assert_eq!(indexer.get_best_height()?, Some(11));
    }

    // 6. Indexed block exists, checkpoint exist and is different from the previous checkpoint height (should error)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_checkpoint_height(10)?;

        store.save_new_best_block(&block_10_clone, 0)?;
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        let block_10_copy = block_10.clone();
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_copy.clone())));

        store.save_best_height(10)?;

        let block_12_clone = block_12.clone();
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(12))
            .returning(move |_| Ok(Some(block_12_clone.clone())));

        let result = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))));
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(IndexerError::AlreadyIndexedWithDifferentCheckpointHeight)
        ));
    }

    // 7. Indexed block exists, checkpoint == indexed height (should use indexed height)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_new_best_block(&block_12, 0)?;
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(12))
            .returning(move |_| Ok(Some(block_12.clone())));
        store.save_best_height(12)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))))?;
        assert_eq!(indexer.get_best_height()?, Some(12));
    }

    clear_output();
    Ok(())
}

#[test]
fn test_orphan_block_not_marked_during_reorg() -> Result<(), anyhow::Error> {
    use bitcoin::{absolute::LockTime, transaction::Version};

    // Initialize tracing to see warn! and info! output
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();
    // Setup: Create blocks at heights 9 and 10
    let hash_9 =
        BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000009")?;
    let hash_10_original =
        BlockHash::from_str("000000000000000000000000000000000000000000000000000000000000000a")?;
    let hash_10_reorg =
        BlockHash::from_str("000000000000000000000000000000000000000000000000000000000000010a")?;
    let hash_11 =
        BlockHash::from_str("000000000000000000000000000000000000000000000000000000000000010b")?;
    let tx = bitcoin::Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };
    let block_9 = BlockInfo {
        height: 9,
        hash: hash_9,
        prev_hash: BlockHash::all_zeros(),
        txs: vec![tx.clone()],
    };
    let block_10_original = BlockInfo {
        height: 10,
        hash: hash_10_original,
        prev_hash: hash_9,
        txs: vec![tx.clone()],
    };
    let block_10_reorg = BlockInfo {
        height: 10,
        hash: hash_10_reorg,
        prev_hash: hash_9,
        txs: vec![tx.clone()],
    };
    let block_11 = BlockInfo {
        height: 11,
        hash: hash_11,
        prev_hash: hash_10_original,
        txs: vec![],
    };
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();
    bitcoin_client.expect_get_best_block().returning(|| Ok(11));

    let block_9_clone = block_9.clone();
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(9))
        .returning(move |_| Ok(Some(block_9_clone.clone())));
    // First tick: return original block 10
    let block_10_clone1 = block_10_original.clone();
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(10))
        .times(1)
        .returning(move |_| Ok(Some(block_10_clone1.clone())));
    // Second tick: return different block 10 (reorg)
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(10))
        .returning(move |_| Ok(Some(block_10_reorg.clone())));
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(11))
        .returning(move |_| Ok(Some(block_11.clone())));

    let indexer = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(9))),
    )?;
    // Tick 1: Sync block 10 (original)
    indexer.tick()?;
    assert_eq!(indexer.get_best_height()?, Some(10));
    let block_at_10: FullBlock = store
        .get_block_by_height(10)?
        .expect("Block 10 should exist");
    assert_eq!(block_at_10.hash, hash_10_original);
    assert_eq!(
        block_at_10.orphan, false,
        "Block 10 should not be orphan initially"
    );

    // Tick 2: Detect reorg (different block at height 10)
    indexer.tick()?;

    // Should have rolled back to height 9
    assert_eq!(indexer.get_best_height()?, Some(9));

    let orphaned_block = store.get_block_by_height(10)?;

    if let Some(block) = orphaned_block {
        assert_eq!(block.hash, hash_10_original);
        assert_eq!(block.orphan, true, "Orphaned block still has orphan=true");
    }

    let orphaned_by_hash = store.get_block_by_hash(&hash_10_original)?;
    if let Some(block) = orphaned_by_hash {
        assert_eq!(
            block.orphan, true,
            "get_block_by_hash also returns orphan=true"
        );
    }

    // Tick 3: Sync new block 10 and block 11
    indexer.tick()?;

    assert_eq!(indexer.get_best_height()?, Some(10));

    let block_at_10: FullBlock = store
        .get_block_by_height(10)?
        .expect("Block 10 should exist");
    assert_eq!(block_at_10.hash, hash_10_reorg);
    assert_eq!(
        block_at_10.orphan, false,
        "New Block 10 should not be orphan"
    );

    indexer.tick()?;

    assert_eq!(indexer.get_best_height()?, Some(11));

    let block_at_11: FullBlock = store
        .get_block_by_height(11)?
        .expect("Block 11 should exist");
    assert_eq!(block_at_11.hash, hash_11);
    assert_eq!(block_at_11.orphan, false, "Block 11 should not be orphan");

    clear_output();
    Ok(())
}
