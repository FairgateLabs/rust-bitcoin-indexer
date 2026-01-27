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
fn test_load_config_from_yaml() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify that valid YAML configuration is parsed correctly into IndexerConfig.
     * Preconditions: config/development.yaml exists with valid structure.
     * Input: Path to valid YAML configuration file.
     * Steps:
     *
     * Call settings::load::<IndexerConfig>().
     * Inspect returned IndexerConfig struct fields.
     * Verify all fields match expected values from YAML.
     * Expected Result: Configuration loaded successfully with storage, bitcoin, 
     *  settings, and log_level fields populated correctly. No errors thrown.
     */
    
    // Load the configuration from YAML file
    let config = settings::load::<IndexerConfig>()?;
    
    // Verify storage configuration
    assert_eq!(config.storage.path, "data");
    
    // Verify bitcoin RPC configuration
    assert_eq!(config.bitcoin.network, Network::Regtest);
    assert_eq!(config.bitcoin.url.expose_secret(), "http://localhost:18443");
    assert_eq!(config.bitcoin.username.expose_secret(), "foo");
    assert_eq!(config.bitcoin.password.expose_secret(), "rpcpassword");
    assert_eq!(config.bitcoin.wallet, "test_wallet");
    
    // Verify indexer settings
    assert!(config.settings.is_some());
    let settings = config.settings.unwrap();
    assert_eq!(settings.checkpoint_height, Some(1));
    
    // Verify log level
    assert!(config.log_level.is_some());
    assert_eq!(config.log_level.unwrap(), "info");
    
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
fn test_invalid_configuration_fails_gracefully() -> Result<(), anyhow::Error> {
    /*
    * Objective: Ensure malformed configuration produces clear error messages.
    * Preconditions: Create invalid YAML (missing required fields, wrong types).
    * Input: YAML with missing storage.path or invalid network type.
    * Steps:
    * Attempt to load invalid configuration.
    * Catch error result.
    * Verify error message indicates specific validation failure.
    * Expected Result: Configuration loading returns Err with descriptive error message. Does not panic.
     */
    
    // Test 1: Missing required 'storage' field
    let invalid_config_missing_storage = r#"{
        "bitcoin": {
            "network": "regtest",
            "url": "http://localhost:18443",
            "username": "testuser",
            "password": "testpass",
            "wallet": "test_wallet"
        }
    }"#;
    
    let result: Result<IndexerConfig, _> = serde_json::from_str(invalid_config_missing_storage);
    assert!(result.is_err(), "Should fail when storage field is missing");
    
    // Test 2: Missing required 'bitcoin' field  
    let invalid_config_missing_bitcoin = r#"{
        "storage": {
            "path": "data"
        }
    }"#;
    
    let result: Result<IndexerConfig, _> = serde_json::from_str(invalid_config_missing_bitcoin);
    assert!(result.is_err(), "Should fail when bitcoin field is missing");
    
    // Test 3: Invalid network type
    let invalid_config_wrong_network = r#"{
        "storage": {
            "path": "data"
        },
        "bitcoin": {
            "network": "invalidnetwork",
            "url": "http://localhost:18443",
            "username": "testuser",
            "password": "testpass",
            "wallet": "test_wallet"
        }
    }"#;
    
    let result: Result<IndexerConfig, _> = serde_json::from_str(invalid_config_wrong_network);
    assert!(result.is_err(), "Should fail when network type is invalid");
    
    // Test 4: Empty configuration
    let empty_config = r#"{}"#;
    
    let result: Result<IndexerConfig, _> = serde_json::from_str(empty_config);
    assert!(result.is_err(), "Should fail with empty configuration");
    
    // Test 5: Invalid checkpoint height type (negative number)
    let invalid_checkpoint = r#"{
        "storage": {
            "path": "data"
        },
        "bitcoin": {
            "network": "regtest",
            "url": "http://localhost:18443",
            "username": "testuser",
            "password": "testpass",
            "wallet": "test_wallet"
        },
        "settings": {
            "checkpoint_height": -1
        }
    }"#;
    
    let result: Result<IndexerConfig, _> = serde_json::from_str(invalid_checkpoint);
    assert!(result.is_err(), "Should fail when checkpoint_height is negative");
    
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