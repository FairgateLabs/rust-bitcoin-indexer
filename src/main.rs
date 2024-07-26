use anyhow::{Context, Result};
use clap::Parser;
use log::{error, warn};
use rust_bitcoin_indexer::{
    args::Args, bitcoin_client::BitcoinClient, indexer::Indexer, store::Store, types::BlockHeight,
};
use std::env;

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
    let mut indexer = Indexer::new(Box::new(bitcoin_client), Box::new(store), checkpoint_height)?;
    let a = indexer.run();

    if let Err(err) = a {
        error!("Error: {:?}", err);
        std::process::exit(1);
    }

    Ok(())
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
