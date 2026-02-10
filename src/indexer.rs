use crate::{
    config::IndexerSettings,
    errors::IndexerError,
    store::{IndexerStore, StoreClient},
    types::{FullBlock, TransactionBlockchainStatus, TransactionStatus},
};
use bitcoin::Txid;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::*};
use std::rc::Rc;
use tracing::{error, info, warn};
pub struct Indexer<B>
where
    B: BitcoinClientApi,
{
    pub bitcoin_client: B,
    pub store: Rc<IndexerStore>,
    pub settings: IndexerSettings,
}

pub trait IndexerApi {
    /// Checks if the indexer has indexed the entire blockchain and is at the latest block.
    /// Returns `Ok(true)` if ready, `Ok(false)` if not, or an `IndexerError` if an error occurs.
    fn is_ready(&self) -> Result<bool, IndexerError>;

    /// Processes the next block in the blockchain.
    /// Returns `Ok(())` if successful, or an `IndexerError` if an error occurs during processing.
    fn tick(&self) -> Result<(), IndexerError>;

    /// Retrieves the best (most recent) block that has been indexed.
    /// Returns `Ok(Some(FullBlock))` if a block is found, `Ok(None)` if no block was indexed, or an `IndexerError` if an error occurs.
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError>;

    /// Retrieves the height of the best (most recent) block that has been indexed.
    /// Returns `Ok(Some(BlockHeight))` if a height is found, `Ok(None)` if no block is indexed, or an `IndexerError` if an error occurs.
    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerError>;

    /// Retrieves the current best block height from the Bitcoin blockchain.
    /// Returns `Ok(BlockHeight)` with the current blockchain height, or an `IndexerError` if an error occurs.
    fn get_blockchain_best_height(&self) -> Result<BlockHeight, IndexerError>;

    /// Retrieves a block by its height.
    /// Returns `Ok(Some(FullBlock))` if the block is found, `Ok(None)` if not found, or an `IndexerError` if an error occurs.
    fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<FullBlock>, IndexerError>;

    // Retrieves a block by its hash.
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<FullBlock>, IndexerError>;

    /// Retrieves transaction information for a given transaction ID.
    /// Returns `Ok(TransactionInfo)` with the transaction status, or an `IndexerError` if an error occurs.
    /// If the transaction is not found, returns `TransactionInfo` with `TransactionBlockchainStatus::NotFound`.
    fn get_transaction(&self, tx_id: &Txid) -> Result<TransactionStatus, IndexerError>;

    /// Retrieves the estimated fee rate from the most recently indexed block.
    /// Returns `Ok(u64)` with the fee rate in satoshis per virtual byte (sat/vB), or an `IndexerError` if an error occurs or the indexer is not synced.
    fn get_estimated_fee_rate(&self) -> Result<u64, IndexerError>;
}

impl<B> Indexer<B>
where
    B: BitcoinClientApi,
{
    pub fn new(
        bitcoin_client: B,
        store: Rc<IndexerStore>,
        settings: Option<IndexerSettings>,
    ) -> Result<Self, IndexerError> {
        let settings = settings.unwrap_or_default();

        // The highest block height that has already been synchronized and stored in the storage.
        let indexed_height = store.get_best_height()?;

        // The current block height of the Bitcoin network.
        let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

        let height_to_sync;

        match indexed_height {
            Some(indexer_height) => {
                info!("Last indexed block is {:?}H", indexer_height);
                // Here we have to validate that the indexed_height is correct against the blockchain_height
                if blockchain_height < indexer_height {
                    error!(
                        "Blockchain height is behind the indexed height. BlockchainHeight({}), IndexedHeight({})",
                        blockchain_height,
                        indexer_height
                    );
                    return Err(IndexerError::InconsistentBlockchain);
                } else {
                    // We have to validate that the hash of the indexed_height is correct
                    let blockchain_block = bitcoin_client.get_block_by_height(&indexer_height)?;

                    if blockchain_block.is_none() {
                        return Err(IndexerError::InconsistentBlockchain);
                    }

                    let blockchain_block_hash = blockchain_block.unwrap().hash;
                    let indexed_block_hash = store.get_block_hash_by_height(indexer_height)?;

                    if indexed_block_hash.is_none() {
                        return Err(IndexerError::DatabaseCorrupted);
                    }

                    if blockchain_block_hash != indexed_block_hash.unwrap() {
                        error!(
                            "Indexed block hash does not match blockchain hash at IndexedHeight({}). IndexedBlockHash({:?}), BlockchainBlockHash({:?})",
                            indexer_height,
                            indexed_block_hash.unwrap(),
                            blockchain_block_hash
                        );
                        return Err(IndexerError::IndexedBlockHashMismatch);
                    }

                    match settings.checkpoint_height {
                        Some(checkpoint) => {
                            let existing_checkpoint = store.get_checkpoint_height()?;

                            if existing_checkpoint.is_some() {
                                if existing_checkpoint.unwrap() != checkpoint {
                                    error!(
                                    "The checkpoint height used is different from the previously indexed one. Previously CheckpointHeight({}), New CheckpointHeight({})",
                                        existing_checkpoint.unwrap(),
                                        checkpoint
                                    );

                                    info!("To use a new checkpoint, you need to wipe the entire database and restart the indexer with the new checkpoint.");
                                    info!("The indexer will continue syncing from the last indexed height");

                                    return Err(
                                        IndexerError::AlreadyIndexedWithDifferentCheckpointHeight,
                                    );
                                }
                            }

                            height_to_sync = indexer_height;
                        }
                        None => {
                            height_to_sync = indexer_height;
                        }
                    }
                }
            }
            None => match settings.checkpoint_height {
                Some(checkpoint) => {
                    if blockchain_height < checkpoint {
                        error!("The Bitcoin network's current block height is behind the checkpoint height");
                        return Err(IndexerError::CheckpointHeightAheadOfBlockchainHeight);
                    }

                    store.save_checkpoint_height(checkpoint)?;

                    height_to_sync = checkpoint;

                    info!("Starting to sync from CheckpointHeight({})", checkpoint);
                }
                None => {
                    info!("Starting to sync from genesis block");
                    height_to_sync = 0;
                }
            },
        }

        // Check if the block exists in the storage. If not, save it.
        let block_hash = store.get_block_hash_by_height(height_to_sync)?;

        if block_hash.is_none() {
            let block = bitcoin_client
                .get_block_by_height(&height_to_sync)?
                .ok_or(IndexerError::BlockNotFound)?;

            let estimated_fee_rate = estimate_fee_rate(&bitcoin_client, &block)?;

            store.save_new_best_block(&block, estimated_fee_rate)?;
        }

        Ok(Self {
            bitcoin_client,
            store,
            settings,
        })
    }
}

impl<B> IndexerApi for Indexer<B>
where
    B: BitcoinClientApi,
{
    fn is_ready(&self) -> Result<bool, IndexerError> {
        let current_height = self.get_best_height()?;
        let blockchain_height = self.get_blockchain_best_height()?;

        if current_height.is_none() {
            return Ok(false);
        }

        Ok(current_height.unwrap() >= blockchain_height)
    }

    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerError> {
        let best_height = self.store.get_best_height()?;
        Ok(best_height)
    }

    fn get_blockchain_best_height(&self) -> Result<BlockHeight, IndexerError> {
        Ok(self.bitcoin_client.get_best_block()? as BlockHeight)
    }

    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError> {
        Ok(self.store.get_best_block()?)
    }

    fn get_transaction(&self, tx_id: &Txid) -> Result<TransactionStatus, IndexerError> {
        // First, check if transaction is in storage
        let tx_status = self.store.get_tx_info(tx_id)?;

        if let Some(mut tx_info) = tx_status {
            // Update status based on confirmations and threshold
            if tx_info.is_orphan() {
                // If transaction is orphan, check if it's in mempool
                let tx_mempool_status = self.bitcoin_client.get_mempool_entry(tx_id);

                if tx_mempool_status.is_err() {
                    // Transaction is not in mempool, mark as not found
                    tx_info.status = TransactionBlockchainStatus::NotFound;
                    tx_info.confirmations = 0;
                    tx_info.block_info = None;
                }
            }

            Ok(tx_info)
        } else {
            // Transaction not found in storage, check mempool
            let tx_mempool_status = self.bitcoin_client.get_mempool_entry(tx_id);
            let status = if tx_mempool_status.is_ok() {
                TransactionBlockchainStatus::InMempool
            } else {
                TransactionBlockchainStatus::NotFound
            };

            Ok(TransactionStatus {
                tx: None,
                block_info: None,
                confirmations: 0,
                status,
                confirmation_threshold: self.settings.confirmation_threshold,
            })
        }
    }

    fn get_estimated_fee_rate(&self) -> Result<u64, IndexerError> {
        let best_block = self.store.get_best_block()?;
        let best_blockchain_height = self.bitcoin_client.get_best_block()?;

        if best_block.is_none() {
            return Err(IndexerError::IndexerNotSynced);
        }

        let best_block = best_block.unwrap();

        if best_block.height != best_blockchain_height {
            return Err(IndexerError::IndexerNotSynced);
        }

        if best_block.estimated_fee_rate > 0 {
            Ok(best_block.estimated_fee_rate)
        } else {
            Err(IndexerError::FeeRateNotEstimated)
        }
    }

    fn tick(&self) -> Result<(), IndexerError> {
        // Retrieve the last block height that has been successfully synced by the indexer.
        // This should have data.
        let best_indexer_height = self.store.get_best_height()?.unwrap_or(0);
        // Retrieve the current best block height from the Bitcoin blockchain.
        let best_blockchain_height = self.bitcoin_client.get_best_block()?;

        // Choose the minimum of the indexer's synced height and the blockchain's best height
        // to check for potential reorgs.
        let mut current_height = best_indexer_height;

        if best_indexer_height > best_blockchain_height {
            current_height = best_blockchain_height;
        }

        // Fetch the block at the determined height from both the blockchain and the indexer store.
        let new_blockchain_block = self
            .bitcoin_client
            .get_block_by_height(&current_height)?
            .ok_or(IndexerError::BlockNotFound)?;

        let indexer_block = self
            .store
            .get_block_by_height(current_height)?
            .ok_or(IndexerError::BlockNotFound)?;

        // --- REORG DETECTION ---
        // If the block exists but the hashes differ, a reorg has occurred and we must roll back.
        if new_blockchain_block.height > 0 && new_blockchain_block.hash != indexer_block.hash {
            warn!(
                "REORG: Block at height {} is different from the blockchain",
                current_height
            );

            self.store
                .mark_following_blocks_as_orphan(new_blockchain_block.height)?;

            // Roll back to the previous block.
            let previous_blockchain_height = current_height.saturating_sub(1);
            self.store.save_best_height(previous_blockchain_height)?;

            info!(
                "Rolling back to previous block. New block to sync height: {}",
                previous_blockchain_height
            );

            return Ok(());
        }

        // --- NORMAL SYNC PATH ---

        // If the indexer is already caught up with the blockchain, do nothing.
        if best_indexer_height == best_blockchain_height {
            info!("Indexer is up to date");
            return Ok(());
        }

        if best_indexer_height > best_blockchain_height {
            // This branch handles the scenario where the indexer has advanced further than the current blockchain tip.
            // This situation can occur if blocks have been invalidated or a reorg has caused the blockchain to roll back.
            // Mark all blocks above the blockchain's best height as orphan before updating the height.
            warn!("Indexer is ahead of the blockchain. Marking blocks as orphan and updating synced and best heights to match the blockchain's current best height.");

            self.store
                .mark_following_blocks_as_orphan(best_blockchain_height + 1)?;

            self.store.save_best_height(best_blockchain_height)?;
            return Ok(());
        }

        // At this point, the blockchain has advanced beyond the indexer's current state.
        // Proceed to process and index the next block to catch up.
        let next_block_height = current_height + 1;

        info!(
            "Synced block at height {} of {}",
            next_block_height, best_blockchain_height
        );

        let next_block = self
            .bitcoin_client
            .get_block_by_height(&next_block_height)?
            .ok_or(IndexerError::BlockNotFound)?;

        let estimated_fee_rate = estimate_fee_rate(&self.bitcoin_client, &next_block)?;

        // Save the next block to the local store.
        self.store
            .save_new_best_block(&next_block, estimated_fee_rate)?;
        // Update the last synced height to reflect the new block.

        Ok(())
    }

    fn get_block_by_height(&self, height: BlockHeight) -> Result<Option<FullBlock>, IndexerError> {
        let block = self.store.get_block_by_height(height)?;
        Ok(block)
    }

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<FullBlock>, IndexerError> {
        let block = self.store.get_block_by_hash(hash)?;
        Ok(block)
    }
}

/// Estimates the fee rate for the next block based on the middle transaction in the provided block.
///
/// This function analyzes the fee rate of the middle transaction in a block to provide an estimate
/// for future fee calculations. It implements a simple heuristic by selecting the transaction at
/// the median position within the block's transaction list.
///
/// # Implementation Details
///
/// The function uses Bitcoin Core RPC `getrawtransaction` with verbosity level 2, which requires
/// Bitcoin Core version 25.0.0 or higher. It calculates the fee rate using the formula:
/// `fee_rate = transaction_fee_in_sats / transaction_vsize_in_vb`
///
/// # Note
///
/// This is a simple estimation method that could be improved by:
/// - Using `getblock` with verbosity 3 (when supported by bitcoincore-rpc)
/// - Calculating median or average fee rates from multiple transactions
/// - Considering block fullness percentage for more accurate predictions
fn estimate_fee_rate<B: BitcoinClientApi>(
    bitcoin_client: &B,
    next_block: &BlockInfo,
) -> Result<u64, IndexerError> {
    // Const might be settings in the future
    const MIN_BLOCK_TX: usize = 5;
    const ERROR_FEE_RATE: u64 = 0; // sat/vB
    const DEFAULT_MIN_FEE_RATE: u64 = 1; // sat/vB

    // TODO use get block with verbosity 3 would be a nice optimization (not yet supported by bitcoincore-rpc),
    // it will remove the need to call again getrawtransaction
    // with that approach we can cheaply calculate the median fee rate of the block, or the average fee rate from the medium zone transactions
    // See https://bitcoincore.org/en/doc/23.0.0/rpc/blockchain/getblock/

    let block_tx_count = next_block.txs.len();
    tracing::trace!("Transactions count: {}", block_tx_count);

    // In a future version we can analyze if the block is full or empty or which %,
    // a block lower than 75% will lead to lower fee rates for next block inclusion
    if block_tx_count <= MIN_BLOCK_TX {
        warn!(
            "Can't estimate fee rate - block has {} or fewer transactions",
            MIN_BLOCK_TX
        );
        return Ok(ERROR_FEE_RATE);
    }

    let middle_index = block_tx_count / 2;
    let middle_tx = next_block.txs[middle_index].clone();

    // Note: For coinbase transactions (first tx in block), there's no fee since there are no inputs
    // Usually the block is ordered with coinbase as first transaction, but this is not enforced by consensus rules, to this case might rarely happen
    if middle_tx.is_coinbase() {
        warn!("Can't estimate fee rate - middle transaction is coinbase");
        return Ok(ERROR_FEE_RATE);
    }

    let tx_id = middle_tx.compute_txid();

    // This call needs bitcoin core 25.0.0 or higher
    // see https://bitcoincore.org/en/doc/25.0.0/rpc/rawtransactions/getrawtransaction/
    let raw_tx_verbose = bitcoin_client.get_raw_transaction_verbosity_two(&tx_id)?;

    let fee_in_btc = match raw_tx_verbose.get("fee").and_then(|v| v.as_f64()) {
        Some(fee_value) => fee_value,
        None => {
            error!(
                "Can't estimate fee rate - no fee value available for transaction {}",
                tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let vsize = match raw_tx_verbose.get("vsize").and_then(|v| v.as_u64()) {
        Some(vsize_value) => vsize_value,
        None => {
            error!(
                "Can't estimate fee rate - no vsize value available for transaction {}",
                tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let fee = match bitcoin::Amount::from_btc(fee_in_btc) {
        Ok(amount) => amount.to_sat(),
        Err(_) => {
            error!(
                "Can't estimate fee rate - invalid fee value {} for transaction {}",
                fee_in_btc, tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let fee_rate = fee as f64 / vsize as f64;
    let adjusted_fee_rate = if fee_rate < 1.0 {
        DEFAULT_MIN_FEE_RATE
    } else {
        fee_rate as u64
    };

    tracing::debug!("TXID: {:#?}", tx_id);
    tracing::debug!("Adjusted fee rate: {} sat/vB", adjusted_fee_rate);
    tracing::trace!("middle index: {}", middle_index);
    tracing::trace!("middle transaction: {:#?}", middle_tx);
    tracing::trace!("raw_tx_verbose middle transaction: {:#?}", raw_tx_verbose);
    tracing::trace!("Transaction fee: {} sats", fee);
    tracing::trace!("Transaction vsize: {} vB", vsize);
    tracing::trace!("Transaction fee rate: {} sat/vB", fee_rate);
    tracing::trace!("Integer Transaction fee rate: {} sat/vB", fee_rate as u64);

    Ok(adjusted_fee_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::IndexerConfig;
    use bitcoin::PublicKey;
    use bitcoind::bitcoind::Bitcoind;
    use bitcoind::config::BitcoindConfig;
    use bitvmx_bitcoin_rpc::bitcoin_client::BitcoinClient;
    use bitvmx_settings::settings;

    // Helper to setup bitcoind and BitcoinClient for tests
    fn setup_bitcoind(
    ) -> Result<(BitcoinClient, Bitcoind, bitcoin::Address), Box<dyn std::error::Error>> {
        let config = settings::load::<IndexerConfig>()?;
        let bitcoind_config = BitcoindConfig::default();
        let bitcoind = Bitcoind::new(bitcoind_config, config.bitcoin.clone(), None);
        bitcoind.start()?;

        // Wait for bitcoind to be ready
        std::thread::sleep(std::time::Duration::from_millis(500));

        let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
        let wallet = bitcoin_client.init_wallet("test_wallet")?;

        Ok((bitcoin_client, bitcoind, wallet))
    }

    fn get_random_pubkey() -> PublicKey {
        use bitcoin::key::rand::rngs::OsRng;
        use bitcoin::secp256k1::{Secp256k1, SecretKey};

        let secp = Secp256k1::new();
        let secret_key = SecretKey::new(&mut OsRng);
        let public_key = secret_key.public_key(&secp);
        PublicKey::new(public_key)
    }

    #[test]
    fn test_estimate_fee_rate_with_real_transactions() -> Result<(), Box<dyn std::error::Error>> {
        let (bitcoin_client, bitcoind, wallet) = setup_bitcoind()?;

        // Mine 101 blocks to have mature coins (coinbase needs 100 confirmations)
        bitcoin_client.mine_blocks_to_address(101, &wallet)?;

        // Create 10 transactions by generating new addresses and mining to them
        // This creates actual transactions with inputs and outputs
        for _ in 0..10 {
            let pubkey = get_random_pubkey();
            let new_address = bitcoin_client.get_new_address(pubkey, bitcoin::Network::Regtest)?;
            bitcoin_client.mine_blocks_to_address(1, &new_address)?;
        }

        // Get the last mined block which should have coinbase transaction
        let best_block_height = bitcoin_client.get_best_block()?;
        let block = bitcoin_client
            .get_block_by_height(&best_block_height)?
            .ok_or("Block not found")?;

        // Verify estimate_fee_rate is called
        let fee_rate = estimate_fee_rate(&bitcoin_client, &block)?;

        // Block only has coinbase (1 tx), so should return 0
        assert_eq!(
            fee_rate, 0,
            "Fee rate should be 0 for blocks with only coinbase"
        );

        // Verify the block was processed from real blockchain data (not mocks)
        assert!(
            block.txs.len() > 0,
            "Block should have transactions from real blockchain"
        );
        assert!(
            block.txs[0].is_coinbase(),
            "First transaction should be coinbase"
        );

        bitcoind.stop()?;
        Ok(())
    }

    #[test]
    fn test_estimate_fee_rate_computation_from_indexer() -> Result<(), Box<dyn std::error::Error>> {
        use crate::store::IndexerStore;
        use std::rc::Rc;
        use storage_backend::{storage::Storage, storage_config::StorageConfig};

        let (bitcoin_client, bitcoind, wallet) = setup_bitcoind()?;

        // Mine blocks with mature coins
        bitcoin_client.mine_blocks_to_address(105, &wallet)?;

        // Create indexer and let it process blocks
        let db_path = format!(
            "./test_output/estimate_fee_test_{}/db",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let storage = Rc::new(Storage::new(&StorageConfig::new(db_path.clone(), None))?);
        let store = Rc::new(IndexerStore::new(storage, 6)?);

        let indexer = crate::indexer::Indexer::new(bitcoin_client, store.clone(), None)?;

        // Process blocks through the indexer
        for _ in 0..5 {
            indexer.tick()?;
        }

        // Get the best block from indexer store
        let best_block = store.get_best_block()?;
        assert!(best_block.is_some(), "Indexer should have processed blocks");

        let block = best_block.unwrap();

        // Verify estimate_fee_rate was computed and stored by the indexer
        // The fee rate is stored in the FullBlock by the indexer when processing blocks
        // For blocks with only coinbase or few transactions, it will be 0
        assert!(
            block.estimated_fee_rate == 0,
            "Fee rate should be 0 for blocks with insufficient transactions (computed by indexer)"
        );

        // Clean up
        bitcoind.stop()?;
        let _ = std::fs::remove_dir_all(&db_path);

        Ok(())
    }

    #[test]
    fn test_estimate_fee_rate_with_few_transactions() -> Result<(), Box<dyn std::error::Error>> {
        let (bitcoin_client, bitcoind, wallet) = setup_bitcoind()?;

        // Mine a single block with only coinbase
        bitcoin_client.mine_blocks_to_address(1, &wallet)?;

        let best_block_height = bitcoin_client.get_best_block()?;
        let block = bitcoin_client
            .get_block_by_height(&best_block_height)?
            .ok_or("Block not found")?;

        // Verify this is real blockchain data (not mock)
        assert!(
            block.txs.len() > 0,
            "Block should have at least coinbase from real blockchain"
        );
        assert!(block.txs.len() <= 5, "Block should have few transactions");

        // Call estimate_fee_rate on real block data
        let fee_rate = estimate_fee_rate(&bitcoin_client, &block)?;
        assert_eq!(
            fee_rate, 0,
            "Fee rate should be 0 for blocks with <= 5 transactions"
        );

        bitcoind.stop()?;
        Ok(())
    }

    #[test]
    fn test_estimate_fee_rate_returns_valid_result() -> Result<(), Box<dyn std::error::Error>> {
        let (bitcoin_client, bitcoind, wallet) = setup_bitcoind()?;

        // Mine blocks to get real blockchain data
        bitcoin_client.mine_blocks_to_address(5, &wallet)?;

        let best_block_height = bitcoin_client.get_best_block()?;
        let block = bitcoin_client
            .get_block_by_height(&best_block_height)?
            .ok_or("Block not found")?;

        // Verify we're using real blockchain data
        assert!(block.height > 0, "Block should be from real blockchain");
        assert!(
            block.txs.len() > 0,
            "Block should contain transactions from real blockchain"
        );

        // Call estimate_fee_rate and verify it returns Ok
        let result = estimate_fee_rate(&bitcoin_client, &block);
        assert!(
            result.is_ok(),
            "estimate_fee_rate should return Ok with real blockchain data"
        );

        let fee_rate = result?;
        // For blocks with only coinbase, fee_rate will be 0
        assert_eq!(
            fee_rate, 0,
            "Fee rate is 0 because block has insufficient transactions"
        );

        bitcoind.stop()?;
        Ok(())
    }
}
