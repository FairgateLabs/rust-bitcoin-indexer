//! Regression tests for the Testnet incident observed on 2026-06-09.
//!
//! Symptom: the indexer logged "Block already saved at height 4977629" on every
//! tick and never advanced, so the client never reached "Sync complete".
//!
//! Two defects in `IndexerStore::save_new_best_block` (see `src/store.rs`) are
//! exercised here. The early-return branch fires when a block is already stored
//! at its height with a matching hash:
//!
//!   if saved_block.hash == block.hash {
//!       warn!("Block already saved at height {}", block.height);
//!       return Ok(());
//!   }
//!
//!   Defect A (produces the stuck loop): the branch does not call
//!     `save_best_height`, so the best-height cursor never advances. In
//!     `Indexer::tick`, when the cursor sits one block behind a block that is
//!     already stored with the matching hash, every tick re-saves that block,
//!     hits this branch, and leaves the cursor unchanged: an infinite loop.
//!
//!   Defect B (the orphan gap): the branch ignores `saved_block.orphan`. After
//!     a transient reorg that re-converges to the same chain, the stored block
//!     was marked orphan by the rollback (see `Indexer::tick` reorg path, which
//!     calls `mark_following_blocks_as_orphan` then `save_best_height(h - 1)`).
//!     When the chain re-converges, the incoming block has the same hash, so the
//!     branch returns without clearing the orphan flag. The re-converged
//!     canonical block stays flagged orphan, and `get_tx_info` then reports
//!     0 confirmations / orphan for every transaction in it.
//!
//! How the cursor ends up behind an already-stored block in production:
//!   - Reorg re-convergence (the realistic Testnet trigger): the stored block
//!     is marked orphan. Covered by `cursor_stuck_after_reorg_reconvergence`.
//!   - Crash between the height-to-hash write and the cursor write (the five
//!     writes are not atomic): the stored block is not orphan. Covered by
//!     `cursor_stuck_after_crash_during_save`.
//!
//! These tests reproduce the stuck state at the store level. The reorg test
//! replays exactly the two store mutations that `Indexer::tick`'s reorg path
//! performs, so no Bitcoin node or RPC mock is required.
//!
//! Expected results:
//!   - Against the current (buggy) code: both tests FAIL (cursor stuck; in the
//!     reorg case the orphan flag also remains set).
//!   - After the fix (in the early-return branch, clear `orphan` if set and call
//!     `save_best_height(block.height)` before returning): both tests PASS.
//!
//! Run with:
//!   cargo test -p bitcoin-indexer --test cursor_stuck_regression_test

use bitcoin::BlockHash;
use bitcoin_indexer::store::StoreClient;
use bitvmx_bitcoin_rpc::types::{BlockHeight, BlockInfo};
use std::str::FromStr;

use crate::utils::{clear_output, get_indexer_store};
mod utils;

// Heights taken from the incident: the cursor was stuck at 4977628 while block
// 4977629 was already stored with the matching hash.
const STUCK_CURSOR_HEIGHT: BlockHeight = 4_977_628;
const NEXT_BLOCK_HEIGHT: BlockHeight = 4_977_629;

// Valid block hashes (64 hex chars each).
const PREV_HASH: &str = "0000000000000000000a1e2b6f1f3b7f0a1f1e2b6f1f3b7f0a1f1e2b6f1f3b7f";
const PREV_BLOCK_HASH: &str = "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f";
const NEXT_BLOCK_HASH: &str = "0000000000000000000c1e2b6f1f3b7f0c1f1e2b6f1f3b7f0c1f1e2b6f1f3b7f";

fn block(height: BlockHeight, hash: &str, prev_hash: &str) -> BlockInfo {
    BlockInfo {
        height,
        hash: BlockHash::from_str(hash).unwrap(),
        prev_hash: BlockHash::from_str(prev_hash).unwrap(),
        txs: vec![],
    }
}

/// Realistic Testnet trigger: a transient reorg rolled the cursor back and
/// marked the tip block orphan, then the chain re-converged to that same block.
#[test]
fn cursor_stuck_after_reorg_reconvergence() -> Result<(), anyhow::Error> {
    let store = get_indexer_store();

    let prev_block = block(STUCK_CURSOR_HEIGHT, PREV_BLOCK_HASH, PREV_HASH);
    let next_block = block(NEXT_BLOCK_HEIGHT, NEXT_BLOCK_HASH, PREV_BLOCK_HASH);

    // 1. Index both blocks normally. Cursor reaches 4977629; block 4977629 is
    //    stored under its height with the matching hash and orphan = false.
    store.save_new_best_block(&prev_block, 0)?;
    store.save_new_best_block(&next_block, 0)?;
    assert_eq!(store.get_best_height()?, Some(NEXT_BLOCK_HEIGHT));

    // 2. Replay exactly what Indexer::tick's reorg path does at height 4977629:
    //    mark the tip block (and following) as orphan, then roll the cursor back
    //    by one. This is the on-disk state a transient reorg leaves behind.
    store.mark_following_blocks_as_orphan(NEXT_BLOCK_HEIGHT)?;
    store.save_best_height(STUCK_CURSOR_HEIGHT)?;

    let orphaned = store
        .get_block_by_height(NEXT_BLOCK_HEIGHT)?
        .expect("block 4977629 must still be stored");
    assert!(
        orphaned.orphan,
        "setup: the reorg rollback must have marked block 4977629 orphan"
    );
    assert_eq!(store.get_best_height()?, Some(STUCK_CURSOR_HEIGHT));

    // 3. The chain re-converges to the same block 4977629. This is what
    //    Indexer::tick does on the next tick: re-save the now-canonical block.
    //    Repeat a few times to mirror successive ticks.
    for tick in 1..=5 {
        store.save_new_best_block(&next_block, 0)?;

        let cursor = store.get_best_height()?;
        assert_eq!(
            cursor,
            Some(NEXT_BLOCK_HEIGHT),
            "after tick {tick}: re-saving the re-converged block 4977629 must advance \
             the cursor to 4977629, but it is at {cursor:?}. The cursor is stuck: this \
             is the infinite sync loop seen on Testnet (Defect A)."
        );
    }

    // 4. The re-converged block is canonical again, so its orphan flag must be
    //    cleared. Otherwise get_tx_info reports 0 confirmations for its txs.
    let reconverged = store
        .get_block_by_height(NEXT_BLOCK_HEIGHT)?
        .expect("block 4977629 must be stored");
    assert!(
        !reconverged.orphan,
        "block 4977629 re-converged onto the canonical chain, so its orphan flag must \
         be cleared, but it is still marked orphan (Defect B). Transactions in this \
         block would be reported with 0 confirmations."
    );

    clear_output();
    Ok(())
}

/// Alternative trigger: the process was killed between the height-to-hash write
/// and the cursor write inside save_new_best_block (the five writes are not
/// atomic). The stored block is left non-orphan, but the cursor is behind it.
#[test]
fn cursor_stuck_after_crash_during_save() -> Result<(), anyhow::Error> {
    let store = get_indexer_store();

    let prev_block = block(STUCK_CURSOR_HEIGHT, PREV_BLOCK_HASH, PREV_HASH);
    let next_block = block(NEXT_BLOCK_HEIGHT, NEXT_BLOCK_HASH, PREV_BLOCK_HASH);

    // Index both blocks, then simulate the crash by rolling the cursor back
    // while leaving block 4977629 stored (non-orphan).
    store.save_new_best_block(&prev_block, 0)?;
    store.save_new_best_block(&next_block, 0)?;
    store.save_best_height(STUCK_CURSOR_HEIGHT)?;
    assert_eq!(store.get_best_height()?, Some(STUCK_CURSOR_HEIGHT));

    // The next tick re-saves block 4977629, which is already stored with the
    // matching hash. The cursor must advance.
    for tick in 1..=5 {
        store.save_new_best_block(&next_block, 0)?;
        let cursor = store.get_best_height()?;
        assert_eq!(
            cursor,
            Some(NEXT_BLOCK_HEIGHT),
            "after tick {tick}: re-saving already-stored block 4977629 must advance the \
             cursor to 4977629, but it is at {cursor:?} (Defect A)."
        );
    }

    clear_output();
    Ok(())
}
