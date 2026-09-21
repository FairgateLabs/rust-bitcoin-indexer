#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use bitcoin::{
    absolute, transaction, Address, Amount, Block, BlockHash, Network, OutPoint, Transaction, Txid,
};
use bitcoin_indexer::{Indexer, IndexerError, IndexerSettings, IndexerStore, IndexerType};
use bitcoincore_rpc::json::CreateRawTransactionInput;
use bitcoincore_rpc::RpcApi;
use bitcoind::{bitcoind::Bitcoind, config::BitcoindConfig};
use bitvmx_bitcoin_rpc::{
    bitcoin_client::{BitcoinClient, BitcoinClientApi},
    rpc_config::RpcConfig,
    types::BlockHeight,
};
use storage_backend::{storage::Storage, storage_config::StorageConfig};
use tracing::info;

/// Upper bound for the ticks a test waits for the indexer to reach the node's tip.
const MAX_SYNC_TICKS: u32 = 1_000;

pub fn init_trace() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();
}

/// A name that is unique per process and call, used for storage directories.
fn unique_name(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

/// A transaction that is unique per `seed`, with no inputs.
pub fn dummy_tx(seed: u32) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::from_consensus(seed),
        input: vec![],
        output: vec![],
    }
}

pub fn settings(retention_depth: BlockHeight, catch_up: bool) -> Option<IndexerSettings> {
    Some(IndexerSettings::new(retention_depth, catch_up))
}

// =============================================================================
// Storage
// =============================================================================

/// An indexer store under `temp-runs/`, removed when this value is dropped.
pub struct TestStorage {
    path: String,
    store: Option<Rc<IndexerStore>>,
}

impl TestStorage {
    pub fn new() -> Self {
        let path = format!("temp-runs/{}", unique_name("indexer_storage"));
        let _ = std::fs::remove_dir_all(&path);

        let storage =
            Rc::new(Storage::new(&StorageConfig::new(path.clone(), None)).expect("test storage"));
        let store = Rc::new(IndexerStore::new(storage).expect("test store"));

        Self {
            path,
            store: Some(store),
        }
    }

    pub fn store(&self) -> Rc<IndexerStore> {
        Rc::clone(self.store.as_ref().expect("test store already removed"))
    }
}

impl Default for TestStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        self.store.take();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

// =============================================================================
// Regtest node
// =============================================================================

/// Only one regtest container can run at a time, because the container name and the RPC port are fixed.
static RPC_LOCK: Mutex<()> = Mutex::new(());

/// A fresh regtest bitcoind in Docker, with a funded wallet. The container is stopped when this value is dropped.
pub struct TestNode {
    pub rpc_config: RpcConfig,
    pub client: BitcoinClient,
    pub miner: Address,
    bitcoind: Bitcoind,
    _guard: MutexGuard<'static, ()>,
}

impl TestNode {
    /// Starts the node and mines `blocks` blocks to its wallet. Coinbase outputs need 100 blocks to be spendable.
    pub fn start(blocks: u64) -> anyhow::Result<Self> {
        let _guard = RPC_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let rpc_config = RpcConfig::new(
            Network::Regtest,
            "http://127.0.0.1:18443".to_string(),
            "foo".to_string(),
            "rpcpassword".to_string(),
            "test_wallet".to_string(),
        );

        let bitcoind = Bitcoind::new(BitcoindConfig::default(), rpc_config.clone(), None);
        info!("Starting bitcoind");
        bitcoind.start().map_err(|e| {
            anyhow::anyhow!("Failed to start bitcoind: {e:?}. Make sure Docker is running.")
        })?;

        let client = BitcoinClient::new_from_config(&rpc_config)?;
        let miner = client.init_wallet("test_wallet")?;

        let node = Self {
            rpc_config,
            client,
            miner,
            bitcoind,
            _guard,
        };
        node.mine(blocks)?;

        Ok(node)
    }

    /// An indexer on this node, with its own RPC client.
    pub fn indexer(
        &self,
        store: Rc<IndexerStore>,
        retention_depth: BlockHeight,
        catch_up: bool,
    ) -> Result<IndexerType, IndexerError> {
        let client = BitcoinClient::new_from_config(&self.rpc_config)?;
        Indexer::new(client, store, settings(retention_depth, catch_up))
    }

    pub fn tip(&self) -> anyhow::Result<BlockHeight> {
        Ok(self.client.get_tip_height()?)
    }

    pub fn hash_at(&self, height: BlockHeight) -> anyhow::Result<BlockHash> {
        Ok(self.client.get_block_id_by_height(&height)?)
    }

    pub fn block_at(&self, height: BlockHeight) -> anyhow::Result<Block> {
        Ok(self.client.get_block_by_hash(&self.hash_at(height)?)?)
    }

    pub fn coinbase_txid_at(&self, height: BlockHeight) -> anyhow::Result<Txid> {
        Ok(self.block_at(height)?.txdata[0].compute_txid())
    }

    // Every block is mined to a fresh address. Otherwise a block mined right after invalidateblock can be identical to
    // the invalidated one, and the node rejects it.

    /// Mines `blocks` blocks to the wallet. They include whatever is in the mempool.
    pub fn mine(&self, blocks: u64) -> anyhow::Result<()> {
        Ok(self
            .client
            .mine_blocks_to_address(blocks, &self.fresh_address()?)?)
    }

    /// Mines one block with no transaction other than its coinbase.
    pub fn mine_empty(&self) -> anyhow::Result<BlockHash> {
        Ok(self.client.mine_empty_block(&self.fresh_address()?)?)
    }

    /// Mines one block with exactly these transactions, in this order, ignoring the mempool.
    pub fn mine_with(&self, txs: &[Transaction]) -> anyhow::Result<BlockHash> {
        Ok(self
            .client
            .generate_block_with_txs(&self.fresh_address()?, txs)?)
    }

    /// Invalidates the block at `height` and every block above it. Returns the invalidated hash.
    pub fn invalidate(&self, height: BlockHeight) -> anyhow::Result<BlockHash> {
        let hash = self.hash_at(height)?;
        self.client.invalidate_block(&hash)?;
        Ok(hash)
    }

    /// Makes an invalidated block valid again, so the node can switch back to its chain.
    pub fn reconsider(&self, hash: &BlockHash) -> anyhow::Result<()> {
        Ok(self.client.client.reconsider_block(hash)?)
    }

    pub fn fresh_address(&self) -> anyhow::Result<Address> {
        Ok(self
            .client
            .client
            .get_new_address(None, None)?
            .assume_checked())
    }

    /// Sends `sats` to a fresh wallet address through the wallet, which broadcasts it.
    pub fn send(&self, sats: u64) -> anyhow::Result<Txid> {
        let address = self.fresh_address()?;
        Ok(self
            .client
            .send_to_address(&address, Amount::from_sat(sats))?)
    }

    /// Funds a fresh wallet address with `sats`, mining one block, and locks the output so the wallet does not
    /// spend it on its own.
    pub fn fund_utxo(&self, sats: u64) -> anyhow::Result<OutPoint> {
        let address = self.fresh_address()?;
        let (tx, vout) = self.client.fund_address(&address, Amount::from_sat(sats))?;
        let outpoint = OutPoint {
            txid: tx.compute_txid(),
            vout,
        };
        self.client.client.lock_unspent(&[outpoint])?;
        Ok(outpoint)
    }

    /// A signed, not broadcast transaction spending `outpoint` to a fresh wallet address.
    pub fn sign_spend(&self, outpoint: OutPoint, value_out: u64) -> anyhow::Result<Transaction> {
        let inputs = [CreateRawTransactionInput {
            txid: outpoint.txid,
            vout: outpoint.vout,
            sequence: None,
        }];
        let mut outputs = HashMap::new();
        outputs.insert(
            self.fresh_address()?.to_string(),
            Amount::from_sat(value_out),
        );

        let raw = self
            .client
            .client
            .create_raw_transaction(&inputs, &outputs, None, None)?;
        let signed = self
            .client
            .client
            .sign_raw_transaction_with_wallet(&raw, None, None)?;
        anyhow::ensure!(signed.complete, "signing incomplete: {:?}", signed.errors);

        Ok(bitcoin::consensus::deserialize(&signed.hex)?)
    }

    /// Two signed, not broadcast transactions spending the same output. The first pays the higher fee, so it can
    /// replace the second in the mempool.
    pub fn conflicting_txs(&self) -> anyhow::Result<(Transaction, Transaction)> {
        let outpoint = self.fund_utxo(1_000_000)?;
        let high_fee = self.sign_spend(outpoint, 900_000)?;
        let low_fee = self.sign_spend(outpoint, 950_000)?;
        anyhow::ensure!(high_fee.compute_txid() != low_fee.compute_txid());
        Ok((high_fee, low_fee))
    }

    /// Ticks until the indexer holds the node's tip block. The first tick is what places the cursor.
    pub fn sync(&self, indexer: &IndexerType) -> anyhow::Result<()> {
        for _ in 0..MAX_SYNC_TICKS {
            indexer.tick()?;

            if self.is_synced(indexer)? {
                return Ok(());
            }
        }
        anyhow::bail!("the indexer did not reach the node's tip")
    }

    /// True when the indexer's last block is the node's tip block, which also catches a reorg at the same height.
    /// False before the first tick, when there is no cursor yet.
    pub fn is_synced(&self, indexer: &IndexerType) -> anyhow::Result<bool> {
        let height = match indexer.get_indexed_height() {
            Ok(height) => height,
            Err(IndexerError::NotSynced) => return Ok(false),
            Err(error) => return Err(error.into()),
        };

        Ok(height == self.tip()?
            && indexer.get_last_indexed_block()?.hash == self.hash_at(height)?)
    }

    /// Asserts that every block from `from` to the tip is stored with the node's hash.
    pub fn assert_window_matches(
        &self,
        store: &IndexerStore,
        from: BlockHeight,
    ) -> anyhow::Result<()> {
        for height in from..=self.tip()? {
            let stored = store
                .get_block(height)?
                .unwrap_or_else(|| panic!("block {height} is not stored"));
            assert_eq!(stored.hash, self.hash_at(height)?, "block {height}");
        }
        Ok(())
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        info!("Stopping bitcoind");
        let _ = self.bitcoind.stop();
    }
}
