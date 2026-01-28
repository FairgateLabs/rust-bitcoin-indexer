//! # Fee Rate Monitoring Example
//!
//! This example demonstrates how to use the bitcoin indexer to monitor estimated fee rates
//! from a Bitcoin node. It's designed to work with an external Bitcoin node configured
//! through the `config/development.yaml` file.
//!
//! ## Configuration
//!
//! The example reads configuration from `config/development.yaml` and can be used with:
//! regtest, testnet, or mainnet (bitcoin).
//!
//! When using regtest, ensure your regtest blocks are been filled with at least 6 transactions
//!
//! ## Usage
//!
//! 1. Configure your Bitcoin node connection in `config/development.yaml`
//! 2. Ensure your Bitcoin node is running and accessible
//! 3. Run the example: `cargo run --example feerate`
//! 4. Observe the fee rate estimates printed to the console
//! 5. Press Ctrl+C to gracefully shutdown
//!
//! ## Behavior
//!
//! The example will:
//! - Connect to the configured Bitcoin node
//! - Start the indexer and continuously sync with the blockchain
//! - Query and display estimated fee rates every 50ms
//! - Handle graceful shutdown on Ctrl+C
//!

use anyhow::Result;
use bitcoin_indexer::{
    config::IndexerConfig,
    indexer::{Indexer, IndexerApi},
    store::IndexerStore,
};

use bitvmx_bitcoin_rpc::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    types::BlockHeight,
};
use bitvmx_settings::settings;
use std::{rc::Rc, thread::sleep};
use storage_backend::storage::Storage;
use tracing::info;

fn main() -> Result<(), anyhow::Error> {
    let config = settings::load::<IndexerConfig>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::INFO),
        None => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;

    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

    let network = bitcoin_client.get_blockchain_info()?.chain;
    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);
    let storage = Rc::new(Storage::new(&config.storage)?);
    let indexer_store = Rc::new(IndexerStore::new(storage, 6)?);
    let indexer = Indexer::new(bitcoin_client, indexer_store.clone(), config.settings)?;

    info!("Starting indexer loop. Press Ctrl+C to stop...");
    loop {
        indexer.tick()?;
        match indexer.get_estimated_fee_rate() {
            Ok(fee_rate) => info!("Estimated fee rate: {}", fee_rate),
            Err(e) => tracing::warn!("Error getting estimated fee rate: {}", e),
        }
        sleep(std::time::Duration::from_millis(50));
    }

    // This code is unreachable due to the infinite loop above, but we keep it
    // for proper function signature. The program is designed to run until
    // terminated by Ctrl+C or external signal.
    #[allow(unreachable_code)]
    Ok(())
}

pub fn clear_data() {
    let _ = std::fs::remove_dir_all("data");
}
