use log::{error, info, warn};

use crate::{errors::BitcoinClientError, types::BlockHeight};

pub fn define_height_to_sync(
    checkpoint_height: Option<BlockHeight>,
    blockchain_height: BlockHeight,
    indexed_height: Option<BlockHeight>,
) -> Result<BlockHeight, BitcoinClientError> {
    // blockchain_height: The current block height of the Bitcoin network.
    // checkpoint_height: The starting block height for synchronization.
    // indexed_height: The highest block height that has already been synchronized and stored in the storage.

    match indexed_height {
        Some(indexed_height) => {
            info!("Last indexed block is {:?}H", indexed_height);
        }
        None => {
            info!("No block indexed");
        }
    }

    let mut height_to_sync: u32 = indexed_height.unwrap_or(0);

    match checkpoint_height {
        Some(checkpoint) => {

            if checkpoint < height_to_sync {
                warn!("Passed CHECKPOINT_HEIGHT command line is behind last indexed height");
            }

            info!("Using CHECKPOINT_HEIGHT={}H to start to sync", checkpoint);

            height_to_sync = checkpoint;

            if blockchain_height < height_to_sync {
                let error =
                    "The current block height of the Bitcoin network is behind the starting block to sync";
                error!("{}", error);
                return Err(BitcoinClientError::InvalidHeight)
            }
        }
        None => {
            if height_to_sync > 0 {
                height_to_sync += 1;
            }
        }
    }

    Ok(height_to_sync)
}
