use bitcoin::hashes::Hash;
use bitcoin_indexer::{
    config::{IndexerConfig, IndexerSettings},
    indexer::{Indexer, IndexerApi},
};
use bitvmx_bitcoin_rpc::bitcoin_client::{BitcoinClient, BitcoinClientApi};
use bitcoind::{bitcoind::Bitcoind, config::BitcoindConfig};
use bitvmx_settings::settings;
mod utils;
use crate::utils::{clear_output, wait_for_port_available, get_indexer_store};

#[test]
fn test_get_estimated_fee_rate_indexer_not_synced() -> Result<(), anyhow::Error> {
    use bitcoin_indexer::errors::IndexerError;
    clear_output();

    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine blocks to height 101
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;
    
    // Create a second client to mine more blocks later
    let bitcoin_client_2 = BitcoinClient::new_from_config(&config.bitcoin)?;

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Process the block so the indexer is at height 100
    indexer.tick()?;

    // Mine one more block so blockchain is ahead
    bitcoin_client_2.mine_blocks_to_address(1, &wallet)?;

    // Try to get estimated fee rate when indexer is not synced (indexer at 100, blockchain at 102)
    let result = indexer.get_estimated_fee_rate();

    // Should return IndexerError::IndexerNotSynced
    assert!(result.is_err());
    match result.unwrap_err() {
        IndexerError::IndexerNotSynced => {
            // This is expected - test passes
        }
        other_error => {
            panic!(
                "Expected IndexerError::IndexerNotSynced, but got: {:?}",
                other_error
            );
        }
    }

    bitcoind.stop()?;
    clear_output();
    assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    Ok(())
}

#[test]
fn test_get_estimated_fee_rate_not_estimated() -> Result<(), anyhow::Error> {
    use bitcoin_indexer::errors::IndexerError;
    clear_output();

    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    let store = get_indexer_store();

    // Mine just 101 blocks - only coinbase transactions, no user transactions
    // This will cause blocks to have estimated_fee_rate = 0 because there are too few transactions
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;

    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Process the block
    indexer.tick()?;

    // Verify the block was saved with estimated_fee_rate = 0
    // (because blocks with only coinbase have too few transactions for fee estimation)
    let best_block = indexer.get_best_block()?;
    assert_eq!(best_block.unwrap().estimated_fee_rate, 0);

    // Now try to get estimated fee rate - should return IndexerError::FeeRateNotEstimated
    // because the estimated_fee_rate is 0
    let result = indexer.get_estimated_fee_rate();

    // Should return IndexerError::FeeRateNotEstimated
    assert!(result.is_err());
    match result.unwrap_err() {
        IndexerError::FeeRateNotEstimated => {
            // This is expected - test passes
        }
        other_error => {
            panic!(
                "Expected IndexerError::FeeRateNotEstimated, but got: {:?}",
                other_error
            );
        }
    }

    bitcoind.stop()?;
    clear_output();
    assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    Ok(())
}

#[test]
fn test_get_estimated_fee_rate_with_multiple_transactions() -> Result<(), anyhow::Error> {
    use bitcoin::PublicKey;
    use bitcoin::key::rand::rngs::OsRng;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::Network;

    fn get_random_pubkey() -> PublicKey {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::new(&mut OsRng);
        let public_key = secret_key.public_key(&secp);
        PublicKey::new(public_key)
    }

    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;

    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;

    // Mine blocks to have mature coins (coinbase needs 100 confirmations)
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;

    // Create additional blocks to test fee rate estimation
    // Each block will have only a coinbase transaction in regtest
    for _ in 0..10 {
        let pubkey = get_random_pubkey();
        let new_address = bitcoin_client.get_new_address(pubkey, Network::Regtest)?;
        // Each mine_blocks_to_address creates a block with coinbase only
        bitcoin_client.mine_blocks_to_address(1, &new_address)?;
    }

    // Mine one final block
    bitcoin_client.mine_blocks_to_address(1, &wallet)?;

    // Create indexer and process all blocks
    // This will trigger estimate_fee_rate internally for each block
    let store = get_indexer_store();
    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(100))),
    )?;

    // Process all 112 blocks (101 + 10 + 1)
    for _ in 0..13 {
        indexer.tick()?;
    }

    // Get the best block to verify processing
    let best_block = indexer.get_best_block()?;
    assert!(best_block.is_some());
    
    let best_block = best_block.unwrap();
    // The block was processed and estimate_fee_rate was called internally
    // Fee rate is 0 because regtest blocks only have coinbase (< MIN_BLOCK_TX=5)
    assert_eq!(best_block.height, 112);
    assert_eq!(best_block.estimated_fee_rate, 0, "Fee rate should be 0 for blocks with only coinbase");

    bitcoind.stop()?;
    clear_output();
    assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    Ok(())
}

#[test]
fn test_estimate_fee_rate_with_multiple_signed_transactions() -> Result<(), anyhow::Error> {
    use bitcoin::key::rand::rngs::OsRng;
    use bitcoin::secp256k1::{Secp256k1, SecretKey};
    use bitcoin::Amount;
    use bitcoin::transaction::{Transaction, TxIn, TxOut};
    use bitcoin::absolute::LockTime;
    use bitcoin::transaction::Version;
    use bitcoin::Sequence;
    use bitcoin::OutPoint;
    use bitcoin::Witness;
    use bitcoin::PrivateKey;
    use bitcoin::sighash::{SighashCache, EcdsaSighashType};
    use bitcoin::Network;
    use bitcoin::PublicKey;
    
    clear_output();
    
    let config = settings::load::<IndexerConfig>()?;
    let bitcoind_config = BitcoindConfig::default();
    let bitcoind = Bitcoind::new(
        bitcoind_config,
        config.bitcoin.clone(),
        None,
    );
    bitcoind.start()?;
    
    let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;
    let wallet = bitcoin_client.init_wallet("test_wallet")?;
    
    // Mine blocks to have mature coins
    bitcoin_client.mine_blocks_to_address(101, &wallet)?;
    
    println!("=== Creating block with 7+ manually signed transactions ===");
    
    let secp = Secp256k1::new();
    
    // Step 1: Create UTXOs with known private keys
    println!("\nStep 1: Creating UTXOs with known private keys");
    let mut utxo_data = Vec::new();
    
    for i in 0..7 {
        // Generate private key
        let secret_key = SecretKey::new(&mut OsRng);
        let private_key = PrivateKey::new(secret_key, Network::Regtest);
        let secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
        let public_key = PublicKey {
            compressed: true,
            inner: secp_pubkey,
        };
        
        // Create P2PKH address
        let address = bitcoin_client.get_new_address(public_key.clone(), Network::Regtest)?;
        
        // Fund the address (this will auto-mine)
        let (funding_tx, vout) = bitcoin_client.fund_address(&address, Amount::from_sat(100_000))?;
        let txid = funding_tx.compute_txid();
        
        utxo_data.push((txid, vout, private_key, address.script_pubkey(), public_key));
        println!("  Created UTXO {}: {}:{}", i, txid, vout);
    }
    
    println!("\nStep 2: Creating and signing spending transactions");
    let mut signed_txs = Vec::new();
    
    for (i, (prev_txid, prev_vout, private_key, prev_script_pubkey, public_key)) in utxo_data.iter().enumerate() {
        // Create output address
        let output_secret = SecretKey::new(&mut OsRng);
        let output_secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &output_secret);
        let output_public_key = PublicKey {
            compressed: true,
            inner: output_secp_pubkey,
        };
        let output_address = bitcoin_client.get_new_address(output_public_key, Network::Regtest)?;
        
        // Build unsigned transaction
        let mut tx = Transaction {
            version: Version::TWO,
            lock_time: LockTime::ZERO,
            input: vec![TxIn {
                previous_output: OutPoint {
                    txid: *prev_txid,
                    vout: *prev_vout,
                },
                script_sig: bitcoin::ScriptBuf::new(),
                sequence: Sequence::MAX,
                witness: Witness::new(),
            }],
            output: vec![TxOut {
                value: Amount::from_sat(95_000), // 5000 sat fee
                script_pubkey: output_address.script_pubkey(),
            }],
        };
        
        // Sign the transaction (SegWit style)
        let sighash_type = EcdsaSighashType::All;
        let mut sighash_cache = SighashCache::new(&tx);
        
        // Use p2wpkh_signature_hash for SegWit P2WPKH
        let sighash = sighash_cache.p2wpkh_signature_hash(
            0,
            prev_script_pubkey,
            Amount::from_sat(100000), // The value of the UTXO being spent
            sighash_type,
        )?;
        
        let msg = bitcoin::secp256k1::Message::from_digest(*sighash.as_byte_array());
        let signature = secp.sign_ecdsa(&msg, &private_key.inner);
        
        // Build witness (not script_sig for SegWit)
        let mut sig_with_hashtype = signature.serialize_der().to_vec();
        sig_with_hashtype.push(sighash_type.to_u32() as u8);
        
        let sig_push_bytes = bitcoin::script::PushBytesBuf::try_from(sig_with_hashtype)
            .expect("Signature should fit in PushBytes");
        
        // For P2WPKH, witness is [signature, pubkey]
        tx.input[0].witness.push(sig_push_bytes.as_bytes());
        tx.input[0].witness.push(public_key.to_bytes());
        
        signed_txs.push(tx);
        println!("  Signed transaction {}", i);
    }
    
    println!("\nStep 3: Broadcasting all transactions to mempool");
    for (i, tx) in signed_txs.iter().enumerate() {
        match bitcoin_client.send_transaction(tx) {
            Ok(_) => println!("  ✓ Broadcasted transaction {}", i),
            Err(e) => {
                println!("  ✗ Failed to broadcast transaction {}: {}", i, e);
                bitcoind.stop()?;
                clear_output();
                return Err(e.into());
            }
        }
    }
    
    println!("\nStep 4: Mining one block to include all transactions");
    bitcoin_client.mine_blocks_to_address(1, &wallet)?;
    
    // Create indexer and process the block
    let store = get_indexer_store();
    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(100))),
    )?;
    
    // Sync to the latest block - tick until we process all available blocks
    for _ in 0..20 {
        indexer.tick()?;
    }
    
    let best_block = indexer.get_best_block()?;
    assert!(best_block.is_some(), "Indexer should have processed blocks");
    
    let block = best_block.unwrap();
    
    println!("\nRESULTS:");
    println!("Block at height {} has {} transactions", block.height, block.txs.len());
    println!("Calculated fee rate: {} sat/vB", block.estimated_fee_rate);
    
    // Verify success
    assert!(block.txs.len() > 5, "Block should have more than 5 transactions, got {}", block.txs.len());
    assert!(block.txs[0].is_coinbase(), "First transaction should be coinbase");
    assert!(block.estimated_fee_rate > 0, "Fee rate should be > 0 for block with {} transactions", block.txs.len());
    
    println!("\n✅ SUCCESS: Created block with {} transactions", block.txs.len());
    println!("✅ Fee rate is {} sat/vB (non-zero as expected)", block.estimated_fee_rate);
    
    bitcoind.stop()?;
    clear_output();
    assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    Ok(())
}
