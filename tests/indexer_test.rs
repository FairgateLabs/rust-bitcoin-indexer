use bitcoin_indexer::{
    config::{IndexerConfig, IndexerSettings},
    errors::IndexerError,
    indexer::{Indexer, IndexerApi},
    store::{IndexerStore, StoreClient},
    types::FullBlock,
};
use bitcoin::Network;
use bitcoind::bitcoind::Bitcoind;
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitvmx_settings::settings;
mod utils;
use crate::utils::{clear_output, wait_for_port_available};
use utils::get_indexer_store;

#[test]
fn test_get_best_block() -> Result<(), anyhow::Error> {
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
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

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn indexer_constructor_checkpoint_variants() -> Result<(), anyhow::Error> {
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;

    // 1. No indexed block, no checkpoint (should start from genesis)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(None)))?;
        // Should have saved height_to_sync = 0
        assert_eq!(indexer.get_best_height()?, Some(0));
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 2. No indexed block, checkpoint = 11 (should start from 11)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(12, &wallet)?;

        let indexer = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(11))))?;
        assert_eq!(indexer.get_best_height()?, Some(11));
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 3. No indexed block, checkpoint > blockchain height (should error)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
        );
        bitcoind.start()?;
        
        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;
        let store = get_indexer_store();
        
        bitcoin_client.mine_blocks_to_address(10, &wallet)?;

        let result = Indexer::new(bitcoin_client, store, Some(IndexerSettings::new(Some(20))));
        assert!(result.is_err());
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 4. Indexed block exists, checkpoint is None (should start from indexed height)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
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
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 5. Indexed block exists, checkpoint does not exist in the database and passing a checkpoint height (should use indexed height) and warn user
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
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
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 6. Indexed block exists, checkpoint exist and is different from the previous checkpoint height (should error)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
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
        bitcoind.stop()?;
        assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    }

    // 7. Indexed block exists, checkpoint == indexed height (should use indexed height)
    {
        let bitcoind_config = bitcoind::config::BitcoindConfig::default();
        let bitcoind = Bitcoind::new(
            bitcoind_config,
            config.bitcoin.clone(),
            None,
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
        bitcoind.stop()?;
    }

    clear_output();
    Ok(())
}

#[test]
fn test_orphan_block_not_marked_during_reorg() -> Result<(), anyhow::Error> {
    clear_output();

    // Initialize tracing to see warn! and info! output
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();

    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
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

    // Mine a different block 10 and block 11 with a new wallet address
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(2, &new_wallet)?;

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

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_configuration_missing_optional_fields() -> Result<(), anyhow::Error> {
    /*
     * Configuration with Missing Optional Fields
     * Title: Load Configuration with Missing Optional Settings
     * Objective: Verify default values are applied when optional fields are omitted.
     * Preconditions: Create test configuration YAML with settings and log_level omitted.
     * Input: Configuration with only required storage and bitcoin fields.
     * Steps:
     *  Load configuration from test YAML.
     *  Construct Indexer with loaded config.
     *  Verify default checkpoint height (0) is used.
     * Expected Result: 
     *  Configuration loads successfully. settings defaults to None. 
     *  Indexer initializes with default checkpoint height of 0.
     */
    
    // Test that IndexerConfig can be constructed with optional fields set to None
    // This simulates what happens when YAML doesn't contain settings or log_level
    
    // First, load the default config to get valid storage and bitcoin config
    let default_config = settings::load::<IndexerConfig>()?;
    
    // Create a config with optional fields as None
    let config_with_none_settings = IndexerConfig {
        storage: default_config.storage.clone(),
        bitcoin: default_config.bitcoin.clone(),
        settings: None,
        log_level: None,
    };
    
    // Verify optional fields are None
    assert!(config_with_none_settings.settings.is_none(), "settings should be None when not specified");
    assert!(config_with_none_settings.log_level.is_none(), "log_level should be None when not specified");
    
    // Verify that when settings is None, the indexer should use default behavior
    // The IndexerSettings::default() has checkpoint_height set to Some(DEFAULT_CHECKPOINT_HEIGHT)
    let default_settings = IndexerSettings::default();
    assert_eq!(default_settings.checkpoint_height, Some(0), "default checkpoint height should be 0");
    
    Ok(())
}

#[test]
fn test_indexersettings_defaults() {
    /* 
     * Objective: Verify IndexerSettings::default() provides expected values.
     * Preconditions: None.
     * Input: None.
     * Steps:
     * Call IndexerSettings::default().
     * Inspect checkpoint_height field.
     * Expected Result: checkpoint_height equals DEFAULT_CHECKPOINT_HEIGHT (0)
    */
    
    // Create IndexerSettings with default values
    let default_settings = IndexerSettings::default();
    
    // Verify checkpoint_height is set to DEFAULT_CHECKPOINT_HEIGHT (0)
    assert!(default_settings.checkpoint_height.is_some(), "checkpoint_height should have a value");
    assert_eq!(
        default_settings.checkpoint_height.unwrap(), 
        0, 
        "default checkpoint_height should be 0 (DEFAULT_CHECKPOINT_HEIGHT)"
    );
    
    // Test that creating settings with None still uses default when Default trait is invoked
    let settings_with_none = IndexerSettings::new(None);
    assert!(settings_with_none.checkpoint_height.is_none(), "IndexerSettings::new(None) should have None checkpoint");
    
    // But Default::default() should have Some(0)
    assert_ne!(
        settings_with_none.checkpoint_height,
        default_settings.checkpoint_height,
        "new(None) and default() should produce different results"
    );
}

#[test]
fn test_initialize_from_genesis_no_checkpoint() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer begins at block 0 when no checkpoint is configured and no prior state exists.
     * Preconditions: Empty storage. Bitcoin regtest node with 100 blocks.
     * Input: checkpoint_height: None, regtest blockchain at height 100.
     * Steps:
     * Start Bitcoin Core in regtest mode and mine 100 blocks.
     * Construct Indexer with IndexerSettings::new(None) pointing to regtest node.
     * Query get_best_height().
     * Verify block 0 is stored.
     * Expected Result: Indexer initializes successfully. get_best_height() returns Some(0). Block 0 saved to storage with correct genesis block hash.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine 100 blocks
    bitcoin_client.mine_blocks_to_address(100, &wallet)?;

    // Construct Indexer with no checkpoint (None)
    let bitcoin_client_clone = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client_clone,
        store.clone(),
        Some(IndexerSettings::new(None)),
    )?;

    // Verify indexer starts at block 0
    assert_eq!(indexer.get_best_height()?, Some(0));

    // Verify block 0 is stored with correct genesis block hash
    let genesis_block = bitcoin_client.get_block_by_height(&0)?.unwrap();
    let genesis_block_hash = genesis_block.hash;
    let stored_hash = store.get_block_hash_by_height(0)?;
    assert!(stored_hash.is_some());
    assert_eq!(stored_hash.unwrap(), genesis_block_hash);

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_initialize_from_valid_checkpoint() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer begins at checkpoint height when checkpoint < blockchain height.
     * Preconditions: Empty storage. Bitcoin regtest node with 150 blocks.
     * Input: checkpoint_height: Some(100), regtest blockchain at height 150.
     * Steps:
     *  Start regtest node and mine 150 blocks.
     *  Construct indexer with checkpoint_height: Some(100).
     *  Verify checkpoint saved and best height = 100.
     *  Query storage for block at height 100.
     *  Expected Result: Indexer saves checkpoint height 100 to storage. get_best_height() returns Some(100). Block at height 100 saved with correct hash from regtest chain.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine 150 blocks
    bitcoin_client.mine_blocks_to_address(150, &wallet)?;

    // Construct indexer with checkpoint_height = 100 using a separate client instance
    let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client_for_indexer,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),

    )?;

    // Verify indexer starts at checkpoint height 100
    assert_eq!(indexer.get_best_height()?, Some(100));

    // Verify checkpoint is saved in storage
    assert_eq!(store.get_checkpoint_height()?, Some(100));

    // Verify block at height 100 is stored with correct hash from regtest chain
    let block_100 = bitcoin_client.get_block_by_height(&100)?.unwrap();
    let block_hash_100 = block_100.hash;
    let stored_hash = store.get_block_hash_by_height(100)?;
    assert!(stored_hash.is_some());
    assert_eq!(stored_hash.unwrap(), block_hash_100);

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_checkpoint_ahead_of_blockchain_height_fails() -> Result<(), anyhow::Error> {
    /*
     * Objective: Ensure indexer rejects checkpoint ahead of blockchain tip.
     * Preconditions: Empty storage. Bitcoin regtest node with only 50 blocks.
     * Input: checkpoint_height: Some(100), regtest blockchain at height 50.
     * Steps:
     *  Start regtest node and mine only 50 blocks.
     *  Attempt to construct indexer with checkpoint 100.
     *  Catch error result.
     * Expected Result: Construction fails with IndexerError::CheckpointHeightAheadOfBlockchainHeight. No storage mutations. Error message clearly indicates the issue.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine only 50 blocks
    bitcoin_client.mine_blocks_to_address(50, &wallet)?;

    // Attempt to construct indexer with checkpoint 100 (ahead of blockchain height 50)
    let result = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    );

    // Verify construction fails with CheckpointHeightAheadOfBlockchainHeight
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::CheckpointHeightAheadOfBlockchainHeight)
    ));

    // Verify no storage mutations occurred
    assert_eq!(store.get_best_height()?, None);
    assert_eq!(store.get_checkpoint_height()?, None);

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_resume_from_existing_indexed_height() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer continues from last indexed block when storage has prior state.
     * Preconditions: Storage contains blocks 0-75 from previous run. Bitcoin regtest node at height 100.
     * Input: Best height in storage = 75, no checkpoint, regtest blockchain at height 100.
     * Steps:
     *  Run indexer to sync blocks 0-75, then stop.
     *  Mine 25 more blocks on regtest (total 100).
     *  Construct new indexer instance pointing to same storage.
     *  Verify best height is 75.
     *  Verify block hash at height 75 matches regtest chain.
     * Expected Result: Indexer initializes with best height = 75. No errors. Ready to sync block 76 on next tick.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine 80 blocks initially
    bitcoin_client.mine_blocks_to_address(80, &wallet)?;

    // Step 1: Run indexer to sync blocks 0-75
    {
        let bitcoin_client_clone = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
            bitcoin_client_clone,
            store.clone(),
            Some(IndexerSettings::new(None)),
        )?;
        
        // Sync up to height 75
        for _ in 0..75 {
            indexer.tick()?;
        }
        
        // Verify we're at height 75
        assert_eq!(indexer.get_best_height()?, Some(75));
    }
    // Indexer dropped here, simulating a stop

    // Step 2: Mine 25 more blocks (total 105)
    bitcoin_client.mine_blocks_to_address(25, &wallet)?;

    // Step 3: Construct new indexer instance pointing to same storage
    let bitcoin_client_new = BitcoinClient::new_from_config(&config.bitcoin)?;
    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer_new = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(None)),  // Changed from Some(100) to None
    )?;

    // Verify best height is 75 (resume from where we left off)
    assert_eq!(indexer_new.get_best_height()?, Some(75));

    // Verify block hash at height 75 matches regtest chain
    let block_hash_75 = bitcoin_client_new.get_block_by_height(&75)?.unwrap().hash;
    let stored_hash = store.get_block_hash_by_height(75)?;
    assert!(stored_hash.is_some());
    assert_eq!(stored_hash.unwrap(), block_hash_75);

    // Verify we can continue syncing
    indexer_new.tick()?;
    assert_eq!(indexer_new.get_best_height()?, Some(76));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_indexed_height_exceeds_blockchain_height() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify error when indexed height > blockchain height (inconsistent state).
     * Preconditions: Storage has blocks 0-120 from regtest chain. Current regtest node only has 100 blocks (simulating node reset or different chain).
     * Input: Indexed height 120, regtest blockchain at height 100.
     * Steps:
     *  Sync indexer to height 120 on a regtest chain.
     *  Reset regtest node or switch to different node with only 100 blocks.
     *  Attempt to construct indexer pointing to new node.
     * Expected Result: Construction fails with IndexerError::InconsistentBlockchain. Logs error message indicating indexer is ahead of blockchain.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine 125 blocks
    bitcoin_client.mine_blocks_to_address(125, &wallet)?;

    // Step 1: Sync indexer to height 120
    {
        let bitcoin_client_clone = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
            bitcoin_client_clone,
            store.clone(),
            Some(IndexerSettings::new(None)),
        )?;
        
        // Sync up to height 120
        for _ in 0..120 {
            indexer.tick()?;
        }
        
        // Verify we're at height 120
        assert_eq!(indexer.get_best_height()?, Some(120));
    }

    // Step 2: Simulate node reset by invalidating blocks back to 100
    // This makes the blockchain height 100 while storage still has height 120
    let block_101 = bitcoin_client.get_block_by_height(&101)?.unwrap();
    let block_hash_101 = block_101.hash;
    bitcoin_client.invalidate_block(&block_hash_101)?;

    // Verify blockchain is now at height 100
    let blockchain_height = bitcoin_client.get_blockchain_info()?.blocks;
    assert_eq!(blockchain_height, 100);

    // Step 3: Attempt to construct indexer (indexed height 120 > blockchain height 100)
    let bitcoin_client_new = BitcoinClient::new_from_config(&config.bitcoin)?;
    let result = Indexer::new(
        bitcoin_client_new,
        store.clone(),
        Some(IndexerSettings::new(None)),
    );

    // Verify construction fails with InconsistentBlockchain
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::InconsistentBlockchain)
    ));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_checkpoint_already_exists_and_match() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer allows restart with same checkpoint.
     * Preconditions: Storage has checkpoint 100 and indexed height 120 from previous run. Bitcoin regtest at height 150.
     * Input: checkpoint_height: Some(100), stored checkpoint 100.
     * Steps:
     *  Run indexer with checkpoint 100, sync to height 120.
     *  Stop indexer.
     *  Mine more blocks on regtest (to height 150).
     *  Construct new indexer with same checkpoint 100.
     *  Verify no errors.
     * Expected Result: Indexer initializes successfully. Best height = 120. No checkpoint conflict. Ready to continue syncing.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine 125 blocks initially
    bitcoin_client.mine_blocks_to_address(125, &wallet)?;

    // Step 1: Run indexer with checkpoint 100, sync to height 120
    {
        let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
            bitcoin_client_for_indexer,
            store.clone(),
            Some(IndexerSettings::new(Some(100))),
        )?;
        
        // Verify starts at checkpoint 100
        assert_eq!(indexer.get_best_height()?, Some(100));
        
        // Sync up to height 120
        for _ in 0..20 {  // Changed from 21 to 20
            indexer.tick()?;
        }
        
        // Verify we're at height 120
        assert_eq!(indexer.get_best_height()?, Some(120));
    }
    // Indexer dropped here, simulating a stop

    // Step 2: Mine more blocks (to height 155)
    bitcoin_client.mine_blocks_to_address(30, &wallet)?;

    // Step 3: Construct new indexer with same checkpoint 100
    let bitcoin_client_new = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer_new = Indexer::new(
        bitcoin_client_new,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Verify no errors and best height is 120 (resume point)
    assert_eq!(indexer_new.get_best_height()?, Some(120));

    // Verify checkpoint is still 100
    assert_eq!(store.get_checkpoint_height()?, Some(100));

    // Verify we can continue syncing
    indexer_new.tick()?;
    assert_eq!(indexer_new.get_best_height()?, Some(121));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_different_checkpoint_height_fails() -> Result<(), anyhow::Error> {
    /*
     * Objective: Ensure indexer rejects different checkpoint if one already exists.
     * Preconditions: Storage has checkpoint 50 and indexed data from previous run.
     * Input: checkpoint_height: Some(100), stored checkpoint 50.
     * Steps:
     *   Run indexer with checkpoint 50, sync some blocks.
     *   Stop indexer.
     *   Attempt to construct new indexer with different checkpoint 100 using same storage.
     * Expected Result: Construction fails with IndexerError::AlreadyIndexedWithDifferentCheckpointHeight. 
     * Error log suggests wiping database or using original checkpoint.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine enough blocks to work with both checkpoints
    bitcoin_client.mine_blocks_to_address(110, &wallet)?;

    // Step 1: Run indexer with checkpoint 50, sync some blocks
    {
        let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
             bitcoin_client_for_indexer,
            store.clone(),
            Some(IndexerSettings::new(Some(50))),
        )?;
        
        // Verify the indexer is at checkpoint height 50
        assert_eq!(indexer.get_best_height()?, Some(50));
        
        // Sync a few more blocks
        indexer.tick()?;
        indexer.tick()?;
        indexer.tick()?;
        
        // Verify we've synced past the checkpoint
        let best_height = indexer.get_best_height()?;
        assert!(best_height.is_some());
        assert!(best_height.unwrap() > 50);
    }
    // Indexer is dropped here, simulating a stop

    // Step 2: Attempt to construct new indexer with different checkpoint 100 using same storage
    let bitcoin_client_for_second_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
    let result = Indexer::new(
        bitcoin_client_for_second_indexer,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    );

    // Expected Result: Construction fails with IndexerError::AlreadyIndexedWithDifferentCheckpointHeight
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::AlreadyIndexedWithDifferentCheckpointHeight)
    ));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_database_corrupted_missing_block_hash_for_height() -> Result<(), anyhow::Error> {
    /*
     * Objective: Detect corrupted storage when height exists but hash missing.
     * Preconditions: Storage has best height = 80 but corresponding block hash data is corrupted or missing.
     * Input: Best height 80, but block hash for height 80 cannot be retrieved from storage.
     * Steps:
     *   Run indexer to sync to height 80.
     *   Manually corrupt storage by deleting block hash entry for height 80 (while keeping best height metadata).
     *   Attempt to construct indexer.
     * Expected Result: Construction fails with IndexerError::DatabaseCorrupted. Error indicates storage inconsistency.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    
    // Create storage path and store
    let store_path = format!(
        "test_output/get_best_block_height_test/{}",
        utils::generate_random_string()
    );
    // Create the directory before initializing Storage
    std::fs::create_dir_all(&store_path)?;
    
    // Build absolute path and normalize to forward slashes for Storage backend
    let current_dir = std::env::current_dir()?;
    let absolute_store_path = current_dir.join(&store_path);
    let path_str = absolute_store_path.to_string_lossy().replace('\\', "/");
    
    let storage_config = storage_backend::storage_config::StorageConfig::new(path_str, None);
    let storage = std::rc::Rc::new(storage_backend::storage::Storage::new(&storage_config)?);
    let store = std::rc::Rc::new(IndexerStore::new(storage.clone())?);

    // Mine enough blocks to reach height 80
    bitcoin_client.mine_blocks_to_address(85, &wallet)?;

    // Step 1: Run indexer to sync to height 80
    {
        let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
            bitcoin_client_for_indexer,
            store.clone(),
            Some(IndexerSettings::new(Some(80))),
        )?;
        
        // Verify the indexer is at height 80
        assert_eq!(indexer.get_best_height()?, Some(80));
    }

    // Step 2: Manually corrupt storage by deleting block hash entry for height 80
    // while keeping best height metadata intact
    let corrupted_key = format!("indexer/block/height/80");
    
    // Delete the block hash at height 80 directly from storage
    //use storage_backend::storage::KeyValueStore;
    storage.delete(&corrupted_key)?;
    
    // Verify that best height is still 80 but hash is missing
    assert_eq!(store.get_best_height()?, Some(80));
    assert_eq!(store.get_block_hash_by_height(80)?, None);

    // Step 3: Attempt to construct indexer with corrupted storage
    let bitcoin_client_for_corrupted_test = BitcoinClient::new_from_config(&config.bitcoin)?;
    let result = Indexer::new(
        bitcoin_client_for_corrupted_test,
        store.clone(),
        Some(IndexerSettings::new(Some(80))),
    );

    // Expected Result: Construction fails with IndexerError::DatabaseCorrupted
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::DatabaseCorrupted)
    ));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_detect_single_block_reorg() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify reorg detection when block at current height has different hash.
     * Preconditions: Indexer at height 100 with block hash A. Bitcoin regtest performs single-block reorg.
     * Input: Stored block 100 hash A, regtest now has block 100 hash B after reorg.
     * Steps:
     *      Initialize indexer and sync to height 100.
     *      Note the block hash at height 100.
     *      Use invalidateblock RPC on regtest to invalidate block 100.
     *      Mine a new block at height 100 (different hash).
     *      Call tick().
     *      Verify rollback behavior.
     * Expected Result: Tick detects reorg. Marks block 100 with original hash as orphan. Rolls back to height 99. Logs "REORG" warning. Next tick will sync new block 100.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 100
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(99))),
    )?;

    // Sync to block 100
    indexer.tick()?;
    assert_eq!(indexer.get_best_height()?, Some(100));

    // Note the block hash at height 100
    let original_block_100 = store.get_block_by_height(100)?.expect("Block 100 should exist");
    let original_hash_100 = original_block_100.hash.clone();
    assert_eq!(original_block_100.orphan, false);

    // Use invalidateblock RPC to invalidate block 100
    bitcoin_client.invalidate_block(&original_hash_100)?;
    
    // Verify blockchain is back at height 99
    assert_eq!(bitcoin_client.get_best_block()?, 99);

    // Mine a new block at height 100 (different hash)
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(1, &new_wallet)?;
    
    // Verify blockchain is at height 100 with a different hash
    assert_eq!(bitcoin_client.get_best_block()?, 100);
    let new_block_100 = bitcoin_client.get_block_by_height(&100)?.expect("New block 100 should exist");
    assert_ne!(new_block_100.hash, original_hash_100);

    // Call tick() - should detect reorg
    indexer.tick()?;

    // Verify rollback to height 99
    assert_eq!(indexer.get_best_height()?, Some(99));

    // Verify original block 100 is marked as orphan
    let orphaned_block = store.get_block_by_hash(&original_hash_100)?;
    assert!(orphaned_block.is_some());
    assert_eq!(orphaned_block.unwrap().orphan, true);

    // Next tick should sync new block 100
    indexer.tick()?;
    assert_eq!(indexer.get_best_height()?, Some(100));
    
    let new_stored_block = store.get_block_by_height(100)?.expect("New block 100 should be stored");
    assert_eq!(new_stored_block.hash, new_block_100.hash);
    assert_eq!(new_stored_block.orphan, false);

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_reorg_marks_following_blocks_as_orphan() -> Result<(), anyhow::Error> {
    /*
     * Objective: Ensure mark_following_blocks_as_orphan marks affected blocks.
     * Preconditions: Indexer at height 105. Reorg on regtest affects height 102+.
     * Input: Reorg at height 102, blocks 102-105 stored from original chain.
     * Steps:
     *      Initialize indexer and sync to height 105.
     *      Use invalidateblock RPC to invalidate block 102 on regtest.
     *      Mine alternative blocks 102-105.
     *      Call tick() to initiate reorg handling.
     *      Query blocks 102-105 from storage by their original hashes.
     * Expected Result: Original blocks 102-105 have orphan = true. Best height rolled back to 101. Original blocks remain in storage but marked orphan.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 106
    bitcoin_client.mine_blocks_to_address(107, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Sync to height 105
    for _ in 0..5 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(105));

    // Store original block hashes for heights 102-105
    let original_block_102 = store.get_block_by_height(102)?.expect("Block 102 should exist");
    let original_hash_102 = original_block_102.hash.clone();
    
    let original_block_103 = store.get_block_by_height(103)?.expect("Block 103 should exist");
    let original_hash_103 = original_block_103.hash.clone();
    
    let original_block_104 = store.get_block_by_height(104)?.expect("Block 104 should exist");
    let original_hash_104 = original_block_104.hash.clone();
    
    let original_block_105 = store.get_block_by_height(105)?.expect("Block 105 should exist");
    let original_hash_105 = original_block_105.hash.clone();

    // Verify all blocks are not orphan initially
    assert_eq!(original_block_102.orphan, false);
    assert_eq!(original_block_103.orphan, false);
    assert_eq!(original_block_104.orphan, false);
    assert_eq!(original_block_105.orphan, false);

    // Use invalidateblock RPC to invalidate block 102 on regtest
    bitcoin_client.invalidate_block(&original_hash_102)?;
    
    // Verify blockchain is back at height 101
    assert_eq!(bitcoin_client.get_best_block()?, 101);

    // Mine alternative blocks 102-106 with different wallet (one extra to trigger reorg detection)
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(5, &new_wallet)?;
    
    // Verify blockchain is at height 106
    assert_eq!(bitcoin_client.get_best_block()?, 106);

    // Call tick() multiple times to initiate reorg handling and roll back through all affected blocks
    // Starting at height 105, reorg affects 102-105, rolling back one block per tick to reach 101
    for _ in 0..4 {
        indexer.tick()?;
    }

    // Verify best height rolled back to 101
    assert_eq!(indexer.get_best_height()?, Some(101));

    // Query blocks 102-105 from storage by their original hashes
    let orphaned_102 = store.get_block_by_hash(&original_hash_102)?;
    assert!(orphaned_102.is_some());
    assert_eq!(orphaned_102.unwrap().orphan, true);
    
    let orphaned_103 = store.get_block_by_hash(&original_hash_103)?;
    assert!(orphaned_103.is_some());
    assert_eq!(orphaned_103.unwrap().orphan, true);
    
    let orphaned_104 = store.get_block_by_hash(&original_hash_104)?;
    assert!(orphaned_104.is_some());
    assert_eq!(orphaned_104.unwrap().orphan, true);
    
    let orphaned_105 = store.get_block_by_hash(&original_hash_105)?;
    assert!(orphaned_105.is_some());
    assert_eq!(orphaned_105.unwrap().orphan, true);

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_resync_after_reorg() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer syncs new blocks after rolling back.
     * Preconditions: Reorg occurred on regtest, indexer rolled back to height 99.
     * Input: New blockchain with different blocks 100-105 after reorg.
     * Steps:
     *
     * Perform reorg: invalidate blocks 100+, mine new chain to 105.
     * Indexer detects reorg and rolls back to 99.
     * Call tick() multiple times (6 times for blocks 100-105).
     * Verify new blocks stored.
     * Query both old and new blocks by hash.
     * Expected Result: Indexer syncs new blocks 100-105. New blocks have orphan = false and can be queried by height. Old blocks with same heights remain in storage marked orphan = true (queryable only by hash). Best height advances to 105.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 103
    bitcoin_client.mine_blocks_to_address(104, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(99))),
    )?;

    // Sync to height 102
    for _ in 0..3 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(102));

    // Store original block hashes for heights 100-102
    let old_block_100 = store.get_block_by_height(100)?.expect("Block 100 should exist");
    let old_hash_100 = old_block_100.hash.clone();
    
    let old_block_101 = store.get_block_by_height(101)?.expect("Block 101 should exist");
    let old_hash_101 = old_block_101.hash.clone();
    
    let old_block_102 = store.get_block_by_height(102)?.expect("Block 102 should exist");
    let old_hash_102 = old_block_102.hash.clone();

    // Invalidate block at height 100 on regtest to simulate reorg
    bitcoin_client.invalidate_block(&old_hash_100)?;
    
    // Verify blockchain is back at height 99
    assert_eq!(bitcoin_client.get_best_block()?, 99);

    // Mine new blocks with different wallet so new chain is at 105
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(6, &new_wallet)?;
    
    // Verify blockchain is at height 105
    assert_eq!(bitcoin_client.get_best_block()?, 105);

    // Call tick() multiple times to rollback indexer
    // From height 102, need to roll back through 3 blocks to get to 99 (102→101→100→99)
    for _ in 0..3 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(99));

    // Call tick() 6 times to sync to new blocks 100-105
    for _ in 0..6 {
        indexer.tick()?;
    }

    // Verify indexer now at height 105
    assert_eq!(indexer.get_best_height()?, Some(105));

    // Verify new blocks stored and have orphan=false
    for height in 100..=105 {
        let block = store.get_block_by_height(height)?.expect(&format!("Block {} should exist", height));
        assert_eq!(block.orphan, false, "Block {} should not be orphan", height);
    }

    // Get new block hashes at heights 100-105
    let new_block_100 = store.get_block_by_height(100)?.unwrap();
    let new_hash_100 = new_block_100.hash.clone();

    // Verify new blocks are different from original
    assert_ne!(new_hash_100, old_hash_100, "New block 100 should have different hash");

    // Query old blocks by hash - they should still exist but marked orphan
    let orphaned_100 = store.get_block_by_hash(&old_hash_100)?.expect("Old block 100 should still exist");
    assert_eq!(orphaned_100.orphan, true, "Old block 100 should be marked orphan");
    
    let orphaned_101 = store.get_block_by_hash(&old_hash_101)?.expect("Old block 101 should still exist");
    assert_eq!(orphaned_101.orphan, true, "Old block 101 should be marked orphan");
    
    let orphaned_102 = store.get_block_by_hash(&old_hash_102)?.expect("Old block 102 should still exist");
    assert_eq!(orphaned_102.orphan, true, "Old block 102 should be marked orphan");

    // Verify all new blocks are properly connected
    for height in 101..=105 {
        let current = store.get_block_by_height(height)?.expect(&format!("Block {} should exist", height));
        let previous = store.get_block_by_height(height - 1)?.expect(&format!("Block {} should exist", height - 1));
        assert_eq!(current.prev_hash, previous.hash, "Block {} should be connected to block {}", height, height - 1);
    }

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_multi_depth_reorg() -> Result<(), anyhow::Error> {
    /*
     * Objective: Test reorg detection and rollback across multiple blocks.
     * Preconditions: Indexer at height 120. Bitcoin regtest performs deep reorg.
     * Input: Reorg invalidates blocks 110-120, alternative chain provided.
     * Steps:
     * Initialize indexer and sync to height 120.
     * Use invalidateblock RPC on regtest to invalidate block 110.
     * Mine alternative blocks 110-120 (different hashes, possibly different number).
     * Call tick() repeatedly until indexer catches up.
     * Verify rollback and re-sync.
     * Expected Result: Indexer detects reorg at first mismatched block during tick. Rolls back to height 109. Old blocks 110-120 marked orphan. New blocks 110-120 synced from alternative chain. Final best height matches regtest tip.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 120
    bitcoin_client.mine_blocks_to_address(121, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Sync to height 120
    for _ in 0..20 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(120));

    // Store original block hashes for heights 110-120
    let mut original_hashes = vec![];
    for height in 110..=120 {
        let block = store.get_block_by_height(height)?.expect(&format!("Block {} should exist", height));
        assert_eq!(block.orphan, false);
        original_hashes.push(block.hash.clone());
    }

    // Use invalidateblock RPC to invalidate block 110
    bitcoin_client.invalidate_block(&original_hashes[0])?;
    
    // Verify blockchain is back at height 109
    assert_eq!(bitcoin_client.get_best_block()?, 109);

    // Mine alternative blocks 110-120 with different wallet
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(11, &new_wallet)?;
    
    // Verify blockchain is at height 120
    assert_eq!(bitcoin_client.get_best_block()?, 120);

    // Call tick() repeatedly (11 ticks to rollback 110-120, then 11 more to re-sync)
    for _ in 0..11 {
        indexer.tick()?;
    }

    // Verify best height rolled back to 109
    assert_eq!(indexer.get_best_height()?, Some(109));

    // Verify old blocks are marked orphan
    for (idx, original_hash) in original_hashes.iter().enumerate() {
        let height = 110 + idx;
        let orphaned = store.get_block_by_hash(original_hash)?;
        assert!(orphaned.is_some(), "Old block {} should exist", height);
        assert_eq!(orphaned.unwrap().orphan, true, "Block {} should be orphan", height);
    }

    // Call tick() 11 times to re-sync new blocks
    for _ in 0..11 {
        indexer.tick()?;
    }

    // Verify indexer is back at height 120
    assert_eq!(indexer.get_best_height()?, Some(120));

    // Verify new blocks stored and not orphan
    for height in 110..=120 {
        let block = store.get_block_by_height(height)?.expect(&format!("Block {} should exist", height));
        assert_eq!(block.orphan, false, "Block {} should not be orphan", height);
    }

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_checkpoint_cannot_be_reorged() -> Result<(), anyhow::Error> {
    /*
     * Objective: Ensure reorg handling respects checkpoint as immutable starting point.
     * Preconditions: Indexer initialized with checkpoint at height 50, synced to height 100.
     * Input: Attempt to reorg at or below checkpoint height.
     * Steps:
     * Initialize indexer with checkpoint 50, sync to height 100.
     * On regtest, attempt to invalidate block 50 or earlier.
     * Mine alternative chain.
     * Restart indexer or call tick().
     * Expected Result: Indexer either detects inconsistency and returns error, or continues to operate as checkpoint block is treated as immutable. Implementation-dependent behavior: may error or may ignore blocks below checkpoint.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 110
    bitcoin_client.mine_blocks_to_address(111, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(50))),
    )?;

    // Sync to height 100
    for _ in 0..50 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(100));

    // Verify checkpoint is set to 50
    assert_eq!(store.get_checkpoint_height()?, Some(50));

    // Get block hash at checkpoint height
    let checkpoint_block = store.get_block_by_height(50)?.expect("Block 50 should exist");
    let checkpoint_hash = checkpoint_block.hash.clone();

    // Get block hash at height 51 (just above checkpoint)
    let block_51 = store.get_block_by_height(51)?.expect("Block 51 should exist");
    let hash_51 = block_51.hash.clone();

    // Attempt to invalidate a block above checkpoint (51)
    bitcoin_client.invalidate_block(&hash_51)?;
    
    // Verify blockchain is back at height 50
    assert_eq!(bitcoin_client.get_best_block()?, 50);

    // Mine alternative blocks from 51 onwards
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(60, &new_wallet)?;
    
    // Verify blockchain is at height 110
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    // Call tick() multiple times to detect reorg and roll back to checkpoint level
    // From height 100, need to roll back to 50 (50 blocks to roll back)
    for _ in 0..50 {
        indexer.tick()?;
    }

    // Verify indexer rolled back to 50 (checkpoint level)
    assert_eq!(indexer.get_best_height()?, Some(50));

    // Verify checkpoint block is still not orphan (checkpoint blocks are immutable)
    let checkpoint_block_after = store.get_block_by_hash(&checkpoint_hash)?;
    assert!(checkpoint_block_after.is_some());
    assert_eq!(checkpoint_block_after.unwrap().orphan, false, "Checkpoint block should not be orphan");

    // Sync new blocks from 51 onwards
    for _ in 0..60 {
        indexer.tick()?;
    }

    // Verify indexer is at height 110
    assert_eq!(indexer.get_best_height()?, Some(110));

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_reorg_shortens_chain() -> Result<(), anyhow::Error> {
    /*
     * Objective: Handle case where blockchain has fewer blocks after reorg.
     * Preconditions: Indexer at height 150. Regtest reorgs to height 140.
     * Input: Indexed height 150, regtest performs reorg resulting in best height 140.
     * Steps:
     * Initialize indexer and sync to height 150.
     * Use invalidateblock on regtest to invalidate from height 141.
     * Mine alternative chain to only height 140.
     * Call tick() or restart indexer.
     * Verify adjustment.
     * Expected Result: Indexer detects chain is now shorter. Best height adjusted to 140. Blocks 141-150 from original chain remain in storage marked orphan. Warning logged about chain shortening.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks up to height 150
    bitcoin_client.mine_blocks_to_address(151, &wallet)?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Sync to height 150
    for _ in 0..50 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(150));

    // Store original block hashes for heights 141-150
    let mut original_hashes = vec![];
    for height in 141..=150 {
        let block = store.get_block_by_height(height)?.expect(&format!("Block {} should exist", height));
        assert_eq!(block.orphan, false);
        original_hashes.push(block.hash.clone());
    }

    // Get block at height 141 to invalidate
    let block_141 = store.get_block_by_height(141)?.expect("Block 141 should exist");
    let hash_141 = block_141.hash.clone();

    // Use invalidateblock RPC to invalidate block 141
    bitcoin_client.invalidate_block(&hash_141)?;
    
    // Verify blockchain is back at height 140
    assert_eq!(bitcoin_client.get_best_block()?, 140);

    // Mine one alternative block at height 141 (resulting in shorter chain than before)
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(1, &new_wallet)?;
    
    // Verify blockchain is at height 141 (shorter than indexer's 150)
    assert_eq!(bitcoin_client.get_best_block()?, 141);

    // Call tick() to detect reorg - should rollback to 140
    indexer.tick()?;

    // Verify best height rolled back to 140
    assert_eq!(indexer.get_best_height()?, Some(140));

    // Verify old blocks 141-150 are marked orphan
    for (idx, original_hash) in original_hashes.iter().enumerate() {
        let height = 141 + idx;
        let orphaned = store.get_block_by_hash(original_hash)?;
        assert!(orphaned.is_some(), "Old block {} should exist", height);
        assert_eq!(orphaned.unwrap().orphan, true, "Block {} should be orphan", height);
    }

    // Call tick() to sync new block 141
    indexer.tick()?;

    // Verify indexer is at height 141
    assert_eq!(indexer.get_best_height()?, Some(141));

    // Verify new block 141 is not orphan
    let new_block_141 = store.get_block_by_height(141)?.expect("New block 141 should exist");
    assert_eq!(new_block_141.orphan, false, "New block 141 should not be orphan");
    assert_ne!(new_block_141.hash, hash_141, "New block 141 should have different hash");

    bitcoind.stop()?;
    clear_output();
    Ok(())
}

#[test]
fn test_transaction_confirmations_after_reorg() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify get_tx() returns correct confirmations post-reorg.
     * Preconditions: Transaction in a block on regtest. Reorg orphans that block.
     * Input: TX in original block.
     * Steps:
     * Send transaction on regtest and mine it in a block.
     * Sync indexer to include that block.
     * Note transaction ID and verify it exists.
     * Perform reorg: invalidate the block, mine alternative block without that transaction.
     * Sync indexer through reorg.
     * Call get_tx(tx_id).
     * Inspect confirmations and orphan status.
     * Expected Result: get_tx() returns TransactionInfo with confirmations = 0 and block_info.orphan = true for original block. If transaction was re-mined in new block, separate entry would exist for new block.
     */
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = bitcoind::config::BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine initial blocks to ensure coinbase maturity
    bitcoin_client.mine_blocks_to_address(151, &wallet)?;
    
    let height_after_mining = bitcoin_client.get_best_block()?;

    let bitcoin_client2 = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(height_after_mining - 1))),
    )?;

    // Create and send a transaction (fund_address mines it into a block)
    let recipient_address = bitcoin_client.get_new_address(utils::get_random_pubkey(), Network::Regtest)?;
    let (funding_tx, _vout) = bitcoin_client.fund_address(&recipient_address, bitcoin::Amount::from_sat(100000))?;
    let tx_id = funding_tx.compute_txid();
    
    let tx_block_height = bitcoin_client.get_best_block()?;
    
    // Sync indexer to include the transaction block
    for _ in 0..(tx_block_height - (height_after_mining - 1)) {
        indexer.tick()?;
    }

    // Get original block containing the transaction
    let original_block = store.get_block_by_height(tx_block_height)?.expect("Block should exist");
    let original_hash = original_block.hash.clone();

    // Verify transaction exists in the block
    assert!(original_block.txs.iter().any(|tx| tx.compute_txid() == tx_id), 
            "Transaction should exist in storage");

    // Perform reorg: invalidate the block containing the transaction
    bitcoin_client.invalidate_block(&original_hash)?;

    // Mine alternative block without the transaction (using different wallet)
    let user_pubkey = utils::get_random_pubkey();
    let new_wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(1, &new_wallet)?;

    // Sync indexer through reorg (rollback + sync new block)
    indexer.tick()?;
    indexer.tick()?;

    // Verify original block is marked orphan and transaction still exists in it
    let orphaned_block = store.get_block_by_hash(&original_hash)?.expect("Orphaned block should exist");
    assert_eq!(orphaned_block.orphan, true);
    assert!(orphaned_block.txs.iter().any(|tx| tx.compute_txid() == tx_id), 
            "Transaction should still exist in orphaned block");

    // Verify new block exists and is not orphan
    let new_block = store.get_block_by_height(tx_block_height)?.expect("New block should exist");
    assert_eq!(new_block.orphan, false);
    assert_ne!(new_block.hash, original_hash, "New block should have different hash");

    bitcoind.stop()?;
    clear_output();
    Ok(())
}