use bitcoin_indexer::helper::define_height_to_sync;

#[test]
fn define_height_to_sync_test() -> Result<(), anyhow::Error> {
    // Tests:

    // Test
    // checkpoint_height: None | blockchain_height: 100 | indexed_height: None
    let start_height = define_height_to_sync(None, 100, None)?;
    // Then start_height should be height 0 (no checkpoint, no block indexed)
    assert_eq!(start_height, 0);

    // Test
    // checkpoint_height: None | blockchain_height: 100 | indexed_height: 40
    let start_height = define_height_to_sync(None, 100, Some(40))?;
    // Then start_height should be height 41 (indexed_height + 1 )
    assert_eq!(start_height, 41);

    // Test
    // checkpoint_height: 10000 | blockchain_height: 100 | indexed_height: None
    let start_height = define_height_to_sync(Some(10000), 100, None);
    // Checkpoint can not be bigger than blockchain_height
    assert!(start_height.is_err());

    // Test
    // checkpoint_height: 40 | blockchain_height: 100 | indexed_height: None
    let start_height = define_height_to_sync(Some(40), 100, None)?;
    // Then start_height should be height 40 (checkpoint_height should rule)
    assert_eq!(start_height, 40);

    // Test
    // checkpoint_height 100 | blockchain_height 100 | indexed_height 100
    let start_height = define_height_to_sync(Some(100), 100, Some(100))?;
    // Then start_height should be height 100 (checkpoint should rule)
    assert_eq!(start_height, 100);

    Ok(())
}
