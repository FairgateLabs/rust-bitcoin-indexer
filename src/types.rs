use bitcoin::{BlockHash, Transaction, Txid};
use bitvmx_bitcoin_rpc::types::BlockHeight;
use serde::{Deserialize, Serialize};

use crate::errors::IndexerError;

/// A block the indexer holds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FullBlock {
    pub height: BlockHeight,
    pub hash: BlockHash,
    pub prev_hash: BlockHash,
    pub txs: Vec<Transaction>,
    pub estimated_fee_rate: u64, // In sat/vB.
}

/// What the indexer knows about a transaction.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "status")]
pub enum TransactionStatus {
    Confirmed {
        tx: Transaction,
        block_height: BlockHeight,
        block_hash: BlockHash,
        confirmations: u32,
    },
    InMempool,
    NotFound,
}

impl TransactionStatus {
    pub fn new(
        tx: Transaction,
        block_height: BlockHeight,
        block_hash: BlockHash,
        confirmations: u32,
    ) -> Self {
        Self::Confirmed {
            tx,
            block_height,
            block_hash,
            confirmations,
        }
    }

    pub fn is_confirmed(&self) -> bool {
        matches!(self, Self::Confirmed { .. })
    }

    pub fn is_in_mempool(&self) -> bool {
        matches!(self, Self::InMempool)
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }

    /// Confirmations of the block that contains the transaction, and zero for anything not confirmed.
    pub fn confirmations(&self) -> u32 {
        match self {
            Self::Confirmed { confirmations, .. } => *confirmations,
            _ => 0,
        }
    }

    /// True once the transaction is buried deep enough for the caller to treat it as final.
    pub fn is_finalized(&self, required_confirmations: u32) -> bool {
        required_confirmations > 0 && self.confirmations() >= required_confirmations
    }

    pub fn tx_or_err(&self) -> Result<&Transaction, IndexerError> {
        match self {
            Self::Confirmed { tx, .. } => Ok(tx),
            _ => Err(IndexerError::NotConfirmed),
        }
    }

    pub fn tx_id_or_err(&self) -> Result<Txid, IndexerError> {
        Ok(self.tx_or_err()?.compute_txid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{block_hash, dummy_tx};

    fn confirmed(confirmations: u32) -> TransactionStatus {
        TransactionStatus::new(dummy_tx(0), 7, block_hash([1u8; 32]), confirmations)
    }

    // Every variant serializes with its name in a status field, and comes back unchanged.
    #[test]
    fn serde() {
        // Confirmed puts its data next to the status field.
        let value = serde_json::to_value(confirmed(4)).unwrap();
        assert_eq!(value["status"], "Confirmed");
        assert_eq!(value["confirmations"], 4);
        assert_eq!(value["block_height"], 7);
        assert!(value["block_hash"].is_string());
        assert!(value["tx"].is_object());
        assert!(value.get("block_info").is_none());

        // InMempool and NotFound are the status field alone.
        let value = serde_json::to_value(TransactionStatus::NotFound).unwrap();
        assert_eq!(value["status"], "NotFound");
        assert!(value.get("tx").is_none());

        let value = serde_json::to_value(TransactionStatus::InMempool).unwrap();
        assert_eq!(value["status"], "InMempool");

        // Round trip of every variant.
        for status in [
            confirmed(2),
            TransactionStatus::InMempool,
            TransactionStatus::NotFound,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(
                serde_json::from_str::<TransactionStatus>(&json).unwrap(),
                status
            );
        }
    }

    // Every predicate and accessor answers correctly for each variant.
    #[test]
    fn predicates() {
        let status = confirmed(5);
        assert!(status.is_confirmed());
        assert!(!status.is_in_mempool());
        assert!(!status.is_not_found());
        assert_eq!(status.confirmations(), 5);
        assert!(status.is_finalized(5));
        assert!(!status.is_finalized(6));
        assert!(status.tx_or_err().is_ok());
        assert_eq!(status.tx_id_or_err().unwrap(), dummy_tx(0).compute_txid());

        for other in [TransactionStatus::InMempool, TransactionStatus::NotFound] {
            assert!(!other.is_confirmed());
            assert_eq!(other.confirmations(), 0);
            assert!(!other.is_finalized(1));
            assert!(other.tx_or_err().is_err());
        }
    }
}
