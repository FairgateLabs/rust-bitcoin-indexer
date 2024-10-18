pub use bitcoin::hash_types::{BlockHash, Txid};
pub use bitcoin::hashes::{hash160::Hash as Hash160, sha256d::Hash as Sha256dHash};
pub use bitcoin::hashes::{hex::FromHex as _, Hash};
use serde::{Deserialize, Serialize};

pub type BlockHeight = u32;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BlockInfo {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Txid>,
}
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Txid>,
    pub orphan: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TransactionInfo {
    pub tx_id: Txid,
    pub block_height: BlockHeight,
    pub block_hash: BlockHash,
    pub orphan: bool,
}
