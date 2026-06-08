// Updated: 2026-04-07 by Constructor Tech
//! Configuration for the credstore gear.

use serde::Deserialize;

/// Gear configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CredStoreConfig {
    /// Vendor selector used to pick a plugin implementation.
    ///
    /// The gear queries types-registry for plugin instances matching
    /// this vendor and selects the one with lowest priority number.
    pub vendor: String,
}

impl Default for CredStoreConfig {
    fn default() -> Self {
        Self {
            vendor: "constructorfabric".to_owned(),
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "config_tests.rs"]
mod config_tests;
