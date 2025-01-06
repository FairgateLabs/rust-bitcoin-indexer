pub use bitcoin::hash_types::{BlockHash, Txid};
pub use bitcoin::hashes::{hash160::Hash as Hash160, sha256d::Hash as Sha256dHash};
pub use bitcoin::hashes::{hex::FromHex as _, Hash};
use bitcoin::Transaction;
use serde::{Deserialize, Serialize};

pub type BlockHeight = u32;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Transaction>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FullBlock {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Transaction>,
    pub orphan: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TransactionInfo {
    pub tx: Transaction,
    pub block_height: BlockHeight,
    pub block_hash: BlockHash,
    pub orphan: bool,
    pub confirmations: u32,
}
