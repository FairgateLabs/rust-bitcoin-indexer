pub use bitcoin::hash_types::{BlockHash, Txid};
pub use bitcoin::hashes::{hash160::Hash as Hash160, sha256d::Hash as Sha256dHash};
pub use bitcoin::hashes::{hex::FromHex as _, Hash};
use serde::{Deserialize, Serialize};

pub type BlockHeight = u32;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlockInfo {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
}

/// Block data from BitcoinCore (`rust-bitcoin`)
pub type BlockHex = String;
pub type TxHex = String;
pub type TxHash = Sha256dHash;
