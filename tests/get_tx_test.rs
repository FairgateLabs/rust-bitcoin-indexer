use anyhow::Result;
use bitcoin::{address::NetworkChecked, Address, Amount, Network};
use bitcoin_indexer::{
    config::IndexerConfig,
    indexer::{Indexer, IndexerApi},
    types::TransactionStatus,
};
use bitcoincore_rpc::RpcApi;
use bitcoind::{bitcoind::Bitcoind, config::BitcoindConfig};
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitvmx_settings::settings;
use tracing::info;
mod utils;
use crate::utils::{clear_output, get_indexer_store, get_random_pubkey};

#[test]
fn test_get_transaction_lifecycle() -> Result<(), anyhow::Error> {
    clear_output();

    let config = settings::load::<IndexerConfig>()?;

    let log_level = match &config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::ERROR),
        None => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let bitcoind_config = BitcoindConfig::default();

    let bitcoind = Bitcoind::new(bitcoind_config, config.bitcoin.clone(), None);

    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let indexer_store = get_indexer_store();
    let bitcoin_client_for_indexer = BitcoinClient::new_from_config(&config.bitcoin)?;
    let indexer = Indexer::new(bitcoin_client_for_indexer, indexer_store.clone(), None)?;
    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;

    // Step 1: Mine some blocks to have funds
    info!("Mining 110 blocks to wallet");
    bitcoin_client.mine_blocks_to_address(110, &wallet)?;

    // Step 2: Index the blocks
    info!("Indexing 110 blocks");
    for _ in 0..110 {
        indexer.tick()?;
    }

    info!("Checking that the indexer is at height 110");
    assert_eq!(indexer.get_best_height()?, Some(110));
    assert_eq!(bitcoin_client.get_best_block()?, 110);

    // Step 3: Create a transaction and send it to mempool
    info!("Creating a transaction to send to mempool");
    let recipient_pubkey = get_random_pubkey();
    let address = bitcoin_client.get_new_address(recipient_pubkey, Network::Regtest)?;
    let recipient_address: &Address<NetworkChecked> = &address;

    // Use RPC client to send a transaction
    let txid = bitcoin_client.client.send_to_address(
        recipient_address,
        Amount::from_sat(10000),
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    info!("Transaction {} sent to mempool", txid);

    // Step 4: Check that the transaction is in mempool
    info!("Checking that get_transaction returns InMempool status");
    let tx_info = indexer.get_transaction(&txid)?;
    assert_eq!(tx_info.status, TransactionStatus::InMempool);
    assert_eq!(tx_info.confirmations, 0);
    assert!(tx_info.block_info.is_none());
    assert!(tx_info.tx.is_none()); // Transaction not in storage yet, only in mempool

    // Step 5: Mine a block to confirm the transaction
    info!("Mining a block to confirm the transaction");
    bitcoin_client.mine_blocks_to_address(1, &wallet)?;

    // Step 6: Index the new block
    info!("Indexing the new block");
    indexer.tick()?;

    // Step 7: Check that the transaction is confirmed
    info!("Checking that get_transaction returns Confirmed status");
    let tx_info = indexer.get_transaction(&txid)?;
    assert_eq!(tx_info.status, TransactionStatus::Confirmed);
    assert_eq!(tx_info.confirmations, 1);
    assert!(tx_info.block_info.is_some());
    assert!(tx_info.tx.is_some());
    assert!(!tx_info.block_info.as_ref().unwrap().orphan);

    // Step 8: Revert the chain (reorg) by invalidating the block
    info!("Invalidating the block to cause a reorg and mining a new block to confirm the reorg");
    let block_height = bitcoin_client.get_best_block()?;
    let block_to_invalidate = bitcoin_client.get_block_by_height(&block_height)?.unwrap();
    bitcoin_client.invalidate_block(&block_to_invalidate.hash)?;
    bitcoin_client.mine_blocks_to_address(1, &address)?;

    // Step 9: Index to detect the reorg
    info!("Indexing to detect the reorg");
    indexer.tick()?;
    indexer.tick()?;

    let block_height_indexer_after = indexer.get_best_height()?.unwrap();
    let block_height_after = bitcoin_client.get_best_block()?;
    assert_eq!(block_height_indexer_after, block_height_after);

    // Step 10: Check that the transaction is now orphan
    info!("Checking that get_transaction returns Confirmed status with 1 confirmation");
    let tx_info = indexer.get_transaction(&txid)?;
    assert_eq!(tx_info.status, TransactionStatus::Confirmed);
    assert_eq!(tx_info.confirmations, 1);
    assert!(!tx_info.is_orphan());

    clear_output();

    Ok(())
}
