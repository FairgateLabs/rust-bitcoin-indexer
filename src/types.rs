use bitcoin::{BlockHash, Transaction, Txid};
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
    pub tx: Option<Transaction>,
    pub block_info: Option<FullBlock>,
    pub confirmations: u32,
    pub status: TransactionStatus,
    pub confirmation_threshold: u32,
}

impl TransactionInfo {
    pub fn new(
        tx: Transaction,
        block_info: FullBlock,
        status: TransactionStatus,
        confirmations: u32,
        confirmation_threshold: u32,
    ) -> Self {
        Self {
            tx: Some(tx),
            block_info: Some(block_info),
            confirmations,
            status,
            confirmation_threshold,
        }
    }

    pub fn is_finalized(&self) -> bool {
        // A transaction is considered finalized if:
        // - The status is Finalized
        // - The number of confirmations meets or exceeds the confirmation threshold
        self.confirmations >= self.confirmation_threshold
            && self.status == TransactionStatus::Finalized
    }

    pub fn is_confirmed(&self) -> bool {
        // A transaction is considered confirmed if it has been included in a block
        // and has at least one confirmation (confirmations > 0), regardless of the exact number of confirmations.
        // This means the transaction is in the main chain and not orphaned.
        self.confirmations > 0 && self.status == TransactionStatus::Confirmed
    }

    pub fn is_orphan(&self) -> bool {
        // An orphan transaction should have:
        //  block_info because it was mined at some point before.
        //  confirmations == 0, this is just a validation - orphan transactions should be moved to confirmation 0.
        //  is_orphan = true
        //  status = Orphan
        if let Some(block_info) = &self.block_info {
            self.confirmations == 0 && block_info.orphan && self.status == TransactionStatus::Orphan
        } else {
            false
        }
    }

    pub fn is_in_mempool(&self) -> bool {
        self.status == TransactionStatus::InMempool
    }

    pub fn is_not_found(&self) -> bool {
        self.status == TransactionStatus::NotFound
    }

    pub fn tx_id(&self) -> Txid {
        self.tx.as_ref().unwrap().compute_txid()
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub enum TransactionStatus {
    // Represents a transaction that has been successfully confirmed by the network but a reorganization moved it out of the chain.
    Orphan,
    // Represents a transaction that has been successfully confirmed by the network
    Confirmed,
    // Represents when the transaction was confirmed by a certain number of blocks
    Finalized,

    // Represents when the transaction is in mempool and not yet confirmed
    InMempool,

    // Indicates that the transaction is not present in the blockchain, not in the mempool, and has not been confirmed.
    NotFound,
}
