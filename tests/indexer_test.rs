use bitcoin_indexer::{
    config::{IndexerConfig, IndexerSettings},
    errors::IndexerError,
    indexer::{Indexer, IndexerApi},
    store::StoreClient,
    types::FullBlock,
};
use bitcoind::bitcoind::Bitcoind;
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitvmx_settings::settings;
mod utils;
use crate::utils::clear_output;
use utils::get_indexer_store;

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
fn test_get_best_block() -> Result<(), anyhow::Error> {
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind = Bitcoind::new(
        "bitcoin-regtest",
        "bitcoin/bitcoin:29.1",
        config.bitcoin.clone(),
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine some blocks to have data to work with
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;

    let indexer = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;
    
    // Initially the indexer should be at checkpoint height
    let best_block = indexer.get_best_block()?;
    assert!(best_block.is_some());
    assert_eq!(best_block.unwrap().height, 100);

    // After tick, it should sync the next block
    indexer.tick()?;
    let best_block = indexer.get_best_block()?;
    assert!(best_block.is_some());
    assert_eq!(best_block.unwrap().height, 101);

    clear_output();
    Ok(())
}

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
fn indexer_constructor_checkpoint_variants() -> Result<(), anyhow::Error> {
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;

    // 1. No indexed block, no checkpoint (should start from genesis)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-1",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        // Should have saved height_to_sync = 0
        assert_eq!(indexer.get_best_height()?, Some(0));
    }

    // 2. No indexed block, checkpoint = 11 (should start from 11)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-2",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(11))))?;
        assert_eq!(indexer.get_best_height()?, Some(11));
    }

    // 3. No indexed block, checkpoint > blockchain height (should error)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-3",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(10, &wallet)?;

        let result = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(20))));
        assert!(result.is_err());
    }

    // 4. Indexed block exists, checkpoint is None (should start from indexed height)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-4",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;
        
        // Get block at height 10 and save it to store
        let block_10 = bitcoin_client.get_block_by_height(&10)?.unwrap();
        store.save_new_best_block(&block_10, 0)?;
        store.save_best_height(10)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        assert_eq!(indexer.get_best_height()?, Some(10));
    }

    // 5. Indexed block exists, checkpoint does not exist in the database and passing a checkpoint height (should use indexed height) and warn user
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-5",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;
        
        // Save block 11 as indexed
        let block_11 = bitcoin_client.get_block_by_height(&11)?.unwrap();
        store.save_new_best_block(&block_11, 0)?;
        store.save_best_height(11)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(10))))?;
        assert_eq!(indexer.get_best_height()?, Some(11));
    }

    // 6. Indexed block exists, checkpoint exist and is different from the previous checkpoint height (should error)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-6",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;
        
        // Save checkpoint at 10
        store.save_checkpoint_height(10)?;
        
        // Save block 10 as indexed
        let block_10 = bitcoin_client.get_block_by_height(&10)?.unwrap();
        store.save_new_best_block(&block_10, 0)?;
        store.save_best_height(10)?;

        let result = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))));
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(IndexerError::AlreadyIndexedWithDifferentCheckpointHeight)
        ));
    }

    // 7. Indexed block exists, checkpoint == indexed height (should use indexed height)
    {
        let bitcoind = Bitcoind::new(
            "bitcoin-regtest-7",
            "bitcoin/bitcoin:29.1",
            config.bitcoin.clone(),
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;
        
        // Save block 12 as indexed
        let block_12 = bitcoin_client.get_block_by_height(&12)?.unwrap();
        store.save_new_best_block(&block_12, 0)?;
        store.save_best_height(12)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(12))))?;
        assert_eq!(indexer.get_best_height()?, Some(12));
    }

    clear_output();
    Ok(())
}

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
fn test_orphan_block_not_marked_during_reorg() -> Result<(), anyhow::Error> {
    clear_output();

    // Initialize tracing to see warn! and info! output
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let config = settings::load::<IndexerConfig>()?;
    let bitcoind = Bitcoind::new(
        "bitcoin-regtest-reorg",
        "bitcoin/bitcoin:29.1",
        config.bitcoin.clone(),
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks (up to block 9)
    bitcoin_client.mine_blocks_to_address(10, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(9))),
    )?;

    // Sync to block 9
    assert_eq!(indexer.get_best_height()?, Some(9));

    // Mine one more block (block 10)
    bitcoin_client.mine_blocks_to_address(1, &wallet)?;

    // Tick 1: Sync block 10 (original)
    indexer.tick()?;
    assert_eq!(indexer.get_best_height()?, Some(10));

    let block_at_10: FullBlock = store
        .get_block_by_height(10)?
        .expect("Block 10 should exist");
    let hash_10_original = block_at_10.hash.clone();
    assert_eq!(
        block_at_10.orphan, false,
        "Block 10 should not be orphan initially"
    );

    // Cause a reorg: Invalidate block 10
    bitcoin_client.invalidate_block(&hash_10_original)?;

    // Verify we're back at block 9
    assert_eq!(bitcoin_client.get_best_block()?, 9);

    // Mine a different block 10 and block 11
    bitcoin_client.mine_blocks_to_address(2, &wallet)?;

    // Verify blockchain is now at height 11
    assert_eq!(bitcoin_client.get_best_block()?, 11);

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

    // Tick 3: Sync new block 10
    indexer.tick()?;

    assert_eq!(indexer.get_best_height()?, Some(10));

    let block_at_10: FullBlock = store
        .get_block_by_height(10)?
        .expect("Block 10 should exist");
    let hash_10_reorg = block_at_10.hash.clone();
    assert_ne!(hash_10_reorg, hash_10_original);
    assert_eq!(
        block_at_10.orphan, false,
        "New Block 10 should not be orphan"
    );

    // Tick 4: Sync block 11
    indexer.tick()?;

    assert_eq!(indexer.get_best_height()?, Some(11));

    let block_at_11: FullBlock = store
        .get_block_by_height(11)?
        .expect("Block 11 should exist");
    assert_eq!(block_at_11.orphan, false, "Block 11 should not be orphan");

    clear_output();
    Ok(())
}
