use bitvmx_bitcoin_rpc::types::BlockHeight;

// Default Bitcoin Indexer constants

// Number of recent blocks kept on disk. Must be deeper than any reorg the chain can produce, and at least
// the monitor's max_monitoring_confirmations. The indexer can check neither bound.
pub const DEFAULT_RETENTION_DEPTH: BlockHeight = 100;

// A reorg is unwound one block per tick, so the block below the cursor must still be there. That is the only
// bound the indexer can enforce on its own.
pub const MIN_RETENTION_DEPTH: BlockHeight = 2;

// Resume from the stored cursor and index every block in between. Skipping blocks loses the events that can
// only be found by reading them, so catching up is the default.
pub const DEFAULT_CATCH_UP: bool = true;
