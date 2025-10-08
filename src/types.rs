use bitcoin::{BlockHash, Transaction};
use bitvmx_bitcoin_rpc::types::BlockHeight;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FullBlock {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Transaction>,
    pub orphan: bool,
    pub estimated_fee_rate: u64, // in sat/vB
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TransactionInfo {
    pub tx: Transaction,
    pub block_info: FullBlock,
    pub confirmations: u32,
}
