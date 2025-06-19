use bitcoin::{absolute::LockTime, transaction::Version, BlockHash, Transaction};
use bitcoin_indexer::{store::StoreClient, types::FullBlock};
use bitvmx_bitcoin_rpc::types::BlockInfo;
use std::str::FromStr;

use crate::utils::{clear_output, get_indexer_store};
mod utils;

#[test]
fn get_best_block_test() -> Result<(), anyhow::Error> {
    let indexer_store = get_indexer_store();
    let height = indexer_store.get_best_block()?;
    assert_eq!(height, None);

    let block_1 = BlockInfo {
        height: 1,
        hash: BlockHash::from_str(
            "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "0000000000000000000a1e2b6f1f3b7f0a1f1e2b6f1f3b7f0a1f1e2b6f1f3b7f",
        )
        .unwrap(),
        txs: vec![],
    };

    let expected_block = FullBlock {
        height: block_1.height,
        hash: block_1.hash,
        prev_hash: block_1.prev_hash,
        txs: block_1.txs.clone(),
        orphan: false,
    };

    //Insert block at height 1 and check best block
    indexer_store.save_new_best_block(&block_1)?;
    let best_block = indexer_store.get_best_block()?;
    assert_eq!(best_block, Some(expected_block));

    let block_2 = BlockInfo {
        height: 2,
        hash: BlockHash::from_str(
            "0000000000000000000c1e2b6f1f3b7f0c1f1e2b6f1f3b7f0c1f1e2b6f1f3b7f",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "0000000000000000000c1e2b6f1f3b7f0c1f1e2b6f1f3b7f0c1f1e2b6f1f3b7f",
        )
        .unwrap(),
        txs: vec![],
    };

    //Insert block at height 2 and check best block
    indexer_store.save_new_best_block(&block_2)?;
    let best_block = indexer_store.get_best_block()?;

    let expected_block_2 = FullBlock {
        height: block_2.height,
        hash: block_2.hash,
        prev_hash: block_2.prev_hash,
        txs: block_2.txs.clone(),
        orphan: false,
    };

    assert_eq!(best_block, Some(expected_block_2.clone()));

    //Insert block at height 1 again and check best block
    indexer_store.save_new_best_block(&block_1)?;
    let block_again = indexer_store.get_best_block()?;
    assert_eq!(block_again, Some(expected_block_2));

    clear_output();

    Ok(())
}

#[test]
fn save_block_test() -> Result<(), anyhow::Error> {
    let indexer_store = get_indexer_store();

    let block_hash_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7a")
            .unwrap();

    let block_hash_2 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7b")
            .unwrap();

    let block_1 = BlockInfo {
        height: 1,
        hash: block_hash_1,
        prev_hash: block_hash_2,
        txs: vec![],
    };

    //Insert block_1 and check get_block_by_hash and get_block_hash_by_height
    indexer_store.save_new_best_block(&block_1)?;
    let saved_block_1 = indexer_store.get_block_by_hash(&block_hash_1)?.unwrap();
    assert_eq!(saved_block_1.hash, block_1.hash);
    assert_eq!(saved_block_1.height, block_1.height);
    assert_eq!(saved_block_1.orphan, false);
    assert_eq!(saved_block_1.prev_hash, block_1.prev_hash);

    let block_hash = indexer_store.get_block_hash_by_height(1)?.unwrap();
    assert_eq!(block_hash, block_1.hash);

    // NEW BLOCK TO INSERT
    let block_hash_new_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7c")
            .unwrap();

    let new_block_1 = BlockInfo {
        height: 1,
        hash: block_hash_new_1,
        prev_hash: block_hash_2,
        txs: vec![],
    };

    //Insert block_1 at the same height 1 check get_block_by_hash and get_block_hash_by_height
    indexer_store.save_new_best_block(&new_block_1)?;
    let saved_new_block_1 = indexer_store.get_block_by_hash(&block_hash_new_1)?.unwrap();
    assert_eq!(saved_new_block_1.hash, new_block_1.hash);
    assert_eq!(saved_new_block_1.height, new_block_1.height);
    assert_eq!(saved_new_block_1.orphan, false);
    assert_eq!(saved_new_block_1.prev_hash, new_block_1.prev_hash);

    let block_hash = indexer_store.get_block_hash_by_height(1)?.unwrap();
    assert_eq!(block_hash, new_block_1.hash);

    //Check data block for the previous block at height 1
    let saved_block_1 = indexer_store.get_block_by_hash(&block_hash_1)?.unwrap();
    assert_eq!(saved_block_1.hash, block_1.hash);
    assert_eq!(saved_block_1.height, block_1.height);
    assert_eq!(saved_block_1.orphan, true);
    assert_eq!(saved_block_1.prev_hash, block_1.prev_hash);

    let block_hash = indexer_store.get_block_hash_by_height(1)?.unwrap();
    assert_eq!(block_hash, new_block_1.hash);

    clear_output();

    Ok(())
}

#[test]
fn get_tx_info_test() -> Result<(), anyhow::Error> {
    let indexer_store = get_indexer_store();

    let block_hash_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7a")
            .unwrap();

    let block_hash_2: BlockHash =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7b")
            .unwrap();

    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    let tx_id = tx.compute_txid();

    let block_1 = BlockInfo {
        height: 1,
        hash: block_hash_1,
        prev_hash: block_hash_2,
        txs: vec![tx.clone()],
    };
    //1) Save block_1 and check get_tx_info method, transaction with tx_id should exist
    indexer_store.save_new_best_block(&block_1)?;
    let tx_info = indexer_store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx.compute_txid(), tx_id);
    assert_eq!(tx_info.block_info.height, block_1.height);
    assert_eq!(tx_info.block_info.orphan, false);
    assert_eq!(tx_info.block_info.hash, block_1.hash);
    assert_eq!(tx_info.confirmations, 1);

    //Creating a new block for height 1
    let block_hash_new_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7c")
            .unwrap();

    let new_block_1 = BlockInfo {
        height: 1,
        hash: block_hash_new_1,
        prev_hash: block_hash_2,
        txs: vec![],
    };

    //2) Insert new_block_1 and check get_tx_info, transaction should be orphan.
    //A block with the same height was inserted, it means that there was an reorganization and transaction was moved to meempool.
    // Then transaction was not mined. But we keep in our database that the transaction was seen.

    indexer_store.save_new_best_block(&new_block_1)?;

    let tx_info = indexer_store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx.compute_txid(), tx_id);
    assert_eq!(tx_info.block_info.height, block_1.height);
    assert_eq!(tx_info.block_info.orphan, true);
    assert_eq!(tx_info.block_info.hash, block_1.hash);

    // Create new block
    let block_hash_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7d")
            .unwrap();

    let new_block_1_again = BlockInfo {
        height: 1,
        hash: block_hash_1,
        prev_hash: block_hash_2,
        txs: vec![tx],
    };

    //3) Insert new_block_1_again and check get_tx_info, transaction tx_id should exist again an not be orphan anymore. It was included in a new block at same height
    indexer_store.save_new_best_block(&new_block_1_again)?;
    let tx_info = indexer_store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx.compute_txid(), tx_id);
    assert_eq!(tx_info.block_info.height, new_block_1_again.height);
    assert_eq!(tx_info.block_info.orphan, false);
    assert_eq!(tx_info.block_info.hash, new_block_1_again.hash);

    clear_output();

    Ok(())
}
