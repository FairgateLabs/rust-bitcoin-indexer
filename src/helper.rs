use bitcoin::Transaction;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::BlockHeight};
use tracing::{debug, error, trace, warn};

use crate::errors::IndexerError;

/// Where a fresh database, or a restart that is not catching up, starts indexing: one window below the tip.
/// Saturating, so a chain shorter than the window starts at genesis.
pub fn start_height(tip: BlockHeight, retention_depth: BlockHeight) -> BlockHeight {
    tip.saturating_sub(retention_depth)
}

/// Confirmations of a block, counted from the indexer's own cursor rather than the node's tip, so every
/// answer is counted against the chain the indexer has processed.
pub fn confirmations(cursor: BlockHeight, block_height: BlockHeight) -> u32 {
    cursor.saturating_sub(block_height).saturating_add(1)
}

/// The block that falls out of the window once the cursor reaches `cursor`, if any. Keeping the last N blocks
/// means keeping `cursor - N + 1 ..= cursor`, so `cursor - N` is the one to delete.
pub fn height_to_prune(cursor: BlockHeight, retention_depth: BlockHeight) -> Option<BlockHeight> {
    cursor.checked_sub(retention_depth)
}

/// Whether a block the node reports is older than everything the indexer holds, which is the only kind of
/// block the node is trusted to answer for using RPC. Above the cursor is a block the indexer has not reached.
pub fn is_older_than_window(
    height: BlockHeight,
    cursor: BlockHeight,
    block_stored_at_height: bool,
) -> bool {
    height <= cursor && !block_stored_at_height
}

/// Estimates the fee rate for the next block based on the middle transaction of the given block.
/// It implements a simple heuristic by selecting the transaction at the median position within the block's
/// transaction list, and computing `fee_rate = transaction_fee_in_sats / transaction_vsize_in_vb`.
pub fn estimate_fee_rate<B: BitcoinClientApi>(
    bitcoin_client: &B,
    txs: &[Transaction],
) -> Result<u64, IndexerError> {
    // TODO: Const might be settings in the future
    const MIN_BLOCK_TX: usize = 5;
    const ERROR_FEE_RATE: u64 = 0; // sat/vB
    const DEFAULT_MIN_FEE_RATE: u64 = 1; // sat/vB

    let block_tx_count = txs.len();
    trace!("Transactions count: {}", block_tx_count);

    //TODO:
    // In a future version we can analyze if the block is full or empty or which %,
    // a block lower than 75% will lead to lower fee rates for next block inclusion
    if block_tx_count <= MIN_BLOCK_TX {
        warn!(
            "Can't estimate fee rate - block has {} or fewer transactions",
            MIN_BLOCK_TX
        );
        return Ok(ERROR_FEE_RATE);
    }

    let middle_index = block_tx_count / 2;
    let middle_tx = &txs[middle_index];

    // Note: For coinbase transactions (first tx in block), there's no fee since there are no inputs.
    // Usually the block is ordered with coinbase as first transaction, but this is not enforced by consensus
    // rules, so this case might rarely happen.
    if middle_tx.is_coinbase() {
        warn!("Can't estimate fee rate - middle transaction is coinbase");
        return Ok(ERROR_FEE_RATE);
    }

    let tx_id = middle_tx.compute_txid();

    // This call needs bitcoin core 25.0.0 or higher
    // see https://bitcoincore.org/en/doc/25.0.0/rpc/rawtransactions/getrawtransaction/
    let raw_tx_verbose = bitcoin_client.get_raw_transaction_verbosity_two(&tx_id)?;

    let fee_in_btc = match raw_tx_verbose.get("fee").and_then(|v| v.as_f64()) {
        Some(fee_value) => fee_value,
        None => {
            error!(
                "Can't estimate fee rate - no fee value available for transaction {}",
                tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let vsize = match raw_tx_verbose.get("vsize").and_then(|v| v.as_u64()) {
        Some(vsize_value) => vsize_value,
        None => {
            error!(
                "Can't estimate fee rate - no vsize value available for transaction {}",
                tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let fee = match bitcoin::Amount::from_btc(fee_in_btc) {
        Ok(amount) => amount.to_sat(),
        Err(_) => {
            error!(
                "Can't estimate fee rate - invalid fee value {} for transaction {}",
                fee_in_btc, tx_id
            );
            return Ok(ERROR_FEE_RATE);
        }
    };

    let fee_rate = fee as f64 / vsize as f64;
    let adjusted_fee_rate = if fee_rate < 1.0 {
        DEFAULT_MIN_FEE_RATE
    } else {
        fee_rate as u64
    };

    debug!("TXID: {:#?}", tx_id);
    debug!("Adjusted fee rate: {} sat/vB", adjusted_fee_rate);
    trace!("middle index: {}", middle_index);
    trace!("Transaction fee: {} sats", fee);
    trace!("Transaction vsize: {} vB", vsize);
    trace!("Transaction fee rate: {} sat/vB", fee_rate);

    Ok(adjusted_fee_rate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::dummy_tx;
    use bitvmx_bitcoin_rpc::bitcoin_client::MockBitcoinClientApi;

    // A fresh start begins one window below the tip.
    #[test]
    fn start_below_tip() {
        assert_eq!(start_height(1000, 100), 900);
        assert_eq!(start_height(100, 100), 0);
    }

    // A chain shorter than the window starts at genesis instead of wrapping.
    #[test]
    fn start_short_chain() {
        // Release builds wrap on overflow instead of panicking, so this has to saturate explicitly.
        assert_eq!(start_height(3, 100), 0);
        assert_eq!(start_height(0, 100), 0);
    }

    // Confirmations count the block itself.
    #[test]
    fn confirmations_count() {
        assert_eq!(confirmations(100, 100), 1);
        assert_eq!(confirmations(100, 99), 2);
        assert_eq!(confirmations(100, 1), 100);
    }

    // A block above the cursor does not wrap the confirmation count.
    #[test]
    fn confirmations_no_wrap() {
        assert_eq!(confirmations(100, 101), 1);
    }

    // Nothing is pruned until the window is full.
    #[test]
    fn prune_waits_for_full_window() {
        assert_eq!(height_to_prune(99, 100), None);
        assert_eq!(height_to_prune(100, 100), Some(0));
        assert_eq!(height_to_prune(150, 100), Some(50));
    }

    // A block above the cursor has not been processed, so it is not old.
    #[test]
    fn above_cursor_not_old() {
        assert!(!is_older_than_window(11, 10, false));
    }

    // A different block at a held height is a reorg the indexer has not unwound yet.
    #[test]
    fn held_height_not_old() {
        assert!(!is_older_than_window(10, 10, true));
        assert!(!is_older_than_window(5, 10, true));
    }

    // A height below everything the indexer holds is old.
    #[test]
    fn below_window_old() {
        assert!(is_older_than_window(5, 10, false));
        assert!(is_older_than_window(0, 10, false));
    }

    // A block with few transactions has no fee rate and makes no call.
    #[test]
    fn fee_small_block() {
        let bitcoin_client = MockBitcoinClientApi::new();
        let txs = vec![dummy_tx(1), dummy_tx(2)];
        assert_eq!(estimate_fee_rate(&bitcoin_client, &txs).unwrap(), 0);
    }

    // The fee rate comes from the middle transaction of the block.
    #[test]
    fn fee_from_middle_tx() {
        let txs: Vec<_> = (0..7).map(dummy_tx).collect();
        let middle = txs[3].compute_txid();

        let mut bitcoin_client = MockBitcoinClientApi::new();
        bitcoin_client
            .expect_get_raw_transaction_verbosity_two()
            .withf(move |txid| *txid == middle)
            .returning(|_| Ok(serde_json::json!({ "fee": 0.00001, "vsize": 200 })));

        // 0.00001 BTC is 1000 sats over 200 vB, so 5 sat/vB.
        assert_eq!(estimate_fee_rate(&bitcoin_client, &txs).unwrap(), 5);
    }

    // A fee rate below 1 sat/vB is raised to the floor.
    #[test]
    fn fee_floor() {
        let txs: Vec<_> = (0..7).map(dummy_tx).collect();

        let mut bitcoin_client = MockBitcoinClientApi::new();
        bitcoin_client
            .expect_get_raw_transaction_verbosity_two()
            .returning(|_| Ok(serde_json::json!({ "fee": 0.00000001, "vsize": 200 })));

        assert_eq!(estimate_fee_rate(&bitcoin_client, &txs).unwrap(), 1);
    }

    // A node answer without a fee gives no rate.
    #[test]
    fn fee_missing() {
        let txs: Vec<_> = (0..7).map(dummy_tx).collect();

        let mut bitcoin_client = MockBitcoinClientApi::new();
        bitcoin_client
            .expect_get_raw_transaction_verbosity_two()
            .returning(|_| Ok(serde_json::json!({ "vsize": 200 })));

        assert_eq!(estimate_fee_rate(&bitcoin_client, &txs).unwrap(), 0);
    }
}
