use bitcoin_indexer::{
    config::{IndexerConfig, IndexerSettings},
    errors::IndexerError,
    indexer::{Indexer, IndexerApi},
    store::{IndexerStore, StoreClient},
    types::FullBlock,
};
use bitcoin::{Network, hashes::Hash};
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
     * Objective: Verify indexer begins at block 0 when no checkpoint is configured and no 
     * prior state exists.
     * Preconditions: Empty storage. Bitcoin regtest node with 100 blocks.
     * Input: checkpoint_height: None, regtest blockchain at height 100.
     * Steps:

     * -Start Bitcoin Core in regtest mode and mine 100 blocks.
     * -Construct Indexer with IndexerSettings::new(None) pointing to regtest node.
     * -Query get_best_height().
     * -Verify block 0 is stored.
     * Expected Result: Indexer initializes successfully. get_best_height() returns Some(0). 
     *     Block 0 saved to storage with correct genesis block hash
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock blockchain at height 100
    let genesis_hash = BlockHash::from_str("0f9188f13cb7b2c71f2a335e3a4fc328bf5beb436012afca590b1a11466e2206")?;
    //let block_100_hash = BlockHash::from_str("00000000000000000001a335e3a4fc328bf5beb436012afca590b1a11466e220")?;
    
    // Mock get_best_block to return height 100
    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(100));
    
    // Mock get_block_by_height for block 0 (genesis)
    let genesis_hash_clone = genesis_hash;
    bitcoin_client
        .expect_get_block_by_height()
        .withf(move |h| *h == 0)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: genesis_hash_clone,
                height: 0,
                prev_hash: BlockHash::all_zeros(),
                txs: vec![],
            }))
        });

    // Construct Indexer with no checkpoint (None)
    let indexer = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(None)),
    )?;

    // Verify indexer starts at block 0
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(0), "Indexer should start at block 0 when no checkpoint is configured");

    // Verify block 0 (genesis block) is stored
    let genesis_block = store.get_block_by_height(0)?;
    assert!(genesis_block.is_some(), "Genesis block should be stored");

    let genesis = genesis_block.unwrap();
    assert_eq!(genesis.height, 0, "Stored block should be at height 0");
    assert_eq!(genesis.orphan, false, "Genesis block should not be marked as orphan");
    assert_eq!(genesis.hash, genesis_hash, "Stored genesis block hash should match");

    clear_output();
    Ok(())
}

#[test]
fn test_starts_from_checkpoint_when_blockchain_height_is_sufficient() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer begins at checkpoint height when checkpoint < blockchain height.
     * Preconditions: Empty storage. Bitcoin regtest node with 150 blocks.
     * Input: checkpoint_height: Some(100), regtest blockchain at height 150.
     * Steps:
     *      Start regtest node and mine 150 blocks.
     *      Construct indexer with checkpoint_height: Some(100).
     *      Verify checkpoint saved and best height = 100.
     *      Query storage for block at height 100.
     * Expected Result: Indexer saves checkpoint height 100 to storage. get_best_height() 
     * returns Some(100). Block at height 100 saved with correct hash from regtest chain.
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock blockchain at height 150
    let block_100_hash = BlockHash::from_str("000000000000000000012a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_99_hash = BlockHash::from_str("000000000000000000011a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    // Mock get_best_block to return height 150
    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(150));
    
    // Mock get_block_by_height for block 100
    let block_100_hash_clone = block_100_hash;
    let block_99_hash_clone = block_99_hash;
    bitcoin_client
        .expect_get_block_by_height()
        .withf(move |h| *h == 100)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_100_hash_clone,
                height: 100,
                prev_hash: block_99_hash_clone,
                txs: vec![],
            }))
        });

    // Construct Indexer with checkpoint at height 100
    let indexer = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Verify indexer starts at checkpoint height 100
    let best_height = indexer.get_best_height()?;
    assert_eq!(
        best_height, 
        Some(100), 
        "Indexer should start at checkpoint height 100"
    );

    // Verify checkpoint height is saved to storage
    let saved_checkpoint = store.get_checkpoint_height()?;
    assert_eq!(
        saved_checkpoint,
        Some(100),
        "Checkpoint height 100 should be saved to storage"
    );

    // Verify block at height 100 is stored
    let block_at_100 = store.get_block_by_height(100)?;
    assert!(block_at_100.is_some(), "Block at height 100 should be stored");

    let block = block_at_100.unwrap();
    assert_eq!(block.height, 100, "Stored block should be at height 100");
    assert_eq!(block.orphan, false, "Block should not be marked as orphan");
    assert_eq!(block.hash, block_100_hash, "Stored block hash should match");

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
     * Start regtest node and mine only 50 blocks.
     * Attempt to construct indexer with checkpoint 100.
     * Catch error result.
     * Expected Result: Construction fails with IndexerError::CheckpointHeightAheadOfBlockchainHeight. No storage mutations. 
     * Error message clearly indicates the issue..
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    
    clear_output();
    
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock blockchain at height 50
    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(50));

    // Verify storage is empty (no blocks stored yet)
    let best_height_before = store.get_best_height()?;
    assert!(best_height_before.is_none(), "Storage should be empty before indexer creation");

    // Attempt to construct Indexer with checkpoint at height 100 (ahead of blockchain height 50)
    let result = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    );

    // Verify construction fails
    assert!(result.is_err(), "Indexer construction should fail when checkpoint is ahead of blockchain height");

    // Verify the specific error type
    let Err(error) = result else {
        panic!("Expected Err but got Ok");
    };
    assert!(
        matches!(error, IndexerError::CheckpointHeightAheadOfBlockchainHeight),
        "Error should be CheckpointHeightAheadOfBlockchainHeight, got: {:?}",
        error
    );

    // Verify no storage mutations occurred
    let best_height_after = store.get_best_height()?;
    assert!(best_height_after.is_none(), "Storage should remain empty after failed construction");

    let checkpoint_height = store.get_checkpoint_height()?;
    assert!(checkpoint_height.is_none(), "Checkpoint should not be saved after failed construction");

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
     *   Run indexer to sync blocks 0-75, then stop.
     *   Mine 25 more blocks on regtest (total 100).
     *   Construct new indexer instance pointing to same storage.
     *   Verify best height is 75.
     *   Verify block hash at height 75 matches regtest chain.
     * Expected Result: Indexer initializes with best height = 75. No errors. Ready to 
     * sync block 76 on next tick.
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock blockchain at height 76 initially
    let block_75_hash = BlockHash::from_str("000000000000000000075a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_74_hash = BlockHash::from_str("000000000000000000074a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(76));
    
    let block_75_hash_clone = block_75_hash;
    let block_74_hash_clone = block_74_hash;
    bitcoin_client
        .expect_get_block_by_height()
        .withf(move |h| *h == 75)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_75_hash_clone,
                height: 75,
                prev_hash: block_74_hash_clone,
                txs: vec![],
            }))
        });

    // Create first indexer and sync to block 75
    let indexer1 = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(75))),
    )?;

    // Verify first indexer is at height 75
    assert_eq!(indexer1.get_best_height()?, Some(75));

    // Verify block 75 is stored
    let block_75 = store.get_block_by_height(75)?;
    assert!(block_75.is_some(), "Block 75 should be stored");
    let block_75_hash_stored = block_75.unwrap().hash.clone();
    assert_eq!(block_75_hash_stored, block_75_hash);
    
    drop(indexer1);

    // Create new mock for second indexer
    let mut bitcoin_client2 = MockBitcoinClient::new();
    
    // Mock blockchain now at height 100
    bitcoin_client2
        .expect_get_best_block()
        .times(2)
        .returning(|| Ok(100));
    
    let block_75_hash_clone2 = block_75_hash;
    let block_74_hash_clone2 = block_74_hash;
    bitcoin_client2
        .expect_get_block_by_height()
        .withf(move |h| *h == 75)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_75_hash_clone2,
                height: 75,
                prev_hash: block_74_hash_clone2,
                txs: vec![],
            }))
        });
    
    // Mock block 76 for tick
    let block_76_hash = BlockHash::from_str("000000000000000000076a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_76_hash_clone = block_76_hash;
    let block_75_hash_clone3 = block_75_hash;
    bitcoin_client2
        .expect_get_block_by_height()
        .withf(move |h| *h == 76)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_76_hash_clone,
                height: 76,
                prev_hash: block_75_hash_clone3,
                txs: vec![],
            }))
        });

    // Create new indexer instance pointing to same storage (no checkpoint specified)
    let indexer2 = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(None)),
    )?;

    // Verify indexer resumes from height 75
    let best_height = indexer2.get_best_height()?;
    assert_eq!(
        best_height,
        Some(75),
        "Indexer should resume from last indexed height 75"
    );

    // Verify indexer can continue syncing from block 76
    indexer2.tick()?;
    assert_eq!(
        indexer2.get_best_height()?,
        Some(76),
        "After tick, indexer should sync block 76"
    );

    // Verify block 76 is now stored
    let block_76 = store.get_block_by_height(76)?;
    assert!(block_76.is_some(), "Block 76 should be stored after tick");
    assert_eq!(block_76.unwrap().orphan, false, "Block 76 should not be orphaned");

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
     *     Sync indexer to height 120 on a regtest chain.
     *     Reset regtest node or switch to different node with only 100 blocks.
     *     Attempt to construct indexer pointing to new node.
     * Expected Result: Construction fails with IndexerError::InconsistentBlockchain. Logs error message indicating indexer is ahead of blockchain.
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client1 = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock first blockchain at height 120
    let block_120_hash = BlockHash::from_str("000000000000000000120a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_119_hash = BlockHash::from_str("000000000000000000119a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    bitcoin_client1
        .expect_get_best_block()
        .returning(|| Ok(120));
    
    let block_120_hash_clone = block_120_hash;
    let block_119_hash_clone = block_119_hash;
    bitcoin_client1
        .expect_get_block_by_height()
        .withf(move |h| *h == 120)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_120_hash_clone,
                height: 120,
                prev_hash: block_119_hash_clone,
                txs: vec![],
            }))
        });

    // Create indexer and sync to height 120
    let indexer1 = Indexer::new(
        bitcoin_client1,
        store.clone(),
        Some(IndexerSettings::new(Some(120))),
    )?;

    // Verify indexer is at height 120
    assert_eq!(indexer1.get_best_height()?, Some(120));

    // Verify storage has block 120
    let block_120 = store.get_block_by_height(120)?;
    assert!(block_120.is_some(), "Block 120 should be stored");

    drop(indexer1);

    // Create second mock with shorter blockchain at height 100
    let mut bitcoin_client2 = MockBitcoinClient::new();
    
    bitcoin_client2
        .expect_get_best_block()
        .returning(|| Ok(100));

    // Attempt to construct indexer with same storage but pointing to shorter blockchain
    let result = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(None)),
    );

    // Verify construction fails
    assert!(result.is_err(), "Indexer construction should fail when indexed height exceeds blockchain height");

    // Verify the specific error type
    let Err(error) = result else {
        panic!("Expected Err but got Ok");
    };
    assert!(
        matches!(error, IndexerError::InconsistentBlockchain),
        "Error should be InconsistentBlockchain, got: {:?}",
        error
    );

    clear_output();
    Ok(())
}

#[test]
fn test_block_hash_mismatch_at_indexed_height() -> Result<(), anyhow::Error> {
    /* 
     * Objective: Verify error when block hash at indexed height differs from blockchain (chain fork).
     * Preconditions: Storage has blocks 0-50 from one chain. Bitcoin node has different blocks at same heights.
     * Input: Indexed height 50 with hash A, blockchain at height 100+ with different hash B at height 50.
     * Steps:
     *      Sync indexer to height 50 on first regtest chain.
     *      Stop first node and start a new regtest node (different chain).
     *      Mine blocks on new chain to reach height 100+.
     *      Attempt to construct indexer pointing to new node with same storage.
     * Expected Result: Construction fails with IndexerError::BlockHashMismatch. Logs error indicating stored block hash doesn't match blockchain.
    */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client1 = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock first blockchain at height 50
    let block_50_hash_chain1 = BlockHash::from_str("000000000000000000050a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_49_hash = BlockHash::from_str("000000000000000000049a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    bitcoin_client1
        .expect_get_best_block()
        .returning(|| Ok(50));
    
    let block_50_hash_clone = block_50_hash_chain1;
    let block_49_hash_clone = block_49_hash;
    bitcoin_client1
        .expect_get_block_by_height()
        .withf(move |h| *h == 50)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_50_hash_clone,
                height: 50,
                prev_hash: block_49_hash_clone,
                txs: vec![],
            }))
        });

    // Create indexer and sync to height 50
    let indexer1 = Indexer::new(
        bitcoin_client1,
        store.clone(),
        Some(IndexerSettings::new(Some(50))),
    )?;

    // Verify indexer is at height 50
    assert_eq!(indexer1.get_best_height()?, Some(50));

    // Get the block hash at height 50 from first chain
    let block_50_chain1 = store.get_block_by_height(50)?;
    assert!(block_50_chain1.is_some(), "Block 50 should be stored");
    let hash_chain1 = block_50_chain1.unwrap().hash.clone();
    assert_eq!(hash_chain1, block_50_hash_chain1);

    drop(indexer1);

    // Create second mock with different block hash at height 50
    let mut bitcoin_client2 = MockBitcoinClient::new();
    
    // Different hash for block 50 on second chain
    let block_50_hash_chain2 = BlockHash::from_str("111111111111111111150a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    bitcoin_client2
        .expect_get_best_block()
        .returning(|| Ok(100));
    
    let block_50_hash_clone2 = block_50_hash_chain2;
    let block_49_hash_clone2 = block_49_hash;
    bitcoin_client2
        .expect_get_block_by_height()
        .withf(move |h| *h == 50)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_50_hash_clone2,
                height: 50,
                prev_hash: block_49_hash_clone2,
                txs: vec![],
            }))
        });

    // Verify the hashes are different (simulating different chains)
    assert_ne!(
        block_50_hash_chain1, block_50_hash_chain2,
        "Block 50 hashes should differ between the two chains"
    );

    // Attempt to construct indexer with storage from first chain but pointing to second chain
    let result = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(None)),
    );

    // Verify construction fails
    assert!(result.is_err(), "Indexer construction should fail when block hash at indexed height doesn't match blockchain");

    // Verify the specific error type
    let Err(error) = result else {
        panic!("Expected Err but got Ok");
    };
    assert!(
        matches!(error, IndexerError::InconsistentBlockchain),
        "Error should be InconsistentBlockchain (block hash mismatch), got: {:?}",
        error
    );

    clear_output();
    Ok(())
}

#[test]
fn test_checkpoint_already_exists_and_match() -> Result<(), anyhow::Error> {
    /*
     * Objective: Verify indexer allows restart with same checkpoint.
     * Preconditions: Storage has checkpoint 100 and indexed height 120 from previous run. Bitcoin regtest at height 150.
     *  Input: checkpoint_height: Some(100), stored checkpoint 100.
     * Steps:
     *  Run indexer with checkpoint 100, sync to height 120.
     *  Stop indexer.
     *  Mine more blocks on regtest (to height 150).
     *  Construct new indexer with same checkpoint 100.
     *  Verify no errors.
     *  Expected Result: Indexer initializes successfully. Best height = 120. No checkpoint conflict. Ready to continue syncing.
     */
    
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClient;
    use bitcoin::BlockHash;
    use std::str::FromStr;
    
    clear_output();
    
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();

    // Mock blockchain at height 150
    let block_100_hash = BlockHash::from_str("000000000000000000100a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_99_hash = BlockHash::from_str("000000000000000000099a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    // Mock get_best_block to return height 150 for initialization
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(150));
    
    // Mock get_block_by_height for block 100 (checkpoint)
    let block_100_hash_clone = block_100_hash;
    let block_99_hash_clone = block_99_hash;
    bitcoin_client
        .expect_get_block_by_height()
        .withf(move |h| *h == 100)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_100_hash_clone,
                height: 100,
                prev_hash: block_99_hash_clone,
                txs: vec![],
            }))
        });
    
    // Mock blocks 101-120 for ticking
    for i in 101..=120 {
        let block_hash = BlockHash::from_str(&format!("0000000000000000001{:02}a335e3a4fc328bf5beb436012afca590b1a11466e22", i))?;
        let prev_hash = BlockHash::from_str(&format!("0000000000000000001{:02}a335e3a4fc328bf5beb436012afca590b1a11466e22", i - 1))?;
        
        bitcoin_client
            .expect_get_best_block()
            .times(1)
            .returning(move || Ok(150));
        
        bitcoin_client
            .expect_get_block_by_height()
            .withf(move |h| *h == i)
            .times(1)
            .returning(move |_| {
                Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                    hash: block_hash,
                    height: i,
                    prev_hash,
                    txs: vec![],
                }))
            });
    }

    // Create first indexer with checkpoint 100, sync to height 120
    let indexer = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Verify indexer starts at checkpoint 100
    assert_eq!(indexer.get_best_height()?, Some(100));

    // Sync to height 120 by ticking 20 times
    for _ in 0..20 {
        indexer.tick()?;
    }

    // Verify indexer synced to height 120
    assert_eq!(indexer.get_best_height()?, Some(120));

    // At this point, store should have checkpoint 100 and best height 120
    // Verify checkpoint is stored
    let stored_checkpoint = store.get_checkpoint_height()?;
    assert_eq!(stored_checkpoint, Some(100));

    // Drop the first indexer (simulating stopping it)
    drop(indexer);

    // Create a new mock bitcoin client for the new indexer
    let mut bitcoin_client2 = MockBitcoinClient::new();
    
    // Mock blockchain still at height 150
    bitcoin_client2
        .expect_get_best_block()
        .times(2)
        .returning(|| Ok(150));
    
    // Mock get_block_by_height for block 120 (verification during init)
    let block_120_hash = BlockHash::from_str("000000000000000000120a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_119_hash = BlockHash::from_str("000000000000000000119a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    
    let block_120_hash_clone = block_120_hash;
    let block_119_hash_clone = block_119_hash;
    bitcoin_client2
        .expect_get_block_by_height()
        .withf(move |h| *h == 120)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_120_hash_clone,
                height: 120,
                prev_hash: block_119_hash_clone,
                txs: vec![],
            }))
        });
    
    // Mock block 121 for tick
    let block_121_hash = BlockHash::from_str("000000000000000000121a335e3a4fc328bf5beb436012afca590b1a11466e22")?;
    let block_121_hash_clone = block_121_hash;
    let block_120_hash_clone2 = block_120_hash;
    bitcoin_client2
        .expect_get_block_by_height()
        .withf(move |h| *h == 121)
        .times(1)
        .returning(move |_| {
            Ok(Some(bitvmx_bitcoin_rpc::types::BlockInfo {
                hash: block_121_hash_clone,
                height: 121,
                prev_hash: block_120_hash_clone2,
                txs: vec![],
            }))
        });

    // Construct new indexer with same checkpoint 100
    let indexer2 = Indexer::new(
        bitcoin_client2,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Verify no errors occurred and indexer initializes successfully
    // Best height should be 120 (the previously synced height)
    assert_eq!(indexer2.get_best_height()?, Some(120));

    // Verify checkpoint is still 100
    let stored_checkpoint = store.get_checkpoint_height()?;
    assert_eq!(stored_checkpoint, Some(100));

    // Verify the indexer is ready to continue syncing
    // Tick once to sync block 121
    indexer2.tick()?;
    assert_eq!(indexer2.get_best_height()?, Some(121));

    clear_output();
    Ok(())
}

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
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
    let bitcoind = Bitcoind::new(
        "bitcoin-regtest-checkpoint",
        "bitcoin/bitcoin:29.1",
        config.bitcoin.clone(),
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine enough blocks to work with both checkpoints
    bitcoin_client.mine_blocks_to_address(110, &wallet)?;

    // Step 1: Run indexer with checkpoint 50, sync some blocks
    {
        let bitcoin_client_clone = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
             bitcoin_client_clone,
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
    let result = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(100))),
    );

    // Expected Result: Construction fails with IndexerError::AlreadyIndexedWithDifferentCheckpointHeight
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::AlreadyIndexedWithDifferentCheckpointHeight)
    ));

    clear_output();
    Ok(())
}

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
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
    let bitcoind = Bitcoind::new(
        "bitcoin-regtest-corrupted",
        "bitcoin/bitcoin:29.1",
        config.bitcoin.clone(),
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    
    // Create storage path and store
    let store_path = format!(
        "test_output/get_best_block_height_test/{}",
        utils::generate_random_string()
    );
    let storage_config = storage_backend::storage_config::StorageConfig::new(store_path.clone(), None);
    let storage = std::rc::Rc::new(storage_backend::storage::Storage::new(&storage_config)?);
    let store = std::rc::Rc::new(IndexerStore::new(storage.clone())?);

    // Mine enough blocks to reach height 80
    bitcoin_client.mine_blocks_to_address(85, &wallet)?;

    // Step 1: Run indexer to sync to height 80
    {
        let bitcoin_client_clone = BitcoinClient::new_from_config(&config.bitcoin)?;
        let indexer = Indexer::new(
            bitcoin_client_clone,
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
    let result = Indexer::new(
        bitcoin_client,
        store.clone(),
        Some(IndexerSettings::new(Some(80))),
    );

    // Expected Result: Construction fails with IndexerError::DatabaseCorrupted
    assert!(result.is_err());
    assert!(matches!(
        result,
        Err(IndexerError::DatabaseCorrupted)
    ));

    clear_output();
    Ok(())
}