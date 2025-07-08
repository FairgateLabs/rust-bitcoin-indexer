use crate::{
    config::IndexerSettings,
    errors::IndexerError,
    store::{IndexerStore, StoreClient},
    types::{FullBlock, TransactionInfo},
};
use bitcoin::Txid;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::*};
use mockall::automock;
use std::rc::Rc;
use tracing::{error, info, warn};
pub struct Indexer<B>
where
    B: BitcoinClientApi,
{
    pub bitcoin_client: B,
    pub store: Rc<IndexerStore>,
}

#[automock]
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

    /// Determines the next block height that needs to be synchronized.
    /// Returns `Ok(BlockHeight)` with the height to sync, or an `IndexerError` if an error occurs.
    fn get_height_to_sync(&self) -> Result<BlockHeight, IndexerError>;

    /// Retrieves transaction information for a given transaction ID.
    /// Returns `Ok(Some(TransactionInfo))` if the transaction is found, `Ok(None)` if not found, or an `IndexerError` if an error occurs.
    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError>;
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
        let indexed_height = store.get_best_block()?.map(|block| block.height);

        // The current block height of the Bitcoin network.
        let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

        let mut height_to_sync = 0;

        match indexed_height {
            Some(indexer_height) => {
                info!("Last indexed block is {:?}H", indexer_height);
                // Here we have to validate that the indexed_height is correct against the blockchain_height
                if blockchain_height < indexer_height {
                    error!(
                        "Blockchain height is behind the indexed height at height {}. Blockchain height: {}, Indexed height: {}",
                        indexer_height,
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
                            "Indexed block hash does not match blockchain hash at height {}. Indexed block hash: {:?}, Blockchain block hash: {:?}",
                            indexer_height,
                            indexed_block_hash.unwrap(),
                            blockchain_block_hash
                        );
                        return Err(IndexerError::IndexedBlockHashMismatch);
                    }

                    match settings.checkpoint_height {
                        Some(checkpoint) => {
                            if checkpoint > blockchain_height {
                                error!(
                                    "CHECKPOINT_HEIGHT({}) is ahead of blockchain height ({})",
                                    checkpoint, blockchain_height
                                );
                                return Err(IndexerError::CheckpointHeightAheadOfBlockchainHeight);
                            }

                            if checkpoint < indexer_height {
                                warn!(
                                    "CHECKPOINT_HEIGHT({}) is behind last IndexerHeight({})",
                                    checkpoint, height_to_sync
                                );
                                info!("Using CHECKPOINT_HEIGHT({}) to start syncing", checkpoint);
                            }

                            if checkpoint > indexer_height {
                                warn!(
                                    "CHECKPOINT_HEIGHT({}) is ahead of last IndexerHeight({})",
                                    checkpoint, indexer_height
                                );
                                info!("Using IndexerHeight({}) to start syncing", indexer_height);
                            }

                            if checkpoint == indexer_height {
                                info!("Using IndexerHeight({}) to start syncing", indexer_height);
                            }

                            height_to_sync = checkpoint;
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
                        let error =
                                "The current block height of the Bitcoin network is behind the starting block to sync";
                        error!("{}", error);
                        return Err(IndexerError::InconsistentBlockchain);
                    }

                    info!("Starting to sync from CHECKPOINT_HEIGHT({})", checkpoint);

                    height_to_sync = checkpoint;
                }
                None => {
                    info!("Starting to sync from genesis block");
                    height_to_sync = 0;
                }
            },
        }

        store.save_last_synced_height(height_to_sync)?;

        // Check if the block exists in the storage. If not, save it.
        let block_hash = store.get_block_hash_by_height(height_to_sync)?;

        if block_hash.is_none() {
            let block = bitcoin_client
                .get_block_by_height(&height_to_sync)?
                .ok_or(IndexerError::BlockNotFound)?;

            store.save_new_best_block(&block)?;
        }

        Ok(Self {
            bitcoin_client,
            store,
        })
    }
}

#[automock]
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

    fn get_height_to_sync(&self) -> Result<BlockHeight, IndexerError> {
        let height_to_sync = self.store.get_last_synced_height()?;
        Ok(height_to_sync)
    }

    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerError> {
        let best_block = self.store.get_best_block()?;
        if let Some(best_block) = best_block {
            Ok(Some(best_block.height))
        } else {
            Ok(None)
        }
    }

    fn get_blockchain_best_height(&self) -> Result<BlockHeight, IndexerError> {
        Ok(self.bitcoin_client.get_best_block()? as BlockHeight)
    }

    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError> {
        Ok(self.store.get_best_block()?)
    }

    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError> {
        Ok(self.store.get_tx_info(tx_id)?)
    }

    fn tick(&self) -> Result<(), IndexerError> {
        // Retrieve the last block height that has been successfully synced by the indexer.
        let best_indexer_height: u32 = self.store.get_last_synced_height()?;
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

            // Roll back to the previous block.
            let previous_blockchain_height = current_height.saturating_sub(1);
            self.store
                .save_last_synced_height(previous_blockchain_height)?;

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

        if best_indexer_height >= best_blockchain_height {
            // This branch handles the scenario where the indexer has advanced further than the current blockchain tip.
            // This situation can occur if blocks have been invalidated or a reorg has caused the blockchain to roll back.
            // To resolve this, update the indexer's synced and best heights to match the blockchain's current best height.
            warn!("Indexer is ahead of the blockchain. Updating synced and best heights to match the blockchain's current best height.");
            self.store.save_last_synced_height(best_blockchain_height)?;
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

        // Save the next block to the local store.
        self.store.save_new_best_block(&next_block)?;
        // Update the last synced height to reflect the new block.
        self.store.save_last_synced_height(next_block_height)?;

        Ok(())
    }
}
