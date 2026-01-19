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
    // Build the path using PathBuf to handle separators correctly
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let absolute_path = current_dir
        .join("test_output")
        .join("get_best_block_height_test")
        .join(generate_random_string());
    
    // On Windows, directory operations can be async, so retry if needed
    #[cfg(target_os = "windows")]
    {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;
        loop {
            match std::fs::create_dir_all(&absolute_path) {
                Ok(_) => break,
                Err(e) if attempts < MAX_ATTEMPTS => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if attempts == MAX_ATTEMPTS {
                        panic!("Failed to create directory after {} attempts: {}", MAX_ATTEMPTS, e);
                    }
                }
                Err(e) => panic!("Failed to create directory: {}", e),
            }
        }
        // Add delay to ensure directory is fully created and accessible
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    
    #[cfg(not(target_os = "windows"))]
    std::fs::create_dir_all(&absolute_path).expect("Failed to create directory");
    
    // Convert to string and normalize to forward slashes for RocksDB
    let path_str = absolute_path.to_string_lossy().replace('\\', "/");
    
    let config = StorageConfig::new(path_str, None);
    let store = Rc::new(Storage::new(&config).unwrap());
    let indexer_store = IndexerStore::new(store).unwrap();

    Rc::new(indexer_store)
}

pub fn clear_output() {
    // On Windows, add retry logic for directory removal
    #[cfg(target_os = "windows")]
    {
        if let Err(_) = std::fs::remove_dir_all("test_output") {
            // If first attempt fails, wait and retry
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = std::fs::remove_dir_all("test_output");
        }
        // Give Windows time to complete the deletion
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::fs::remove_dir_all("test_output");
    }
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
