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
### Tests

If you make some changes please run tests to verify everything still working as expected.

```
cargo test
```

## Testing Locally

**Pre-requisites:**
1. Install Docker engine
2. Install [ACT](https://nektosact.com/installation/index.html)
3. Get the GitHub token, needed to fetch repositories
4. Remove all commented code in **src/tests/docker_integration_test.rs**

**Run all tests:**
```rust
$cargo test
```

**Run job locally**

Some `act` versions might have issues caching the templates versions and not using the last one and also with some of the authentication tokens, so before locally executing the tests, please do the following:

In project root:

If you're using a Linux Based OS:
```bash
rm -rf ~/.cache/act
```
If you're using windows
```powershell
Remove-Item -Recurse -Force $env:USERPROFILE\.cache\act
```
Then to execute the test use:
```bash
$act --pull -s SSH_PRIVATE_KEY="$(cat ~/.ssh/id_rsa)" -s GITHUB_TOKEN="token" -s REPO_ACCESS_TOKEN="token" -j 'local_test'
```
The use of the `--verbose` flag at the end of the test execution command is not required but is recomended, since it gives the user a more deep info on the total execution log
