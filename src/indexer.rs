use anyhow::Result;
use bitcoincore_rpc::{Client, RpcApi};
use log::{error, info, warn};

use crate::{
    bitcoin_client::{BitcoinClient, RpcClient},
    store::Store,
    types::{BlockHeight, Config},
};

pub struct Indexer {
    checkpoint_height: Option<BlockHeight>,
    blockchain_height: BlockHeight,
    bitcoin_client: BitcoinClient,
    store: Box<Store>,
}

impl Indexer {
    pub fn new(config: Config) -> Result<Self> {
        let bitcoin_client = BitcoinClient::new(&config.node_rpc_url.unwrap())?;
        let blockchain_height: u32 = bitcoin_client.client.get_best_block()? as BlockHeight;
        let network = &bitcoin_client.client.get_blockchain_info()?.chain;

        let db = Store::new(&config.db_file_path.unwrap())?;
        info!("Connected to chain {}", network.to_string().to_uppercase());
        info!("Chain best block at {}H", blockchain_height);

        Ok(Self {
            checkpoint_height: config.checkpoint_height,
            bitcoin_client,
            blockchain_height,
            store: Box::new(db),
        })
    }

    // pub fn get_height_to_sync(&mut self) -> (u32, bool) {
    //     // node_starting_chainhead_height: The current block height of the Bitcoin network.
    //     // height_to_sync: The starting block height for synchronization.
    //     // last_indexed_height: The highest block height that has already been synchronized and stored in the database.

    //     let last_indexed_height = self.store.get_last_indexed_height().unwrap();

    //     if last_indexed_height.is_some() {
    //         info!("Last indexed block is {:?}H", last_indexed_height.unwrap());
    //     } else {
    //         info!("No block indexed");
    //     }

    //     let last_indexed_height = last_indexed_height.unwrap_or(0);

    //     let start_to_sync_from_height = match self.checkpoint_height {
    //         Some(height_to_sync) => {
    //             if height_to_sync < last_indexed_height {
    //                 warn!("Passed HEIGHT_TO_SYNC command line is behind last indexed height");
    //                 info!(
    //                     "Using last indexed height {} instead HEIGHT_TO_SYNC {} to start to sync",
    //                     last_indexed_height, height_to_sync
    //                 );
    //                 (last_indexed_height, false)
    //             } else {
    //                 info!("Using HEIGHT_TO_SYNC={} to start to sync", height_to_sync);
    //                 (height_to_sync, true)
    //             }
    //         }
    //         None => (last_indexed_height, false),
    //     };

    //     // 3) ERROR: node_starting_chainhead_height < start_height
    //     if self.blockchain_height < start_to_sync_from_height.0 {
    //         error!(
    //             "The current block height of the Bitcoin network is behind the starting block to sync"
    //         );
    //         panic!();
    //     }

    //     start_to_sync_from_height
    // }

    pub fn run(&mut self) -> Result<()> {
        // let start_to_sync = self.get_height_to_sync();

        Ok(())
    }
}
