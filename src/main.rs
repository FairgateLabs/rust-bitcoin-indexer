use anyhow::Result;
use bitcoin::Network;
use bitcoin_indexer::{
    config::IndexerConfig,
    indexer::{Indexer, IndexerApi},
    store::IndexerStore,
};
use bitcoind::bitcoind::Bitcoind;
use bitvmx_bitcoin_rpc::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    types::BlockHeight,
};
use bitvmx_settings::settings;
use std::rc::Rc;
use storage_backend::storage::Storage;
use tracing::info;

fn main() -> Result<(), anyhow::Error> {
    let config = settings::load::<IndexerConfig>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::INFO),
        None => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let bitcoind = Bitcoind::new(
        "bitcoin-regtest",
        "ruimarinho/bitcoin-core",
        config.bitcoin.clone(),
    );

    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet(Network::Regtest, "test_wallet")?;

    info!("Mining 100 blocks to wallet");
    bitcoin_client.mine_blocks_to_address(100, &wallet)?;
    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

    let network = bitcoin_client.get_blockchain_info()?.chain;
    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);
    let storage = Rc::new(Storage::new(&config.storage)?);
    let indexer_store = Rc::new(IndexerStore::new(storage)?);
    let indexer = Indexer::new(bitcoin_client, indexer_store.clone(), config.constants)?;

    for _ in 0..150 {
        indexer.tick()?;
    }

    bitcoind.stop()?;
    clear_data();

    Ok(())
}

pub fn clear_data() {
    let _ = std::fs::remove_dir_all("data");
}
