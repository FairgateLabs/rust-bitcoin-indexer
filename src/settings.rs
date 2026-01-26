use bitvmx_bitcoin_rpc::types::BlockHeight;

// The default block height from which the indexer begins synchronization if no checkpoint is specified.
pub const DEFAULT_CHECKPOINT_HEIGHT: BlockHeight = 0;

// The default confirmation threshold for determining when a transaction is considered finalized.
pub const DEFAULT_CONFIRMATION_THRESHOLD: u32 = 6;
