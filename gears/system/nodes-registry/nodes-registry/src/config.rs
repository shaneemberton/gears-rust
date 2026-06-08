use serde::{Deserialize, Serialize};

/// Configuration for the nodes registry gear
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodesRegistryConfig {
    /// Enable/disable the nodes registry gear
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

impl Default for NodesRegistryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
        }
    }
}
