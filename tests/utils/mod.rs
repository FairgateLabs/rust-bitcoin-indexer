use bitcoin::key::rand;
use bitcoin_indexer::store::IndexerStore;
use std::rc::Rc;
use storage_backend::{storage::Storage, storage_config::StorageConfig};

pub fn generate_random_string() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..10).map(|_| rng.gen_range('a'..='z')).collect()
}

pub fn get_indexer_store() -> IndexerStore {
    let path = format!(
        "test_output/get_best_block_height_test/{}",
        generate_random_string()
    );
    let config = StorageConfig::new(path, None);
    let store = Rc::new(Storage::new(&config).unwrap());
    let indexer_store = IndexerStore::new(store).unwrap();

    indexer_store
}

pub fn clear_output() {
    let _ = std::fs::remove_dir_all("test_output");
}
