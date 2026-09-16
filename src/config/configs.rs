use crate::{config::settings::*, errors::IndexerError};
use bitvmx_bitcoin_rpc::{rpc_config::RpcConfig, types::BlockHeight};
use bitvmx_settings::settings::load_config_file;
use serde::Deserialize;
use storage_backend::storage_config::StorageConfig;

macro_rules! ensure {
    ($cond:expr, $msg:expr) => {
        if !($cond) {
            return Err(IndexerError::InvalidConfiguration($msg.to_string()));
        }
    };
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)] // Enforce fields.
pub struct IndexerConfig {
    pub storage: StorageConfig,
    pub rpc: RpcConfig,

    #[serde(default)]
    pub settings: IndexerSettings,
}

impl IndexerConfig {
    pub fn load_config(path: &str) -> Result<Self, IndexerError> {
        let config = load_config_file::<Self>(Some(path.to_string()))
            .map_err(|e| IndexerError::InvalidConfiguration(e.to_string()))?;
        config.settings.validate()?;

        Ok(config)
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)] // Enforce fields.
pub struct IndexerSettings {
    /// Number of recent blocks kept on disk. Must be deeper than any reorg the chain can produce. It must also be at least the
    /// monitor's max_monitoring_confirmations, so a watched transaction never loses its block while it is still being watched.
    #[serde(default = "default_retention_depth")]
    pub retention_depth: BlockHeight,

    /// Resume from the stored cursor, indexing every block in between. When false, a restart jumps straight
    /// to tip - retention_depth, and any output pattern or spending UTXO event in the skipped range is lost.
    #[serde(default = "default_catch_up")]
    pub catch_up: bool,
}

fn default_retention_depth() -> BlockHeight {
    DEFAULT_RETENTION_DEPTH
}

fn default_catch_up() -> bool {
    DEFAULT_CATCH_UP
}

impl Default for IndexerSettings {
    fn default() -> Self {
        Self {
            retention_depth: DEFAULT_RETENTION_DEPTH,
            catch_up: DEFAULT_CATCH_UP,
        }
    }
}

impl IndexerSettings {
    pub fn new(retention_depth: BlockHeight, catch_up: bool) -> Self {
        Self {
            retention_depth,
            catch_up,
        }
    }

    /// Validates the settings that can be checked without touching the chain.
    pub fn validate(&self) -> Result<(), IndexerError> {
        ensure!(
            self.retention_depth >= MIN_RETENTION_DEPTH,
            "retention_depth must be at least 2 blocks, so a reorg can be rolled back"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The default settings pass validation.
    #[test]
    fn defaults_valid() {
        assert!(IndexerSettings::default().validate().is_ok());
        assert_eq!(
            IndexerSettings::default().retention_depth,
            DEFAULT_RETENTION_DEPTH
        );
        assert!(IndexerSettings::default().catch_up);
    }

    // A retention depth below the minimum is rejected, and the minimum itself is accepted.
    #[test]
    fn short_window_rejected() {
        for depth in 0..MIN_RETENTION_DEPTH {
            let err = IndexerSettings::new(depth, true).validate().unwrap_err();
            assert!(matches!(err, IndexerError::InvalidConfiguration(_)));
        }

        assert!(IndexerSettings::new(MIN_RETENTION_DEPTH, true)
            .validate()
            .is_ok());
    }

    // A setting that no longer exists, fails to parse.
    #[test]
    fn unknown_setting_rejected() {
        let err =
            serde_json::from_str::<IndexerSettings>(r#"{"checkpoint_height": 10}"#).unwrap_err();
        assert!(err.to_string().contains("checkpoint_height"));
    }

    // Settings left out of the config take their defaults.
    #[test]
    fn missing_settings_default() {
        let settings: IndexerSettings = serde_json::from_str(r#"{"retention_depth": 6}"#).unwrap();
        assert_eq!(settings.retention_depth, 6);
        assert_eq!(settings.catch_up, DEFAULT_CATCH_UP);
    }

    // The development config loads and validates.
    #[test]
    fn dev_config_loads() {
        let config = IndexerConfig::load_config("config/development.yaml").unwrap();
        assert!(config.settings.validate().is_ok());
    }

    // A config file that does not exist is an invalid configuration.
    #[test]
    fn missing_file_invalid() {
        let err = IndexerConfig::load_config("config/does_not_exist.yaml").unwrap_err();
        assert!(matches!(err, IndexerError::InvalidConfiguration(_)));
    }
}
