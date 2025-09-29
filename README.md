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

- **get_height_to_sync**: Determines the next block height that needs to be synchronized.

- **get_tx**: Retrieves transaction information for a given transaction ID.

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

  // Determine the next block height to sync
  let height_to_sync = indexer.get_height_to_sync()?;
  println!("Next block height to sync: {}", height_to_sync);

  // Example transaction ID (replace with a real one for actual use)
  let tx_id = "some_tx_id";
  if let Some(tx_info) = indexer.get_tx(&tx_id.parse()?)? {
      println!("Transaction info: {:?}", tx_info);
  }

```

## Development Setup

1. Clone the repository
2. Install dependencies: `cargo build`
3. Run tests: `cargo test -- --ignored --test-threads=1`

## Examples
- get_estimated_fee_rate - run with `cargo run --example feerate`

## Contributing
Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## License
This project is licensed under the MIT License.

