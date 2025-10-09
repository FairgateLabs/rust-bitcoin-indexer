use anyhow::Result;
use bitcoin::Network;
use bitcoin_indexer::{
    config::IndexerConfig,
    indexer::{Indexer, IndexerApi},
};
use bitcoind::bitcoind::Bitcoind;
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitvmx_settings::settings;
use tracing::info;
mod utils;
use crate::utils::{clear_output, get_indexer_store, get_random_pubkey};

#[test]
#[ignore = "This test is ignored because it uses a real Bitcoin node, which is not available in CI"]
fn reorganization_test() -> Result<(), anyhow::Error> {
    clear_output();

    let config = settings::load::<IndexerConfig>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::ERROR),
        None => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let bitcoind = Bitcoind::new(
        "bitcoin-regtest",
        "bitcoin/bitcoin:29.1",
        config.bitcoin.clone(),
    );

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
    assert_eq!(indexer.get_height_to_sync()?, 110);

    info!("Making 3 more ticks to ensure that the indexer does not process the next block");
    for _ in 0..3 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);
    assert_eq!(indexer.get_height_to_sync()?, 110);

    info!("Mining 10 more blocks");
    bitcoin_client.mine_blocks_to_address(10, &wallet)?;

    info!("Making 10 more ticks");
    for _ in 0..10 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 120 and the blockchain is at height 120");
    assert_eq!(indexer.get_best_height()?, Some(120));
    assert_eq!(bitcoin_client.get_best_block()?, 120);
    assert_eq!(indexer.get_height_to_sync()?, 120);

    info!("Invalidating the last 10 blocks");
    bitcoin_client.invalidate_block(&bitcoin_client.get_block_by_height(&111)?.unwrap().hash)?;

    info!("Making 2 ticks");
    indexer.tick()?;
    indexer.tick()?;

    info!("Checking after ticks that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);
    assert_eq!(indexer.get_height_to_sync()?, 110);

    info!("Making 2 more ticks");
    indexer.tick()?;

    info!("Checking that the indexer is at height 110 and the blockchain is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);
    assert_eq!(indexer.get_height_to_sync()?, 110);

    info!("Mining 10 more blocks");
    let user_pubkey = get_random_pubkey();
    let wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest);
    bitcoin_client.mine_blocks_to_address(10, &wallet)?;

    info!("Making 10 more ticks");
    for _ in 0..10 {
        indexer.tick()?;
    }
    assert_eq!(indexer.get_best_height()?, Some(120));
    assert_eq!(bitcoin_client.get_best_block()?, 120);
    assert_eq!(indexer.get_height_to_sync()?, 120);

    info!("Invalidating 30 blocks and mining 50 blocks more, then the indexer will reorg and see 20 blocks ahead.");
    bitcoin_client.invalidate_block(&bitcoin_client.get_block_by_height(&91)?.unwrap().hash)?;
    let user_pubkey = get_random_pubkey();
    let wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest);
    bitcoin_client.mine_blocks_to_address(50, &wallet)?;

    info!("Making 80 ticks to ensure that the indexer rolls back and processes new blocks");
    for _ in 0..80 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 140 and the blockchain is at height 140");
    assert_eq!(indexer.get_best_height()?, Some(140));
    assert_eq!(bitcoin_client.get_best_block()?, 140);
    assert_eq!(indexer.get_height_to_sync()?, 140);

    clear_output();

    Ok(())
}
