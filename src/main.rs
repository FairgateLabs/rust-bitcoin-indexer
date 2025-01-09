use anyhow::{Context, Result};
use bitcoin_indexer::{
    args::Args,
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    helper::define_height_to_sync,
    indexer::{Indexer, IndexerApi},
    store::{IndexerStore, StoreClient},
    types::BlockHeight,
};
use clap::Parser;
use log::{info, warn};
use std::{env, path::PathBuf, rc::Rc, sync::mpsc::channel, thread, time::Duration};
use storage_backend::storage::Storage;

fn main() -> Result<()> {
    let (tx, rx) = channel();

    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let envs = dotenv::dotenv();

    if envs.is_err() {
        warn!("No .env file found");
    }

    env_logger::init();

    let args = Args::parse();

    let db_file_path: String = args
        .db_file_path
        .or_else(|| env::var("DB_FILE_PATH").ok())
        .context("No Bitcoin database file path provided")?;

    let rpc_url: String = args
        .rpc_url
        .or_else(|| env::var("RPC_URL").ok())
        .context("No Bitcoin rpc url provided")?;

    let checkpoint_height: Option<u32> = get_checkpoint()?;
    let bitcoin_client = BitcoinClient::new(&rpc_url)?;
    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

    let network = bitcoin_client.get_blockchain_info()?;
    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);

    let storage = Rc::new(Storage::new_with_path(&PathBuf::from(db_file_path))?);
    let indexer_store = IndexerStore::new(storage)?;
    let best_block = indexer_store.get_best_block()?;
    let best_block_height = best_block.map(|block| block.height);
    let mut height_to_sync =
        define_height_to_sync(checkpoint_height, blockchain_height, best_block_height)?;
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

fn get_checkpoint() -> Result<Option<u32>> {
    let checkpoint = env::var("CHECKPOINT_HEIGHT_BLOCK");
    info!("Checkpoint {:?}", checkpoint);
    let mut checkpoint_height = None;

    if checkpoint.is_ok() {
        checkpoint_height = match checkpoint?.parse::<BlockHeight>() {
            Ok(checkpoint_height) => Some(checkpoint_height),
            Err(_) => {
                warn!("Checkpoint height must be a positive integer");
                None
            }
        };
    }

    Ok(checkpoint_height)
}
