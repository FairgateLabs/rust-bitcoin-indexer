use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use bitcoin::blockdata::block::{Block, Header, Version as BlockVersion};
use bitcoin::blockdata::script;
use bitcoin::hashes::Hash;
use bitcoin::{
    absolute, transaction, BlockHash, CompactTarget, OutPoint, Sequence, Transaction, TxIn,
    TxMerkleNode, Witness,
};
use bitvmx_bitcoin_rpc::types::{BlockHeight, BlockInfo};
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

/// A coinbase carrying its block height.
pub fn coinbase_at(height: BlockHeight) -> Transaction {
    Transaction {
        version: transaction::Version::TWO,
        lock_time: absolute::LockTime::ZERO,
        input: vec![TxIn {
            previous_output: OutPoint::null(),
            script_sig: script::Builder::new().push_int(height as i64).into_script(),
            sequence: Sequence::MAX,
            witness: Witness::new(),
        }],
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

/// What the node returns for a block read by height.
pub fn block_info(
    height: BlockHeight,
    hash: [u8; 32],
    prev_hash: [u8; 32],
    txs: Vec<Transaction>,
) -> BlockInfo {
    BlockInfo {
        height,
        hash: block_hash(hash),
        prev_hash: block_hash(prev_hash),
        txs,
    }
}

pub fn block_at_height(height: BlockHeight, prev_hash: [u8; 32]) -> Block {
    let txdata = vec![coinbase_at(height)];

    Block {
        header: Header {
            version: BlockVersion::TWO,
            prev_blockhash: block_hash(prev_hash),
            merkle_root: TxMerkleNode::all_zeros(),
            time: 0,
            bits: CompactTarget::from_consensus(0x1d00ffff),
            nonce: 0,
        },
        txdata,
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
