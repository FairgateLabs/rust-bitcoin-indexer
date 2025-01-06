# Bitcoin Indexer

## Overview

The Bitcoin Indexer is a Rust-based tool designed to extract and store block and transaction IDs from the Bitcoin blockchain. This project aims to provide a simple and efficient way to index the blockchain starting from a specific height and save the gathered data into a rockdb storage for further analysis and processing.

## Key Features

- **Block and Transaction ID Extraction**: Retrieves and records block and transaction IDs from the Bitcoin blockchain.
- **Configurable Start Height**: Allows indexing to begin from a specified block height.
- **File Storage**: Saves the extracted data into a rockdb storage for easy access and further use.

## Installation
Clone the repository and initialize the submodules:
```bash
$ git clone git@github.com:FairgateLabs/rust-bitcoin-indexer
```

### Setup `.env` File

To set up the Bitcoin Indexer, you need to create a **.env** file. You can use the **.env.example** file as a reference.

### Envs/Args

To check Possible run

```
cargo run -- --help
```

### Tests

If you make some changes please run tests to verify everything still working as expected.

```
cargo test
```


