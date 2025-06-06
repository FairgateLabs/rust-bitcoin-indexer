use crate::utils::{clear_output, get_indexer_store};
use bitcoin::{absolute::LockTime, transaction::Version, BlockHash, Transaction};
use bitcoin_indexer::indexer::{Indexer, IndexerApi};
use bitvmx_bitcoin_rpc::{bitcoin_client::MockBitcoinClient, types::*};
use mockall::predicate::eq;
use std::str::FromStr;
mod utils;

#[test]
fn reorganization_test() -> Result<(), anyhow::Error> {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = get_indexer_store();
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };

    // Reorg 1 block, block_1002 prev hash is different than block_1001 hash
    // Then get again block_1001 and this is different, check that block_1001 prev hash is correct
    // Thne get again block_1002

    // block_1000 -> block_1001 -- reorg -- block_1002 -> block_1001 -> block_1002

    let hash_1000 =
        BlockHash::from_str("12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d")?;

    let hash_1001 =
        BlockHash::from_str("e987bd2b973073b86b83901b03f6d16711452ab634cd8b2f3915e22cdcfa39b2")?;

    let hash_1002 =
        BlockHash::from_str("3c4389fd5a12aa686b546bf5ab2168e6149e21a6a20fcf9272ebc541bd2eed67")?;

    let prev_hash_1000 =
        BlockHash::from_str("4c136e0b24dc517809eabb6b6e6d5ec8f0087a49356be1f2de485d45ab26d2e3")?;

    let hash_1001_reorg =
        BlockHash::from_str("3aee099f9f5102e52767d9289b7a628e61d911d2f74f42c8835006c45d331713")?;

    let block_1000 = BlockInfo {
        height: 1000,
        hash: hash_1000,
        prev_hash: prev_hash_1000,
        txs: vec![tx],
    };

    let block_1001 = BlockInfo {
        height: 1001,
        hash: hash_1001,
        prev_hash: hash_1000,
        txs: vec![],
    };

    let block_1002 = BlockInfo {
        height: 1002,
        hash: hash_1002,
        prev_hash: hash_1001_reorg,
        txs: vec![],
    };

    let block_1002_copy = block_1002.clone();

    let block_1001_reorg = BlockInfo {
        height: 1001,
        hash: hash_1001_reorg,
        prev_hash: hash_1000,
        txs: vec![],
    };

    // Mock bitcoin client new >>>>>>>
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1000));

    //Detecting block 1000:
    bitcoin_client
        .expect_get_block_by_height()
        .times(1)
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_1000.clone())));

    // FIRST TICK >>>>>>>
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1001));

    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1001));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1001))
        .times(1)
        .returning(move |_| Ok(Some(block_1001.clone())));

    // SECOND TICK >>>>>>>
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1002));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1002))
        .times(1)
        .returning(move |_| Ok(Some(block_1002.clone())));

    // THIRD TICK >>>>>>>
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1002));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1001))
        .times(1)
        .returning(move |_| Ok(Some(block_1001_reorg.clone())));

    // FOURTH TICK >>>>>>>
    bitcoin_client
        .expect_get_best_block()
        .times(1)
        .returning(|| Ok(1002));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1002))
        .times(1)
        .returning(move |_| Ok(Some(block_1002_copy.clone())));

    let indexer = Indexer::new(bitcoin_client, store, Some(1000))?;

    // Firt iteration should detect block 1000 and increment height_to_sync to 1001
    indexer.tick()?;
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(1000));

    // Second iteration should detect block 1001 and increment height_to_sync to 1002
    indexer.tick()?;
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(1001));

    // Third iteration should detect block 1002 and decrease height_to_sync to 1001
    indexer.tick()?;
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(1001));

    // Fourth iteration should detect block 1002 and increase height_to_sync to 1002
    indexer.tick()?;
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(1001));

    // Fifth iteration should detect block 1002 and increase height_to_sync to 1002
    indexer.tick()?;
    let best_height = indexer.get_best_height()?;
    assert_eq!(best_height, Some(1002));

    clear_output();

    Ok(())
}
