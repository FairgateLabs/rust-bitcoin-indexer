use anyhow::{Context, Result};
use clap::Parser;
use rust_bitcoin_indexer::args::Args;
use std::env;

fn main() -> Result<()> {
    dotenv::dotenv().context("There was an error loading .env file")?;
    env_logger::init();

    let args = Args::parse();

    let db_file_path = args
        .db_file_path
        .or_else(|| env::var("DB_FILE_PATH").ok())
        .context("No Bitcoin database file path provided")?;

    let node_rpc_url = args
        .node_rpc_url
        .or_else(|| env::var("NODE_RPC_URL").ok())
        .context("No Bitcoin rpc url provided")?;

    let config = Config {
        db_file_path,
        node_rpc_url,
    };

    let indexer = Indexer::new(config);
    indexer.run();

    Ok(())
}
