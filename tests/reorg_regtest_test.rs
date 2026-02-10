use anyhow::Result;
use bitcoin::Network;
use bitcoin_indexer::{
    config::IndexerConfig,
    indexer::{Indexer, IndexerApi},
};
use bitcoind::{bitcoind::Bitcoind, config::BitcoindConfig};
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitvmx_settings::settings;
use tracing::info;
mod utils;
use crate::utils::{clear_output, get_indexer_store, get_random_pubkey};

#[test]
fn reorganization_test() -> Result<(), anyhow::Error> {
    clear_output();

    let config = settings::load::<IndexerConfig>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::ERROR),
        None => tracing::Level::INFO,
    };

    let _ = tracing_subscriber::fmt()
        .with_max_level(log_level)
        .try_init();

    let bitcoind_config = BitcoindConfig::default();

    let bitcoind = Bitcoind::new(bitcoind_config, config.bitcoin.clone(), None);

    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let indexer_store = get_indexer_store();
    let indexer = Indexer::new(bitcoin_client, indexer_store.clone(), None)?;
    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;

    info!("Mining 110 blocks to wallet");
    bitcoin_client.mine_blocks_to_address(110, &wallet)?;

    info!("Indexing 110 blocks");
    for _ in 0..110 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    info!("Making 3 more ticks to ensure that the indexer does not process the next block");
    for _ in 0..3 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    info!("Mining 10 more blocks");
    bitcoin_client.mine_blocks_to_address(10, &wallet)?;

    info!("Making 10 more ticks");
    for _ in 0..10 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 120 and the blockchain is at height 120");
    assert_eq!(indexer.get_best_height()?, Some(120));
    assert_eq!(bitcoin_client.get_best_block()?, 120);

    info!("Invalidating the last 10 blocks");
    bitcoin_client.invalidate_block(&bitcoin_client.get_block_by_height(&111)?.unwrap().hash)?;

    info!("Making 2 ticks");
    indexer.tick()?;
    indexer.tick()?;

    info!("Checking after ticks that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    info!("Making 2 more ticks");
    indexer.tick()?;

    info!("Checking that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    info!("Mining 10 more blocks");
    let user_pubkey = get_random_pubkey();
    let wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(10, &wallet)?;

    info!("Making 10 more ticks");
    for _ in 0..10 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(120));
    assert_eq!(bitcoin_client.get_best_block()?, 120);

    info!("Invalidating 30 blocks and mining 50 blocks more, then the indexer will reorg and see 20 blocks ahead.");
    bitcoin_client.invalidate_block(&bitcoin_client.get_block_by_height(&91)?.unwrap().hash)?;
    let user_pubkey = get_random_pubkey();
    let wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest)?;
    bitcoin_client.mine_blocks_to_address(50, &wallet)?;

    info!("Making 80 ticks to ensure that the indexer rolls back and processes new blocks");
    for _ in 0..80 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 140 and the blockchain is at height 140");
    assert_eq!(indexer.get_best_height()?, Some(140));
    assert_eq!(bitcoin_client.get_best_block()?, 140);

    bitcoind.stop()?;
    clear_output();

    Ok(())
}

#[test]
fn reorg_marks_last_three_blocks_as_orphan() -> Result<(), anyhow::Error> {
    clear_output();

    let config = settings::load::<IndexerConfig>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::ERROR),
        None => tracing::Level::INFO,
    };

    let _ = tracing_subscriber::fmt()
        .with_max_level(log_level)
        .try_init();

    let bitcoind_config = BitcoindConfig::default();

    let bitcoind = Bitcoind::new(bitcoind_config, config.bitcoin.clone(), None);

    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("reorg_orphans_wallet")?;
    let indexer_store = get_indexer_store();
    let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(bitcoin_client_for_indexer, indexer_store.clone(), None)?;

    info!("Mining 100 blocks to wallet");
    bitcoin_client.mine_blocks_to_address(100, &wallet)?;

    info!("Indexing 100 blocks");
    for _ in 0..100 {
        indexer.tick()?;
    }

    info!("Checking that the indexer and blockchain are at height 100");
    assert_eq!(indexer.get_best_height()?, Some(100));
    assert_eq!(bitcoin_client.get_best_block()?, 100);

    // Capture the hashes of the last 3 blocks before invalidation.
    let block_98 = bitcoin_client
        .get_block_by_height(&98)?
        .expect("block at height 98 must exist");
    let block_99 = bitcoin_client
        .get_block_by_height(&99)?
        .expect("block at height 99 must exist");
    let block_100 = bitcoin_client
        .get_block_by_height(&100)?
        .expect("block at height 100 must exist");

    info!("Invalidating the last 3 blocks (heights 98, 99 and 100)");
    bitcoin_client.invalidate_block(&block_100.hash)?;
    bitcoin_client.invalidate_block(&block_99.hash)?;
    bitcoin_client.invalidate_block(&block_98.hash)?;

    info!("Ticking once to let the indexer detect the rollback");
    indexer.tick()?;

    info!("Checking that the indexer and blockchain best height is now 97");
    assert_eq!(bitcoin_client.get_best_block()?, 97);
    assert_eq!(indexer.get_best_height()?, Some(97));

    info!("Checking that blocks 98, 99 and 100 are marked as orphan");
    for height in 98..=100 {
        let block = indexer
            .get_block_by_height(height)?
            .expect("block must exist in indexer store");
        assert!(
            block.orphan,
            "block at height {} should be marked as orphan",
            height
        );
    }

    clear_output();

    Ok(())
}
