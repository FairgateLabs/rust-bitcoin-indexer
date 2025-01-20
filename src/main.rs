use anyhow::{Context, Result};
use bitcoin_indexer::{
    config::ConfigIndexer,
    helper::define_height_to_sync,
    indexer::{Indexer, IndexerApi},
    store::{IndexerStore, StoreClient},
};
use bitvmx_bitcoin_rpc::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    types::BlockHeight,
};
use bitvmx_settings::settings;
use tracing::info;
use std::{ path::PathBuf, rc::Rc, sync::mpsc::channel, thread, time::Duration};
use storage_backend::storage::Storage;

fn main() -> Result<()> {
    let (tx, rx) = channel();

    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let config = settings::load::<ConfigIndexer>()?;

    let log_level = match config.log_level {
        Some(level) => {
            level.parse().unwrap_or(tracing::Level::INFO)   
        },
        None => tracing::Level::INFO,     
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .init();

    let bitcoin_client =
        BitcoinClient::new(&config.rpc.url, &config.rpc.username, &config.rpc.password)?;
    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

    let network = bitcoin_client.get_blockchain_info()?;
    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);

    let storage = Rc::new(Storage::new_with_path(&PathBuf::from(config.db_file_path))?);
    let indexer_store = IndexerStore::new(storage)?;
    let best_block = indexer_store.get_best_block()?;
    let best_block_height = best_block.map(|block| block.height);
    let mut height_to_sync =
        define_height_to_sync(config.checkpoint_height, blockchain_height, best_block_height)?;
    info!("Start synchronizing from {}H", height_to_sync);

    let indexer = Indexer::new(bitcoin_client, indexer_store);

    let mut prev_height = 0;

    loop {
        if rx.try_recv().is_ok() {
            info!("Stop Bitcoin Indexer");
            break;
        }

        height_to_sync = indexer.tick(&height_to_sync).context("Indexing failed")?;

        if prev_height == height_to_sync {
            info!("Waitting for a new block...");
            thread::sleep(Duration::from_secs(10));
        } else {
            prev_height = height_to_sync;
        }
    }

    Ok(())
}