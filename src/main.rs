use anyhow::Result;
use bitcoin::{
    key::rand::rngs::OsRng,
    secp256k1::{self, PublicKey as SecpPublicKey, SecretKey},
    Network, PublicKey,
};
use bitcoin_indexer::{
    config::ConfigIndexer,
    indexer::{Indexer, IndexerApi},
    store::IndexerStore,
};
use bitcoind::bitcoind::Bitcoind;
use bitvmx_bitcoin_rpc::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    types::BlockHeight,
};
use bitvmx_settings::settings;
use std::{rc::Rc, sync::mpsc::channel, thread, time::Duration};
use storage_backend::storage::Storage;
use tracing::info;

fn main() -> Result<(), anyhow::Error> {
    let (tx, rx) = channel();

    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))
        .expect("Error setting Ctrl-C handler");

    let config = settings::load::<ConfigIndexer>()?;

    let log_level = match config.log_level {
        Some(level) => level.parse().unwrap_or(tracing::Level::INFO),
        None => tracing::Level::INFO,
    };

    tracing_subscriber::fmt().with_max_level(log_level).init();

    let bitcoind = Bitcoind::new(
        "bitcoin-regtest",
        "ruimarinho/bitcoin-core",
        config.bitcoin.clone(),
    );

    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet(Network::Regtest, "test_wallet")?;

    info!("Mining 100 blocks to wallet");
    bitcoin_client.mine_blocks_to_address(100, &wallet)?;
    let blockchain_height = bitcoin_client.get_best_block()? as BlockHeight;

    let network = bitcoin_client.get_blockchain_info()?.chain;
    info!("Connected to chain {}", network);
    info!("Chain best block at {}H", blockchain_height);
    let storage = Rc::new(Storage::new(&config.storage)?);
    let indexer_store = IndexerStore::new(storage)?;
    let indexer = Indexer::new(bitcoin_client, indexer_store, None)?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;

    for _ in 0..1000 {
        if rx.try_recv().is_ok() {
            info!("Stop Bitcoin Indexer");
            bitcoind.stop()?;
            break;
        }

        indexer.tick()?;

        let indexer_height = indexer.get_best_height()?;
        let blockchain_height = indexer.get_blockchain_best_height()?;

        if let Some(indexer_height) = indexer_height {
            if indexer_height == blockchain_height {
                info!("Waitting for a new blocks...");

                let invalidate_block_height = indexer_height.saturating_sub(30);
                let hash = bitcoin_client
                    .get_block_by_height(&invalidate_block_height)?
                    .unwrap()
                    .hash;

                info!("Invalidate blocks from HEIGHT({})", invalidate_block_height);
                bitcoin_client.invalidate_block(&hash)?;

                info!("Mining 100 blocks more....");
                let user_pubkey = get_random_pubkey();
                let wallet = bitcoin_client.get_new_address(user_pubkey, Network::Regtest);
                bitcoin_client.mine_blocks_to_address(100, &wallet)?;
                thread::sleep(Duration::from_secs(2));
            }
        }
    }

    bitcoind.stop()?;

    Ok(())
}

pub fn get_random_pubkey() -> PublicKey {
    let secp = secp256k1::Secp256k1::new();
    let mut rng = OsRng;
    let too_sk = SecretKey::new(&mut rng);
    let too_pk = SecpPublicKey::from_secret_key(&secp, &too_sk);
    PublicKey {
        compressed: true,
        inner: too_pk,
    }
}
