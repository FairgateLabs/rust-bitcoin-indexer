use bitcoin::BlockHash;
use bitcoin_indexer::{
    config::IndexerSettings,
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
        store.save_new_best_block(&block_10.clone())?;

        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        let block_10_clone = block_10.clone();

        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_clone.clone())));

        store.save_last_synced_height(10)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        assert_eq!(indexer.get_best_height()?, Some(10));
        assert_eq!(indexer.get_height_to_sync()?, 10);
    }

    // 5. Indexed block exists, checkpoint < indexed height (should warn and use checkpoint)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_new_best_block(&block_11)?;
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

        store.save_last_synced_height(11)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(10))))?;
        assert_eq!(indexer.get_best_height()?, Some(10));
        assert_eq!(indexer.get_height_to_sync()?, 10);
    }

    // 6. Indexed block exists, checkpoint > indexed height (should warn and use indexed height)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_new_best_block(&block_10_clone)?;
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));

        let block_10_copy = block_10.clone();
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(10))
            .returning(move |_| Ok(Some(block_10_copy.clone())));

        store.save_last_synced_height(10)?;

        let block_12_clone = block_12.clone();
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(12))
            .returning(move |_| Ok(Some(block_12_clone.clone())));

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))))?;
        assert_eq!(indexer.get_best_height()?, Some(12));
        assert_eq!(indexer.get_height_to_sync()?, 12);
    }

    // 7. Indexed block exists, checkpoint == indexed height (should use indexed height)
    {
        let mut bitcoin_client = MockBitcoinClient::new();
        let store = get_indexer_store();

        store.save_new_best_block(&block_12)?;
        bitcoin_client
            .expect_get_best_block()
            .returning(move || Ok(12));
        bitcoin_client
            .expect_get_block_by_height()
            .with(eq(12))
            .returning(move |_| Ok(Some(block_12.clone())));
        store.save_last_synced_height(12)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))))?;
        assert_eq!(indexer.get_best_height()?, Some(12));
        assert_eq!(indexer.get_height_to_sync()?, 12);
    }

    clear_output();
    Ok(())
}
