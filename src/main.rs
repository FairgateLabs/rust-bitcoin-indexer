use anyhow::{Context, Result};
use clap::Parser;
use log::{error, info, warn};
use rust_bitcoin_indexer::{
    args::Args,
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    helper::define_height_to_sync,
    indexer::Indexer,
    store::{Store, StoreClient},
    types::BlockHeight,
};
use std::{env, sync::Arc};

fn main() -> Result<()> {
    dotenv::dotenv().context("There was an error loading .env file")?;
    env_logger::init();

    let args = Args::parse();

    let db_file_path: String = args
        .db_file_path
        .or_else(|| env::var("DB_FILE_PATH").ok())
        .context("No Bitcoin database file path provided")?;

    let node_rpc_url: String = args
        .node_rpc_url
        .or_else(|| env::var("NODE_RPC_URL").ok())
        .context("No Bitcoin rpc url provided")?;

    let checkpoint_height: Option<u32> = get_checkpoint()?;
    let bitcoin_client = BitcoinClient::new(&node_rpc_url)?;
    let store = Store::new(&db_file_path)?;

    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;
    let network = bitcoin_client.get_blockchain_info()?;

    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);

    let indexed_height = store.get_best_block_height()?;
    let height_to_sync =
        define_height_to_sync(checkpoint_height, blockchain_height, indexed_height)?;
    info!("Start synchronizing from {}H", height_to_sync);

    let indexer = Indexer::new(Arc::new(bitcoin_client), Arc::new(store))?;

    loop {
        let next_index = indexer.index_height(&height_to_sync);

        if let Err(err) = next_index {
            error!("Error: {:?}", err);
            std::process::exit(1);
        }
    }
}

fn get_checkpoint() -> Result<Option<u32>, anyhow::Error> {
    let checkpoint = env::var("CHECKPOINT_HEIGHT");
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
