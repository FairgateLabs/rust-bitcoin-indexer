use bitcoin::Transaction;
use bitvmx_bitcoin_rpc::{bitcoin_client::BitcoinClientApi, types::BlockHeight};
use tracing::{debug, error, trace, warn};

use crate::errors::IndexerError;

/// Where a fresh database, or a restart that is not catching up, starts indexing: one window below the tip.
/// Saturating, so a chain shorter than the window starts at genesis.
pub fn window_start(tip: BlockHeight, retention_depth: BlockHeight) -> BlockHeight {
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

/// Whether a block the node reports is below every block the indexer holds, which is the only kind of block the
/// node is trusted to answer for using RPC. A block above the indexed height has not been reached yet.
pub fn is_below_window(
    height: BlockHeight,
    indexed_height: BlockHeight,
    height_is_stored: bool,
) -> bool {
    height <= indexed_height && !height_is_stored
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

    // A fresh start begins one window below the tip, and saturates at genesis on a shorter chain.
    #[test]
    fn start_of_window() {
        // A long chain.
        assert_eq!(window_start(1000, 100), 900);
        assert_eq!(window_start(100, 100), 0);

        // A chain shorter than the window.
        assert_eq!(window_start(3, 100), 0);
        assert_eq!(window_start(0, 100), 0);
    }

    // Confirmations count the block itself and never wrap.
    #[test]
    fn confirmations_count() {
        assert_eq!(confirmations(100, 100), 1);
        assert_eq!(confirmations(100, 99), 2);
        assert_eq!(confirmations(100, 1), 100);

        // A block above the cursor.
        assert_eq!(confirmations(100, 101), 1);
    }

    // Nothing is pruned until the window is full, then the block N below the cursor is.
    #[test]
    fn prune_height() {
        assert_eq!(height_to_prune(99, 100), None);
        assert_eq!(height_to_prune(100, 100), Some(0));
        assert_eq!(height_to_prune(150, 100), Some(50));
    }

    // Only a height at or below the cursor, where the indexer holds no block, is below the window.
    #[test]
    fn below_window() {
        // Below everything the indexer holds.
        assert!(is_below_window(5, 10, false));
        assert!(is_below_window(0, 10, false));

        // Above the cursor, not processed yet.
        assert!(!is_below_window(11, 10, false));

        // A different block at a held height, a reorg the indexer has not unwound yet.
        assert!(!is_below_window(10, 10, true));
        assert!(!is_below_window(5, 10, true));
    }

    // The fee rate comes from the middle transaction, with a floor of 1 sat/vB and 0 when it cannot be computed.
    #[test]
    fn fee_rate() {
        // A block with few transactions has no fee rate and makes no call.
        let bitcoin_client = MockBitcoinClientApi::new();
        let small_block = vec![dummy_tx(1), dummy_tx(2)];
        assert_eq!(estimate_fee_rate(&bitcoin_client, &small_block).unwrap(), 0);

        let txs: Vec<_> = (0..7).map(dummy_tx).collect();
        let middle = txs[3].compute_txid();
        let cases = [
            // 0.00001 BTC is 1000 sats over 200 vB, so 5 sat/vB.
            (serde_json::json!({ "fee": 0.00001, "vsize": 200 }), 5),
            // Below 1 sat/vB is raised to the floor.
            (serde_json::json!({ "fee": 0.00000001, "vsize": 200 }), 1),
            // An answer without a fee gives no rate.
            (serde_json::json!({ "vsize": 200 }), 0),
        ];

        for (answer, expected) in cases {
            let mut bitcoin_client = MockBitcoinClientApi::new();
            bitcoin_client
                .expect_get_raw_transaction_verbosity_two()
                .withf(move |txid| *txid == middle)
                .returning(move |_| Ok(answer.clone()));

            assert_eq!(estimate_fee_rate(&bitcoin_client, &txs).unwrap(), expected);
        }
    }
}
