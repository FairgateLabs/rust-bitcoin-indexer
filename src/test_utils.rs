use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use bitcoin::hashes::Hash;
use bitcoin::{absolute, transaction, BlockHash, Transaction};
use bitvmx_bitcoin_rpc::types::BlockHeight;
use storage_backend::storage::Storage;
use storage_backend::storage_config::StorageConfig;

use crate::store::IndexerStore;
use crate::types::FullBlock;

/// A transaction that is unique per `seed`, with no inputs.
pub fn dummy_tx(seed: u32) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::from_consensus(seed),
        input: vec![],
        output: vec![],
    }
}

pub fn block_hash(bytes: [u8; 32]) -> BlockHash {
    BlockHash::from_byte_array(bytes)
}

/// The record the indexer stores.
pub fn full_block(
    height: BlockHeight,
    hash: [u8; 32],
    prev_hash: [u8; 32],
    txs: Vec<Transaction>,
) -> FullBlock {
    FullBlock {
        height,
        hash: block_hash(hash),
        prev_hash: block_hash(prev_hash),
        txs,
        estimated_fee_rate: 0,
    }
}

/// A store backed by a fresh directory, removed when the process exits.
pub fn temp_store() -> Rc<IndexerStore> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let unique = format!(
        "bitcoin_indexer_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    let path = std::env::temp_dir().join(unique);
    let _ = std::fs::remove_dir_all(&path);

    let config = StorageConfig::new(path.to_string_lossy().to_string(), None);
    let storage = Rc::new(Storage::new(&config).expect("storage"));

    Rc::new(IndexerStore::new(storage).expect("store"))
}
