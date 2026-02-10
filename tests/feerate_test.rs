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

/// Tests fee rate estimation with 7 transactions having different fee rates.
/// Verifies that the median fee rate is correctly calculated.
#[test]
fn test_get_estimated_fee_rate_with_seven_transactions() -> Result<(), anyhow::Error> {
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
    
    let secp = Secp256k1::new();
    
    // Step 1: Create 7 UTXOs with known private keys
    let mut utxo_data = Vec::new();
    
    for i in 0..7 {
        let secret_key = SecretKey::new(&mut OsRng);
        let private_key = PrivateKey::new(secret_key, Network::Regtest);
        let secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &secret_key);
        let public_key = PublicKey {
            compressed: true,
            inner: secp_pubkey,
        };
        
        let address = bitcoin_client.get_new_address(public_key.clone(), Network::Regtest)?;
        
        // Fund with 100,000 sats
        let (funding_tx, vout) = bitcoin_client.fund_address(&address, Amount::from_sat(100_000))?;
        let txid = funding_tx.compute_txid();
        
        utxo_data.push((txid, vout, private_key, address.script_pubkey(), public_key));
        println!("  Created UTXO {}: {}:{}", i, txid, vout);
    }
    
    // Step 2: Create and sign 7 transactions with different fees
    // Fee rates: 10, 20, 30, 40, 50, 60, 70 sat/vB (median should be 40)
    let mut signed_txs = Vec::new();
    let fee_rates = vec![10, 20, 30, 40, 50, 60, 70]; // sat/vB
    
    for (i, (prev_txid, prev_vout, private_key, prev_script_pubkey, public_key)) in utxo_data.iter().enumerate() {
        let output_secret = SecretKey::new(&mut OsRng);
        let output_secp_pubkey = bitcoin::secp256k1::PublicKey::from_secret_key(&secp, &output_secret);
        let output_public_key = PublicKey {
            compressed: true,
            inner: output_secp_pubkey,
        };
        let output_address = bitcoin_client.get_new_address(output_public_key, Network::Regtest)?;
        
        let target_fee_rate = fee_rates[i];
        
        // First pass: Create and sign with a temporary fee to get actual vsize
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
                value: Amount::from_sat(99_000), // Temporary value
                script_pubkey: output_address.script_pubkey(),
            }],
        };
        
        // Sign to get actual size
        let sighash_type = EcdsaSighashType::All;
        let mut sighash_cache = SighashCache::new(&tx);
        
        let sighash = sighash_cache.p2wpkh_signature_hash(
            0,
            prev_script_pubkey,
            Amount::from_sat(100_000),
            sighash_type,
        )?;
        
        let msg = bitcoin::secp256k1::Message::from_digest(*sighash.as_byte_array());
        let signature = secp.sign_ecdsa(&msg, &private_key.inner);
        
        let mut sig_with_hashtype = signature.serialize_der().to_vec();
        sig_with_hashtype.push(sighash_type.to_u32() as u8);
        
        let sig_push_bytes = bitcoin::script::PushBytesBuf::try_from(sig_with_hashtype.clone())
            .expect("Signature should fit in PushBytes");
        
        tx.input[0].witness.push(sig_push_bytes.as_bytes());
        tx.input[0].witness.push(public_key.to_bytes());
        
        // Get actual vsize and calculate exact fee
        let actual_vsize = tx.vsize() as u64;
        let exact_fee = target_fee_rate * actual_vsize;
        
        // Second pass: Recreate transaction with exact fee and re-sign
        let mut final_tx = Transaction {
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
                value: Amount::from_sat(100_000 - exact_fee),
                script_pubkey: output_address.script_pubkey(),
            }],
        };
        
        // Re-sign with new output value (outputs are part of SegWit sighash)
        let mut final_sighash_cache = SighashCache::new(&final_tx);
        
        let final_sighash = final_sighash_cache.p2wpkh_signature_hash(
            0,
            prev_script_pubkey,
            Amount::from_sat(100_000),
            sighash_type,
        )?;
        
        let final_msg = bitcoin::secp256k1::Message::from_digest(*final_sighash.as_byte_array());
        let final_signature = secp.sign_ecdsa(&final_msg, &private_key.inner);
        
        let mut final_sig_with_hashtype = final_signature.serialize_der().to_vec();
        final_sig_with_hashtype.push(sighash_type.to_u32() as u8);
        
        let final_sig_push_bytes = bitcoin::script::PushBytesBuf::try_from(final_sig_with_hashtype)
            .expect("Signature should fit in PushBytes");
        
        final_tx.input[0].witness.push(final_sig_push_bytes.as_bytes());
        final_tx.input[0].witness.push(public_key.to_bytes());
        
        let final_vsize = final_tx.vsize() as u64;
        let final_fee_rate = exact_fee / final_vsize;
        
        println!("  Transaction {}: fee={} sat, vsize={}, rate={} sat/vB (target: {})", 
                 i, exact_fee, final_vsize, final_fee_rate, target_fee_rate);
        
        // Verify exact fee rate
        assert_eq!(final_fee_rate, target_fee_rate, 
                   "Transaction {} should have exact fee rate {}", i, target_fee_rate);
        
        signed_txs.push(final_tx);
    }
    
    // Step 3: Broadcast all transactions
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
    
    // Step 4: Mine one block to include all transactions
    bitcoin_client.mine_blocks_to_address(1, &wallet)?;
    
    // Create indexer and process the block
    let store = get_indexer_store();
    let indexer = Indexer::new(
        bitcoin_client,
        store,
        Some(IndexerSettings::new(Some(100))),
    )?;
    
    // Sync to the latest block
    for _ in 0..20 {
        indexer.tick()?;
    }
    
    let best_block = indexer.get_best_block()?;
    assert!(best_block.is_some(), "Indexer should have processed blocks");
    
    let block = best_block.unwrap();
    
    // Verify the block has 8 transactions (1 coinbase + 7 spending)
    assert_eq!(block.txs.len(), 8, "Block should have 8 transactions (1 coinbase + 7 spending)");
    assert!(block.txs[0].is_coinbase(), "First transaction should be coinbase");
    
    // The median of fee rates [10, 20, 30, 40, 50, 60, 70] should be exactly 40
    assert_eq!(block.estimated_fee_rate, 40, 
            "Median fee rate should be exactly 40 sat/vB, got {}", 
            block.estimated_fee_rate);
    
    // Test the get_estimated_fee_rate API method
    let estimated_fee_rate = indexer.get_estimated_fee_rate()?;
    assert_eq!(estimated_fee_rate, 40, "get_estimated_fee_rate should return exactly 40 sat/vB");
    assert_eq!(estimated_fee_rate, block.estimated_fee_rate);
    
    bitcoind.stop()?;
    clear_output();
    assert!(wait_for_port_available(5), "Port 18443 should be available after container stop");
    Ok(())
}

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

