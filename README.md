# rust-bitcoin-indexer

`rust-bitcoin-indexer` turns a Bitcoin node's stateless RPC into a sequential, resumable, reorg aware feed. It reads the chain one block at a time, keeps a window of recent blocks on disk, and answers where a transaction or a block stands, counted against the chain it has actually processed rather than against whatever the node reports at that instant.

## ⚠️ Disclaimer

This library is currently under development and may not be fully stable. It is not production-ready, has not been audited, and future updates may introduce breaking changes without preserving backward compatibility.

## Key Features

- 🧱 **Sequential feed**: each call to `tick()` moves the indexer at most one block, so a consumer sees every block once and in order, and can stop at any point and resume later.
- ↩️ **Reorg aware**: a block that leaves the chain is removed with the transaction entries it brought, one block per tick, and the new chain is then indexed in the same order.
- 🪟 **Bounded storage**: only the last `retention_depth` blocks are kept, so disk usage stops growing no matter how long the indexer runs.
- 🔎 **Transaction and block queries**: the status of a transaction, with its confirmations, and any block of the chain the indexer has processed.
- 📬 **Mempool watch list**: txids a consumer registers are checked against the node's mempool once per tick, so repeated queries about them cost nothing extra.
- ⛽ **Fee estimation**: each indexed block carries a fee rate estimated.
- 💾 **Persistent storage**: all state is persisted through `rust-bitvmx-storage-backend`, and a restart continues from where the previous run stopped.

## Architecture

| Component | Responsibility |
|---|---|
| `Indexer` | The API. Advances the chain, answers queries, owns the rules below. |
| `IndexerStore` | Blocks by height, a txid to height entry per transaction, the cursor, the mempool watch list and its snapshot. |
| `helper` | The pure calculations: window start, confirmations, the height to prune, and the fee estimate. |

**The cursor** is the height of the highest block the indexer has read. It always has a block stored, and everything is counted from it: confirmations, what is inside the window, and what still has to be read.

**Building an indexer reads nothing from the node.** The first `tick()` places the cursor: a fresh database starts one window below the tip, a restart resumes from its cursor, and a restart with `catch_up` disabled jumps to one window below the tip when that skips blocks. Until then `is_ready` is false and anything counted from the cursor fails with `NotSynced`.

**Every later tick** reads the node's tip and the block at the cursor, then does exactly one of:

1. the node's chain is shorter than the indexed one, so the blocks above its tip are removed;
2. the node has a different block at the cursor, so that block is removed and the cursor steps back;
3. the cursor is at the tip, so there is nothing to do;
4. the next block does not build on the block at the cursor, so nothing is stored and the next tick resolves it;
5. otherwise the next block is stored, the cursor moves onto it, and the block that falls out of the window is deleted.

It then refreshes the mempool watch list: entries already in a stored block are marked confirmed, and the rest are checked against the node's mempool. `tick()` returns `true` only in case 5.

## Public API

The `Indexer` struct exposes:

| Method | Purpose |
|---|---|
| `new` | Build from an RPC client, a store and optional settings. Validates the settings and reads nothing from the node. |
| `is_ready` | True once the cursor has reached the node's tip, so there is nothing left to read. False before the first tick. |
| `tick` | Place the cursor on the first call, then advance at most one block and refresh the mempool watch list. |
| `get_indexed_height` | Height of the highest block read. |
| `get_last_indexed_block` | That block, with its transactions and fee rate. |
| `get_block` | The block with a given height and hash, from storage or from the node. |
| `get_stored_block` | The same block, from the indexer alone. |
| `get_transaction` | The status of a txid: confirmed, with the transaction, its block's height and hash and its confirmations; in the mempool; or not found. Asks the node when the indexer holds nothing. |
| `get_stored_transaction` | The same status, from the indexer alone. |
| `get_estimated_fee_rate` | Fee rate of the last indexed block, once the indexer is at the tip. |
| `add_mempool_watch` / `remove_mempool_watch` | Register or drop a txid to follow in the mempool. |
| `rpc_is_utxo_unspent` / `rpc_get_tx_confirmations` | Live node checks, passed straight through. |

Methods with the `rpc_` prefix answer from the node alone, with none of the indexer's own state involved.

### How a transaction is answered

`get_transaction(txid, include_mempool)` tries three steps, in order:

1. **A block the indexer holds.** Answers `Confirmed` with the transaction, its block's height and hash, and the confirmations counted from the cursor.
2. **The mempool snapshot**, when `include_mempool` is true and the txid is on the watch list. Answers `InMempool`.
3. **The node**, which covers a transaction mined below the window and one in the mempool that nobody watches. It answers `Confirmed` only for a block below everything the indexer holds. A transaction in a block the indexer has not reached yet, or in a block it has not unwound yet, is reported as pending: `InMempool` when `include_mempool` is true, `NotFound` when it is false.

`get_stored_transaction(txid, include_mempool)` stops after step 2, for a caller that wants no node call and reads `NotFound` as "the indexer holds nothing about it".

`get_block(height, hash)` follows the same idea: a block the indexer holds is returned when the hash matches, a block below the window is downloaded from the node when the node has that hash at that height, and anything the indexer has not processed yet gives `None`. `get_stored_block(height, hash)` stops at the first of those, for a caller that wants no node call.

> 💡 **The window is the source of truth.** Below it the node is trusted, above it only the indexer's own chain counts. That is what keeps every answer consistent with the blocks a consumer has already been given.

> ⚠️ **Reorgs deeper than `retention_depth` are out of scope.** Such a reorg fails with `ReorgDeeperThanWindow` and the indexer stops advancing, so choose a depth well past anything the chain produces.

> ⚠️ **Use the same `retention_depth` across restarts.** Pruning is computed from the configured depth, so lowering it leaves the blocks below the new window, and their transaction entries, in storage.

> 💡 **Queries are answered as of the last tick.** `is_ready` and `get_estimated_fee_rate` compare heights, and the mempool snapshot is the one the last tick wrote, so a reorg is reflected on the next tick.

> 💡 **The watch list belongs to the consumer.** Entries are never removed by the indexer, not on confirmation and not after a reorg, so a consumer that no longer cares about a txid removes it.

> 💡 **A transaction outside the window costs node calls.** Answers from a held block are free, while step 3 asks the node on every call.

## Configuration

`IndexerConfig` has three sections: `storage`, `rpc` and `settings`. A sample is in `config/development.yaml`.

| Setting | Default | Meaning |
|---|---|---|
| `retention_depth` | 100 | How many recent blocks stay on disk. Must be at least 2, and above the confirmations at which a consumer treats a transaction as final. |
| `catch_up` | true | Resume from the stored cursor, reading every block in between. |

Choosing `retention_depth` means balancing two things: it has to be deeper than any reorg the chain can produce, and at least the confirmations a consumer waits for, so a watched transaction keeps its block while it is still being watched. Disk usage grows with it.

With `catch_up` disabled, a restart that would otherwise have blocks to read jumps straight to one window below the tip and deletes the window it held, so the blocks in between are never read. That is the faster way back to the tip when the events in the skipped range do not matter.

## Development Setup

Prerequisites:

- Rust
- A Bitcoin node running with `-txindex=1`
- Docker, used by the integration tests

Common commands:

```bash
# Build everything (lib + tests).
cargo build --release --tests

# Run the unit test suite.
cargo test --release --lib

# Run the integration tests (require Docker running; one bitcoind per test).
cargo test --release
```

The `feerate` example runs the indexer against a real node: point `config/development.yaml` at it and run `cargo run --release --example feerate`.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## License

This project is licensed under the MIT License - see [LICENSE](LICENSE) file for details.

---

## 🧩 Part of the BitVMX Ecosystem

This repository is a component of the **BitVMX Ecosystem**, an open platform for disputable computation secured by Bitcoin.  
You can find the index of all BitVMX open-source components at [**FairgateLabs/BitVMX**](https://github.com/FairgateLabs/BitVMX).

---
