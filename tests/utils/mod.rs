use bitcoin::key::rand;
use bitcoin::{
    key::rand::rngs::OsRng,
    secp256k1::{self, PublicKey as SecpPublicKey, SecretKey},
    PublicKey,
};
use bitcoin_indexer::store::IndexerStore;
use std::rc::Rc;
use storage_backend::{storage::Storage, storage_config::StorageConfig};

pub fn generate_random_string() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..10).map(|_| rng.gen_range('a'..='z')).collect()
}

pub fn get_indexer_store() -> Rc<IndexerStore> {
    let path = format!(
        "test_output/get_best_block_height_test/{}",
        generate_random_string()
    );
    // Convert to absolute path for Windows compatibility
    let absolute_path = std::fs::canonicalize(".")
        .unwrap()
        .join(&path);
    // Create the full directory path before initializing Storage
    std::fs::create_dir_all(&absolute_path).unwrap();
    let config = StorageConfig::new(absolute_path.to_string_lossy().to_string(), None);
    let store = Rc::new(Storage::new(&config).unwrap());
    let indexer_store = IndexerStore::new(store).unwrap();

    Rc::new(indexer_store)
}

pub fn clear_output() {
    let _ = std::fs::remove_dir_all("test_output");
}

pub fn get_random_pubkey() -> PublicKey {
    let secp = secp256k1::Secp256k1::new();
    let mut rng = OsRng;
    let too_sk = SecretKey::new(&mut rng);
    let too_pk = SecpPublicKey::from_secret_key(&secp, &too_sk);
    PublicKey {
        compressed: true,
        inner: too_pk,
    }
}
