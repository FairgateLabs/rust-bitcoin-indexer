use crate::{
    errors::IndexerError,
    store::{IndexerStore, StoreClient},
};
use bitcoin::Txid;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::*};
use mockall::automock;
use tracing::{error, info, warn};
pub struct Indexer<B>
where
    B: BitcoinClientApi,
{
    pub bitcoin_client: B,
    pub store: IndexerStore,
}

#[automock]
pub trait IndexerApi {
    fn tick(&self) -> Result<(), IndexerError>;
    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError>;
    fn get_best_height(&self) -> Result<Option<BlockHeight>, IndexerError>;
    fn get_blockchain_best_height(&self) -> Result<BlockHeight, IndexerError>;
    fn get_height_to_sync(&self) -> Result<BlockHeight, IndexerError>;
    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError>;
}

impl<B> Indexer<B>
where
    B: BitcoinClientApi,
{
    pub fn new(
        bitcoin_client: B,
        store: IndexerStore,
        // checkpoint_height: The starting block height for synchronization.
        checkpoint_height: Option<BlockHeight>,
    ) -> Result<Self, IndexerError> {
        // The highest block height that has already been synchronized and stored in the storage.
        let indexed_height = store.get_best_block()?.map(|block| block.height);

        // The current block height of the Bitcoin network.
        let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

        let mut height_to_sync = 0;

        match indexed_height {
            Some(indexer_height) => {
                info!("Last indexed block is {:?}H", indexer_height);
                // Here we have to validate the indexed_height is correct againt the blockchain_height
                if blockchain_height < indexer_height {
                    error!(
                        "Blockchain height is behind the indexed height at height {}. Blockchain height: {}, Indexed height: {}",
                        indexer_height,
                        blockchain_height,
                        indexer_height
                    );
                    return Err(IndexerError::InconsistentBlockchain);
                } else {
                    // We have to balidate the hash of the indexed_height is correct
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
                            "Indexed block hash mismatch blockchain hash at height {}. Indexed block hash: {:?}, Blockchain block hash: {:?}",
                            indexer_height,
                            indexed_block_hash.unwrap(),
                            blockchain_block_hash
                        );
                        return Err(IndexerError::IndexedBlockHashMismatch);
                    }

                    match checkpoint_height {
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
                                info!("Using CHECKPOINT_HEIGHT({}) to start to sync", checkpoint);
                            }

                            if checkpoint > indexer_height {
                                warn!(
                                    "CHECKPOINT_HEIGHT({}) is ahead of last IndexerHeight({})",
                                    checkpoint, indexer_height
                                );
                                info!("Using IndexerHeight({}) to start to sync", indexer_height);
                            }

                            if checkpoint == indexer_height {
                                info!("Using IndexerHeight({}) to start to sync", indexer_height);
                            }

                            height_to_sync = checkpoint;
                        }
                        None => {
                            height_to_sync = indexer_height;
                        }
                    }
                }
            }
            None => match checkpoint_height {
                Some(checkpoint) => {
                    if blockchain_height < checkpoint {
                        let error =
                                "The current block height of the Bitcoin network is behind the starting block to sync";
                        error!("{}", error);
                        return Err(IndexerError::InconsistentBlockchain);
                    }

                    info!("Start to sync from CHECKPOINT_HEIGHT({})", checkpoint);

                    height_to_sync = checkpoint;
                }
                None => {
                    info!("Start to sync from genesis block");
                    height_to_sync = 0;
                }
            },
        }

        store.save_height_to_sync(height_to_sync)?;

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
    fn get_height_to_sync(&self) -> Result<BlockHeight, IndexerError> {
        let height_to_sync = self.store.get_height_to_sync()?;
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
        let blockchain_height = self.bitcoin_client.get_best_block()?;
        Ok(blockchain_height)
    }

    fn get_best_block(&self) -> Result<Option<FullBlock>, IndexerError> {
        let block = self.store.get_best_block()?;
        Ok(block)
    }

    fn get_tx(&self, tx_id: &Txid) -> Result<Option<TransactionInfo>, IndexerError> {
        let tx_info = self.store.get_tx_info(tx_id)?;

        if let Some(mut tx_info) = tx_info {
            let best_block = self.get_best_block()?;
            if let Some(best_block) = best_block {
                if !tx_info.orphan {
                    tx_info.confirmations = best_block.height - tx_info.block_height + 1;
                }
            }

            return Ok(Some(tx_info));
        }

        Ok(tx_info)
    }

    fn tick(&self) -> Result<(), IndexerError> {
        let mut best_index_height: u32 = self.store.get_height_to_sync()?;
        let best_blockchain_height = self.bitcoin_client.get_best_block()? as BlockHeight;
        let prev_height = best_index_height.saturating_sub(1);

        if best_index_height > best_blockchain_height {
            info!("Blockchain is up to date");
            return Ok(());
        }

        if best_blockchain_height < prev_height {
            warn!(
                "REORG: Blockchain height is behind the height to index. Blockchain height: {}, Height to index: {}",
                best_blockchain_height, best_index_height
            );
            best_index_height = best_blockchain_height;
        }

        let block = self
            .bitcoin_client
            .get_block_by_height(&best_index_height)?;

        let block = match block {
            Some(block) => block,
            None => {
                //Block does not exist in blockchain, then return same height.
                return Ok(());
            }
        };

        let prev_height = best_index_height.saturating_sub(1);
        let prev_block_hash = self.store.get_block_hash_by_height(prev_height)?;

        // Is Genesis block or a checkpoint block
        if best_index_height == 0
            || prev_block_hash.is_none()
            || block.prev_hash == prev_block_hash.unwrap()
        {
            if prev_block_hash.is_none() && best_index_height > 0 {
                warn!("Block height not found. Then could be a checkpoint block",);
                // We don't need to check prev block because does not exist.
            }

            info!(
                "New block at height {} of {} blocks",
                best_index_height, best_blockchain_height
            );

            self.store.save_block(&block)?;
            self.store.save_height_to_sync(best_index_height + 1)?;

            return Ok(());
        }

        // Go back one block to check if the previous block is the same as the previous block hash.
        self.store.save_height_to_sync(best_index_height - 1)?;

        // if current block prev_hash is different than the previous block hash, then we need to reorg.
        warn!(
            "REORG: Block height mismatch. Block at height {} is not matching prev_hash {:?}",
            best_index_height,
            prev_block_hash.unwrap()
        );

        Ok(())
    }
}
