use bitcoin::key::rand;
use bitcoin::{
    key::rand::rngs::OsRng,
    secp256k1::{self, PublicKey as SecpPublicKey, SecretKey},
    PublicKey,
};
use bitcoin_indexer::store::IndexerStore;
use std::rc::Rc;
use storage_backend::{storage::Storage, storage_config::StorageConfig};
use std::net::TcpListener;
use std::time::{Duration, Instant};

pub fn generate_random_string() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..10).map(|_| rng.gen_range('a'..='z')).collect()
}

/// Wait for port 18443 to become available after Docker cleanup
/// Returns true if port is available, false if timeout reached
pub fn wait_for_port_available(timeout_secs: u64) -> bool {
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);
    
    while start.elapsed() < timeout {
        // Try to bind to the port - if successful, it's available
        if TcpListener::bind("127.0.0.1:18443").is_ok() {
            return true;
        }
        // Check every 100ms
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

pub fn get_indexer_store() -> Rc<IndexerStore> {
    // Build the path using PathBuf to handle separators correctly
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let absolute_path = current_dir
        .join("test_output")
        .join("get_best_block_height_test")
        .join(generate_random_string());
    
    // Create directory with retries for Windows file system consistency
    #[cfg(target_os = "windows")]
    {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 5;
        loop {
            match std::fs::create_dir_all(&absolute_path) {
                Ok(_) => break,
                Err(e) if attempts < MAX_ATTEMPTS - 1 => {
                    attempts += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => panic!("Failed to create directory after {} attempts: {}", MAX_ATTEMPTS, e),
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    std::fs::create_dir_all(&absolute_path).expect("Failed to create directory");
    
    // Convert to string and normalize to forward slashes for RocksDB
    let path_str = absolute_path.to_string_lossy().replace('\\', "/");
    
    let config = StorageConfig::new(path_str.clone(), None);
    
    // Create Storage with retries for Windows file system consistency
    #[cfg(target_os = "windows")]
    {
        let mut attempts = 0;
        const MAX_ATTEMPTS: u32 = 3;
        loop {
            match Storage::new(&config) {
                Ok(store) => {
                    let indexer_store = IndexerStore::new(Rc::new(store)).unwrap();
                    return Rc::new(indexer_store);
                }
                Err(e) if attempts < MAX_ATTEMPTS - 1 => {
                    attempts += 1;
                    eprintln!("Attempt {} failed to create Storage at {}: {}. Retrying...", attempts, path_str, e);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => panic!("Failed to create Storage after {} attempts at {}: {}", MAX_ATTEMPTS, path_str, e),
            }
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        let store = Rc::new(Storage::new(&config).unwrap());
        let indexer_store = IndexerStore::new(store).unwrap();
        Rc::new(indexer_store)
    }
}

pub fn clear_output() {
    // Try once, retry once on Windows-like systems with brief delay
    match std::fs::remove_dir_all("test_output") {
        Ok(_) => {},
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = std::fs::remove_dir_all("test_output");
        }
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
