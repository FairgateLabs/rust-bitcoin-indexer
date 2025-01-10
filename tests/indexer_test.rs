use std::str::FromStr;

use bitcoin::{absolute::LockTime, transaction::Version, BlockHash, Transaction, Txid};
use bitcoin_indexer::{
    bitcoin_client::MockBitcoinClient,
    indexer::{Indexer, IndexerApi},
    store::MockIndexerStore,
    types::{BlockInfo, FullBlock, TransactionInfo},
};
use mockall::predicate::eq;

#[test]
fn reorg_1_block() -> Result<(), anyhow::Error> {
    let mut bitcoin_client = MockBitcoinClient::new();
    let mut store = MockIndexerStore::new();

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
    let indexer = Indexer::new(bitcoin_client, store);

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
    let mut store = MockIndexerStore::new();

    let hash =
        BlockHash::from_str("000000000000000000076c3e2e0f70537b1bf75268e502e0123b35a6207bf7e2")?;
    let prev_hash =
        BlockHash::from_str("000000000000000000045bc1e2ff2a08d10e3fae9b0a8b3536acb6f43adf1234")?;
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

    let indexer = Indexer::new(bitcoin_client, store);
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block, Some(full_block));

    Ok(())
}

#[test]
fn test_get_tx_existing_non_orphan() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let mut store = MockIndexerStore::new();

    let tx_id =
        Txid::from_str("4d3a5c31e5a25d27687a3ed3bb8a3f65e5fdccf39f476574f8a73d38a65f3a5d").unwrap();
    let block_hash =
        BlockHash::from_str("12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d")
            .unwrap();
    let transact = Transaction {
        version: Version::TWO,
        lock_time: LockTime::ZERO,
        input: vec![],
        output: vec![],
    };
    let tx_info = TransactionInfo {
        tx: transact,
        block_height: 500,
        block_hash: block_hash,
        orphan: false,
        confirmations: 0,
    };

    store
        .expect_get_tx_info()
        .with(eq(tx_id.clone()))
        .returning(move |_| Ok(Some(tx_info.clone())));

    let best_block = FullBlock {
        height: 505,
        hash: BlockHash::from_str(
            "12efaa3528db3845a859c470a525f1b8b4643b0d561f961ab395a9db778c204d",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "e3d2aa2c8211961e6a6f94740cd1e9be6a8e55534ed824f82a157dd2c51be5f2",
        )
        .unwrap(),
        orphan: false,
        txs: vec![],
    };

    store
        .expect_get_best_block()
        .returning(move || Ok(Some(best_block.clone())));

    bitcoin_client.expect_get_best_block().returning(|| Ok(505));

    let indexer = Indexer::new(bitcoin_client, store);

    let result = indexer.get_tx(&tx_id).unwrap();
    assert!(result.is_some());
    let tx_info = result.unwrap();
    assert_eq!(tx_info.confirmations, 6);
}

#[test]
fn test_get_tx_existing_orphan() {
    let bitcoin_client = MockBitcoinClient::new();
    let mut store = MockIndexerStore::new();

    let tx_id =
        Txid::from_str("4d3a5c31e5a25d27687a3ed3bb8a3f65e5fdccf39f476574f8a73d38a65f3a5d").unwrap();

    let tx_info = TransactionInfo {
        tx: Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![],
            output: vec![],
        },
        block_height: 0,
        block_hash: BlockHash::from_str(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        orphan: true,
        confirmations: 0,
    };

    store
        .expect_get_tx_info()
        .with(eq(tx_id.clone()))
        .returning(move |_| Ok(Some(tx_info.clone())));

    let best_block = FullBlock {
        height: 1000,
        hash: BlockHash::from_str(
            "0000000000000000000000000000000000000000000000000000000000000001",
        )
        .unwrap(),
        prev_hash: BlockHash::from_str(
            "0000000000000000000000000000000000000000000000000000000000000000",
        )
        .unwrap(),
        orphan: true,
        txs: vec![],
    };

    store
        .expect_get_best_block()
        .returning(move || Ok(Some(best_block.clone())));

    let indexer = Indexer::new(bitcoin_client, store);

    let result = indexer.get_tx(&tx_id).unwrap();
    assert!(result.is_some());

    let tx_info = result.unwrap();
    assert_eq!(tx_info.confirmations, 0);
}

#[test]
fn test_get_tx_nonexistent() {
    let bitcoin_client = MockBitcoinClient::new();
    let mut store = MockIndexerStore::new();

    let tx_id =
        Txid::from_str("4d3a5c31e5a25d27687a3ed3bb8a3f65e5fdccf39f476574f8a73d38a65f3a5d").unwrap();

    store
        .expect_get_tx_info()
        .with(eq(tx_id.clone()))
        .returning(move |_| Ok(None));

    let indexer = Indexer::new(bitcoin_client, store);

    let result = indexer.get_tx(&tx_id).unwrap();
    assert!(result.is_none());
}

#[test]
fn test_blockchain_height_lower_than_index_height() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockIndexerStore::new();

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(500)); // Blockchain height is lower

    let indexer = Indexer::new(bitcoin_client, store);
    let result = indexer.tick(&1000);
    assert_eq!(result.unwrap(), 1000);
}

#[test]
fn test_blockchain_height_less_than_index_height() {
    let mut bitcoin_client = MockBitcoinClient::new();
    let store = MockIndexerStore::new();

    bitcoin_client
        .expect_get_best_block()
        .returning(move || Ok(999));

    let indexer = Indexer::new(bitcoin_client, store);
    let result = indexer.tick(&1000);
    assert_eq!(result.unwrap(), 1000);
}
