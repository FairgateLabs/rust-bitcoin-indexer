use anyhow::{bail, Context, Result};
use clap::Parser;
use log::{error, info, warn};
use rust_bitcoin_indexer::{
    args::Args,
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    indexer::Indexer,
    store::{Store, StoreClient},
    types::BlockHeight,
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
    let mut store = Store::new(&db_file_path)?;

    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;
    let network = bitcoin_client.get_blockchain_info()?;

    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);

    let indexed_height = store.get_best_block_height()?;
    let height_to_sync =
        define_height_to_sync(checkpoint_height, blockchain_height, indexed_height)?;
    info!("Start synchronizing from {}H", height_to_sync);

    let mut indexer = Indexer::new(Box::new(bitcoin_client), Box::new(store), height_to_sync)?;

    loop {
        let indexer = indexer.sync();

        if let Err(err) = indexer {
            error!("Error: {:?}", err);
            std::process::exit(1);
        }
    }
}

fn define_height_to_sync(
    checkpoint_height: Option<BlockHeight>,
    blockchain_height: BlockHeight,
    indexed_height: Option<BlockHeight>,
) -> Result<BlockHeight> {
    // blockchain_height: The current block height of the Bitcoin network.
    // checkpoint_height: The starting block height for synchronization.
    // indexed_height: The highest block height that has already been synchronized and stored in the storage.

    if indexed_height.is_some() {
        info!("Last indexed block is {:?}H", indexed_height.unwrap());
    } else {
        info!("No block indexed");
    }

    let mut height_to_sync: u32 = indexed_height.unwrap_or(0);

    if checkpoint_height.is_some() {
        let checkpoint = checkpoint_height.unwrap();

        if checkpoint < height_to_sync {
            warn!("Passed CHECKPOINT_HEIGHT command line is behind last indexed height");
        }

        info!("Using CHECKPOINT_HEIGHT={}H to start to sync", checkpoint);

        height_to_sync = checkpoint;
    }

    // ERROR if blockchain_height < start_height
    if blockchain_height < height_to_sync {
        let error =
            "The current block height of the Bitcoin network is behind the starting block to sync";
        error!("{}", error);
        bail!(error);
    }

    if height_to_sync > 0 && checkpoint_height.is_none() {
        height_to_sync += 1
    }

    Ok(height_to_sync)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn define_height_to_sync_test() -> Result<(), anyhow::Error> {
        // Tests:

        // Test
        // checkpoint_height: None | blockchain_height: 100 | indexed_height: None
        let start_height = define_height_to_sync(None, 100, None)?;
        // Then start_height should be height 0 (no checkpoint, no block indexed)
        assert_eq!(start_height, 0);

        // Test
        // checkpoint_height: None | blockchain_height: 100 | indexed_height: 40
        let start_height = define_height_to_sync(None, 100, Some(40))?;
        // Then start_height should be height 41 (indexed_height + 1 )
        assert_eq!(start_height, 41);

        // Test
        // checkpoint_height: 10000 | blockchain_height: 100 | indexed_height: None
        let start_height = define_height_to_sync(Some(10000), 100, None);
        // Checkpoint can not be bigger than blockchain_height
        assert!(start_height.is_err());

        // Test
        // checkpoint_height: 40 | blockchain_height: 100 | indexed_height: None
        let start_height = define_height_to_sync(Some(40), 100, None)?;
        // Then start_height should be height 40 (checkpoint_height should rule)
        assert_eq!(start_height, 40);

        // Test
        // checkpoint_height 100 | blockchain_height 100 | indexed_height 100
        let start_height = define_height_to_sync(Some(100), 100, Some(100))?;
        // Then start_height should be height 100 (checkpoint should rule)
        assert_eq!(start_height, 100);

        Ok(())
    }
}
