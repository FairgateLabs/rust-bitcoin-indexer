# Bitcoin Indexer

## Overview

The Bitcoin Indexer is a Rust-based library designed to extract and store block and transaction IDs from the Bitcoin blockchain. This project provides a simple and efficient way to index the blockchain starting from a specific height and save the gathered data into a RockDB storage for further analysis and processing.

## ⚠️ Disclaimer

This library is currently under development and may not be fully stable.
It is not production-ready, has not been audited, and future updates may introduce breaking changes without preserving backward compatibility.

## Features

- 🏷️ **Block and Transaction Data**: Retrieves and records block and transaction IDs from the Bitcoin blockchain.
- 📏 **Configurable Start Height**: Allows indexing to begin from a specified block height.
- 💾 **File Storage**: Saves the extracted data into a RockDB storage for easy access and further use.
- ⏱️ **Synchronous Processing**: Processes each block and transaction synchronously, ensuring data consistency with each tick.

## Methods
The `IndexerApi` trait provides several methods to interact with the Bitcoin Indexer:

- **is_ready**: Checks if the indexer has indexed the entire blockchain and is at the latest block.

- **tick**: Processes the next block in the blockchain.

- **get_best_block**: Retrieves the best (most recent) block that has been indexed.

- **get_best_height**: Retrieves the height of the best (most recent) block that has been indexed.

- **get_blockchain_best_height**: Retrieves the current best block height from the Bitcoin blockchain.

- **get_block_by_height**: Retrieves a block by its height.

- **get_block_by_hash**: Retrieves a block by its hash.

- **get_transaction**: Retrieves transaction information for a given transaction ID. Returns a `TransactionInfo` with the transaction status. If the transaction is not found, returns `TransactionInfo` with `TransactionBlockchainStatus::NotFound`.

- **get_estimated_fee_rate**: Retrieves the estimated fee rate from the most recently indexed block.

## Usage
```rust
  // Load configuration
  let config = ...

  // Initialize Bitcoin client
  let bitcoin_client = BitcoinClient::new_from_config(&config.bitcoin)?;

  // Initialize storage and indexer store
  let storage = Rc::new(IndexerStore::new(&config.storage)?);
  let indexer_store = Rc::new(IndexerStore::new(storage)?);

  // Create an Indexer instance
  let indexer = Indexer::new(bitcoin_client, indexer_store.clone(), config.settings)?;

  // Check if the indexer is ready
  if indexer.is_ready()? {
      println!("Indexer is ready and at the latest block.");
  } else {
      println!("Indexer is not yet at the latest block.");
  }

  // Process the next block
  indexer.tick()?;

  // Retrieve the best block
  if let Some(best_block) = indexer.get_best_block()? {
      println!("Best block: {:?}", best_block);
  }

  // Retrieve the best block height
  if let Some(best_height) = indexer.get_best_height()? {
      println!("Best block height: {}", best_height);
  }

  // Retrieve the current blockchain best height
  let blockchain_best_height = indexer.get_blockchain_best_height()?;
  println!("Blockchain best height: {}", blockchain_best_height);

  // Example transaction ID (replace with a real one for actual use)
  let tx_id = "some_tx_id";
  let tx_info = indexer.get_transaction(&tx_id.parse()?)?;
  println!("Transaction status: {:?}", tx_info.status);
  match tx_info.status {
      TransactionBlockchainStatus::InMempool => {
          println!("Transaction is in mempool");
      }
      TransactionBlockchainStatus::NotFound => {
          println!("Transaction not found");
      }
      TransactionBlockchainStatus::Orphan => {
          println!("Transaction is orphaned");
      }
      TransactionBlockchainStatus::Confirmed => {
          println!("Transaction is confirmed with {} confirmations", 
                   tx_info.confirmations);
      }
      TransactionBlockchainStatus::Finalized => {
          println!("Transaction is finalized with {} confirmations", 
                   tx_info.confirmations);
      }
  }

```

## Configuration

### Checkpoint Height

The `checkpoint_height` is an optional setting in the indexer configuration that specifies a specific block height from which the indexing process should start. This can be useful for syncing from a specific height. Here's how it works:

- **With Checkpoint Height**:
  - If `checkpoint_height` is set, the indexer will begin syncing from the specified block height.
  - If the blockchain height is lower than the `checkpoint_height`, an error will occur as the network has not reached this checkpoint.
  - Once a checkpoint is set and indexed, you cannot change it unless the database is cleared.
  
- **Without Checkpoint Height**:
  - If not set, indexing will start from the genesis block or from the last indexed height if there's an existing index.

### Confirmation Threshold

The `confirmation_threshold` is a setting that determines when a transaction is considered "finalized". When a transaction has at least `confirmation_threshold` confirmations, its status will be set to `TransactionBlockchainStatus::Finalized`. Otherwise, it will be `TransactionBlockchainStatus::Confirmed`. The default value is `0`.

To configure these settings:

```yaml
# Example configuration in config.yaml
settings:
  checkpoint_height: 2000
  confirmation_threshold: 6  # Transactions with 6+ confirmations are considered finalized
```

## Transaction Status

The `get_transaction` method returns a `TransactionInfo` struct that includes a `status` field indicating the transaction's state:

- **`InMempool`**: The transaction is in the mempool and has not yet been confirmed in a block.
- **`Confirmed`**: The transaction has been confirmed in a block but has fewer confirmations than the `confirmation_threshold`.
- **`Finalized`**: The transaction has been confirmed with at least `confirmation_threshold` confirmations.
- **`Orphan`**: The transaction was confirmed but a reorganization moved it out of the chain.
- **`NotFound`**: The transaction is not present in the blockchain, not in the mempool, and has not been confirmed.

The `TransactionInfo` struct contains:
- `tx: Option<Transaction>` - The transaction data (if available)
- `block_info: Option<FullBlock>` - Information about the block containing the transaction (if confirmed)
- `confirmations: u32` - Number of confirmations (0 if not confirmed)
- `status: TransactionBlockchainStatus` - The current status of the transaction
- `confirmation_threshold: u32` - The confirmation threshold used to determine if a transaction is finalized

## Development Setup

1. Clone the repository
2. Install dependencies: `cargo build`
3. Run tests: `cargo test -- --test-threads=1`

## Examples
- **get_estimated_fee_rate:**
    1. Get a node endpoint (including spinning a node for regtest if needed) for your desired network
    3. Edit `config/development.yaml` with the correct values
    3. run with `cargo run --example feerate`

## Contributing
Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## License

This project is licensed under the MIT License - see [LICENSE](LICENSE) file for details.

---

## 🧩 Part of the BitVMX Ecosystem

This repository is a component of the **BitVMX Ecosystem**, an open platform for disputable computation secured by Bitcoin.  
You can find the index of all BitVMX open-source components at [**FairgateLabs/BitVMX**](https://github.com/FairgateLabs/BitVMX).

---
