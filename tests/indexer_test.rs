use std::str::FromStr;

use anyhow::Ok;
use bitcoin::{absolute::LockTime, transaction::Version, BlockHash, Transaction}; //, Txid};
use bitcoin_indexer::{
    bitcoin_client::MockBitcoinClient,
    indexer::{Indexer, IndexerApi},
    store::MockStore,
    types::{BlockInfo, FullBlock},
};
use mockall::predicate::eq;

#[test]
fn reorg_1_block() -> Result<(), anyhow::Error> {
    let mut bitcoin_client = MockBitcoinClient::new();
    let mut store = MockStore::new();

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

    let block_1001_reorg = BlockInfo {
        height: 1001,
        hash: hash_1001_reorg,
        prev_hash: hash_1000,
        txs: vec![],
    };

    let block_1000_copy = block_1000.clone();
    let block_1001_copy = block_1001.clone();
    let block_1001_reorg_copy = block_1001_reorg.clone();

    bitcoin_client
        .expect_get_best_block()
        .times(4)
        .returning(|| Ok(10000000));

    //Detecting block 1000:
    bitcoin_client
        .expect_get_block_by_height()
        .times(1)
        .with(eq(1000))
        .returning(move |_| Ok(Some(block_1000.clone())));

    store
        .expect_get_block_hash_by_height()
        .with(eq(999))
        .times(1)
        .returning(move |_| Ok(Some(prev_hash_1000.clone())));

    store
        .expect_save_block()
        .with(eq(block_1000_copy.clone()))
        .times(1)
        .returning(move |_| Ok(()));

    //Detecting block 1001:
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1001))
        .times(1)
        .returning(move |_| Ok(Some(block_1001.clone())));

    store
        .expect_get_block_hash_by_height()
        .with(eq(1000))
        .times(1)
        .returning(move |_| Ok(Some(hash_1000.clone())));

    store
        .expect_save_block()
        .with(eq(block_1001_copy))
        .times(1)
        .returning(move |_| Ok(()));

    //Detecting block 1002 and decrease one block:
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1002))
        .times(1)
        .returning({
            let block_1002 = block_1002.clone();
            move |_| Ok(Some(block_1002.clone()))
        });

    store
        .expect_get_block_hash_by_height()
        .with(eq(1001))
        .times(1)
        .returning(move |_| Ok(Some(hash_1001.clone())));

    store.expect_save_block().never();

    //Going black to block 1001:
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1001))
        .times(1)
        .returning(move |_| Ok(Some(block_1001_reorg.clone())));

    store
        .expect_get_block_hash_by_height()
        .with(eq(1000))
        .times(1)
        .returning(move |_| Ok(Some(hash_1000.clone())));

    store
        .expect_save_block()
        .with(eq(block_1001_reorg_copy))
        .times(1)
        .returning(move |_| Ok(()));

    let height_to_sync = 1000;
    let indexer = Indexer::new(bitcoin_client, store)?;

    // Firt iteration should detect block 1000 and increment height_to_sync to 1001
    let next_index = indexer.tick(&height_to_sync)?;
    assert_eq!(next_index, 1001);

    // Second iteration should detect block 1001 and increment height_to_sync to 1002
    let next_index = indexer.tick(&next_index)?;
    assert_eq!(next_index, 1002);

    // Third iteration should detect block 1002 and decrease height_to_sync to 1001
    let next_index = indexer.tick(&next_index)?;
    assert_eq!(next_index, 1001);

    // Fourth iteration should detect block 1002 and increase height_to_sync to 1002
    let next_index = indexer.tick(&next_index)?;
    assert_eq!(next_index, 1002);

    Ok(())
}

#[test]
fn test_get_best_block() -> Result<(), anyhow::Error> {
    let bitcoin_client = MockBitcoinClient::new();
    let mut store = MockStore::new();

    let hash = BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash = BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;
    let full_block = FullBlock {
        height: 1000,
        hash: hash.clone(),
        orphan: false,
        prev_hash: prev_hash.clone(),
        txs: vec![],
    };

    let full_block_clone = full_block.clone();
    store
        .expect_get_best_block()
        .times(1)
        .returning(move || Ok(Some(full_block_clone.clone())));

    let indexer = Indexer::new(bitcoin_client, store)?;
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block, Some(full_block));

    Ok(())
}

#[test]
fn test_index_height_block_not_exists() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockStore::new();

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(1000));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1000))
        .returning(move |_| Ok(None)); // Simula que el bloque en la altura 1000 no existe

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&1000);
    assert_eq!(result.unwrap(), 1000);
}

#[test]
fn test_blockchain_height_lower_than_index_height() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockStore::new();

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(500)); // Blockchain height is lower

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&1000);
    assert_eq!(result.unwrap(), 1000);
}

#[test]
fn test_block_hash_mismatch() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let mut store = MockStore::new();

    let block_info = BlockInfo {
        height: 1,
        hash: BlockHash::from_str("12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d").unwrap(),
        prev_hash: BlockHash::from_str("ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff").unwrap(),
        txs: vec![],
    };

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(1));

    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(&1))
        .returning(move |_| Ok(Some(block_info.clone())));

    store
        .expect_get_block_hash_by_height()
        .with(eq(0))
        .returning(move |_| Ok(Some(BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap())));

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&1);
    assert_eq!(result.unwrap(), 1000);
}

#[test]
fn test_blockchain_height_less_than_index_height() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockStore::new();

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(999)); // Blockchain height is lower than height_to_index (1000)

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&1000);
    assert_eq!(result.unwrap(), 1000);
}

#[test]
fn test_index_height_empty_blockchain() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockStore::new();
    
    // Mock get_best_block to return 0, simulating an empty blockchain
    bitcoin_client
        .expect_get_best_block()
        .returning(|| Ok(0));
    
    // Mock get_block_by_height to return None since no blocks exist
    bitcoin_client
        .expect_get_block_by_height()
        .with(eq(1))
        .returning(|_| Ok(None));
    
    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&1);

    assert!(result.is_err(), "Error expected while trying to index in an empty blockchain");
}

use std::sync::Arc;
#[test]
fn test_index_height_invalid_block() -> Result<(), anyhow::Error> {
    let mut bitcoin_client = MockBitcoinClient::new();
    let mut store = MockStore::new();

    let prev_hash_103 = 
        BlockHash::from_str("2bec48d30f0dd43d00a90dfd2de68a3ec5b8d9213ad9471c2945a5498d0c0697")?;
    
    let invalid_hash = BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000000")?;
    let tx = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };
    let block_103 = BlockInfo {
        hash: invalid_hash,
        prev_hash: prev_hash_103,
        height: 103,
        txs: vec![tx],
    };

    let block_103_for_client = Arc::new(block_103.clone());
    let block_103_for_store = block_103.clone();

    bitcoin_client.expect_get_best_block()
        .returning(|| Ok(103));

    bitcoin_client.expect_get_block_by_height()
        .with(eq(103))
        .returning(move |_| Ok(Some((*block_103_for_client).clone())));

    store.expect_get_block_hash_by_height()
        .with(eq(102))
        .returning(move |_| Ok(Some(prev_hash_103)));

    store.expect_save_block()
        .with(eq(block_103_for_store))
        .returning(|_| Ok(()));

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&103);

    assert!(result.is_err(), "Expected error while processing an invalid block");

    Ok(()) // Devuelve un resultado exitoso si la prueba pasa
}

#[test]
fn test_index_height_upper_limit_reached() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockStore::new();
    bitcoin_client.expect_get_best_block()
        .returning(|| Ok(500_000));

    bitcoin_client.expect_get_block_by_height()
        .returning(|height| {
            if *height > 500_000 { 
                Ok(None) 
            } else { 
                Ok(Some(BlockInfo { 
                    height: *height, 
                    hash: BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap(), 
                    prev_hash: BlockHash::from_str("0000000000000000000000000000000000000000000000000000000000000000").unwrap(), 
                    txs: vec![],
                })) 
            }
        });

    let indexer = Indexer::new(bitcoin_client, store).unwrap();
    let result = indexer.tick(&500_101);

    assert!(result.is_err());
}
