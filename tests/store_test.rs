use std::str::FromStr;

use bitcoin::{key::rand, BlockHash, Txid};
use bitcoin_indexer::{
    store::{Store, StoreClient},
    types::BlockInfo,
};

fn generate_random_string() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..10).map(|_| rng.gen_range('a'..='z')).collect()
}

#[test]
fn get_best_block_height_test() -> Result<(), anyhow::Error> {
    //This is not a test, is just a way to call methods easily.
    let path = format!(
        "test_output/get_best_block_height_test/{}",
        generate_random_string()
    );
    let store = Store::new(&path)?;
    let height = store.get_best_block_height()?;
    assert_eq!(height, None);

    let block_1 = BlockInfo {
        height: 1,
        hash: BlockHash::from_str(
            "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f",
        )
        .unwrap(),
        txs: vec![],
    };

    //Insert block at height 1 and check get_best_block_height
    store.save_block(&block_1)?;
    let height = store.get_best_block_height()?;
    assert_eq!(height, Some(1));

    let block_2 = BlockInfo {
        height: 2,
        hash: BlockHash::from_str(
            "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f",
        )
        .unwrap(),
        txs: vec![],
    };

    //Insert block at height 2 and check get_best_block_height
    store.save_block(&block_2)?;
    let height = store.get_best_block_height()?;
    assert_eq!(height, Some(2));

    //Insert block at height 1 again and check get_best_block_height

    store.save_block(&block_1)?;
    let height = store.get_best_block_height()?;
    assert_eq!(height, Some(2));

    Ok(())
}

#[test]
fn save_block_test() -> Result<(), anyhow::Error> {
    //This is not a test, is just a way to call methods easily.
    let path = format!("test_output/save_block_test/{}", generate_random_string());
    let store = Store::new(&path)?;

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
    store.save_block(&block_1)?;
    let saved_block_1 = store.get_block_by_hash(&block_hash_1)?.unwrap();
    assert_eq!(saved_block_1.hash, block_1.hash);
    assert_eq!(saved_block_1.height, block_1.height);
    assert_eq!(saved_block_1.orphan, false);
    assert_eq!(saved_block_1.prev_hash, block_1.prev_hash);

    let block_hash = store.get_block_hash_by_height(1)?.unwrap();
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
    store.save_block(&new_block_1)?;
    let saved_new_block_1 = store.get_block_by_hash(&block_hash_new_1)?.unwrap();
    assert_eq!(saved_new_block_1.hash, new_block_1.hash);
    assert_eq!(saved_new_block_1.height, new_block_1.height);
    assert_eq!(saved_new_block_1.orphan, false);
    assert_eq!(saved_new_block_1.prev_hash, new_block_1.prev_hash);

    let block_hash = store.get_block_hash_by_height(1)?.unwrap();
    assert_eq!(block_hash, new_block_1.hash);

    //Check data block for the previous block at height 1
    let saved_block_1 = store.get_block_by_hash(&block_hash_1)?.unwrap();
    assert_eq!(saved_block_1.hash, block_1.hash);
    assert_eq!(saved_block_1.height, block_1.height);
    assert_eq!(saved_block_1.orphan, true);
    assert_eq!(saved_block_1.prev_hash, block_1.prev_hash);

    let block_hash = store.get_block_hash_by_height(1)?.unwrap();
    assert_eq!(block_hash, new_block_1.hash);

    Ok(())
}

#[test]
fn get_tx_info_test() -> Result<(), anyhow::Error> {
    let path = format!("test_output/get_tx_info_test/{}", generate_random_string());
    let store = Store::new(&path)?;

    let block_hash_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7a")
            .unwrap();

    let block_hash_2: BlockHash =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7b")
            .unwrap();

    let tx_id =
        Txid::from_str("91c1acedb27109016bb3a177372cdbb5f8f9d9c32fd4c2506ebb564ac0a61eaf").unwrap();

    let block_1 = BlockInfo {
        height: 1,
        hash: block_hash_1,
        prev_hash: block_hash_2,
        txs: vec![tx_id],
    };

    //1) Save block_1 and check get_tx_info method, transaction with tx_id should exist
    store.save_block(&block_1)?;
    let tx_info = store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx_id, tx_id);
    assert_eq!(tx_info.block_height, block_1.height);
    assert_eq!(tx_info.orphan, false);
    assert_eq!(tx_info.block_hash, block_1.hash);

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
    store.save_block(&new_block_1)?;
    let tx_info = store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx_id, tx_id);
    assert_eq!(tx_info.block_height, block_1.height);
    assert_eq!(tx_info.orphan, true);
    assert_eq!(tx_info.block_hash, block_1.hash);

    // Create new block
    let block_hash_1 =
        BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7d")
            .unwrap();

    let new_block_1_again = BlockInfo {
        height: 1,
        hash: block_hash_1,
        prev_hash: block_hash_2,
        txs: vec![tx_id],
    };

    //3) Insert new_block_1_again and check get_tx_info, transaction tx_id should exist again an not be orphan anymore. It was included in a new block at same height
    store.save_block(&new_block_1_again)?;
    let tx_info = store.get_tx_info(&tx_id)?.unwrap();
    assert_eq!(tx_info.tx_id, tx_id);
    assert_eq!(tx_info.block_height, new_block_1_again.height);
    assert_eq!(tx_info.orphan, false);
    assert_eq!(tx_info.block_hash, new_block_1_again.hash);

    Ok(())
}
