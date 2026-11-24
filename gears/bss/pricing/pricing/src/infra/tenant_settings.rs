//! The tenant pricing settings read from the Settings Service.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use settings_service_sdk::{BulkSelector, SettingKey, SettingsReaderClient};
use toolkit::client_hub::ClientHub;
use toolkit_canonical_errors::CanonicalError;
use uuid::Uuid;

use crate::domain::resolve::TenantDefaults;
use crate::domain::tenant_settings::{SettingsUnavailable, TenantSettings, TenantSettingsSource};
use crate::infra::reference_ticker::system_actor;
use crate::infra::settings_declarations::setting_keys;

/// How long one read of a tenant's settings may take before it counts as the Settings Service
/// being unavailable.
pub const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// [`TenantSettingsSource`] over the Settings Service client found in the `ClientHub` at the time
/// of each read.
pub struct SettingsServiceSource {
    hub: Arc<ClientHub>,
    read_timeout: Duration,
}

impl SettingsServiceSource {
    #[must_use]
    pub const fn new(hub: Arc<ClientHub>) -> Self {
        Self {
            hub,
            read_timeout: DEFAULT_READ_TIMEOUT,
        }
    }

    /// Replace [`DEFAULT_READ_TIMEOUT`].
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn with_read_timeout(mut self, read_timeout: Duration) -> Self {
        self.read_timeout = read_timeout;
        self
    }
}

#[async_trait]
impl TenantSettingsSource for SettingsServiceSource {
    async fn effective(&self, tenant_id: Uuid) -> Result<TenantSettings, SettingsUnavailable> {
        // Looked up at each read: the Settings Service may bind its client after this gear's
        // state is built.
        let reader = self
            .hub
            .try_get::<dyn SettingsReaderClient>()
            .ok_or_else(|| unavailable("no Settings Service client is bound"))?;
        let keys = setting_keys().map_err(|error| {
            unavailable(format!("a declared setting key does not parse: {error}"))
        })?;
        let actor = system_actor(tenant_id)
            .map_err(|error| refused("the system actor could not be built", &error))?;
        let read = reader.get_effective_bulk(
            &actor,
            BulkSelector::Keys(keys.to_vec()),
            format!("/tenants/{tenant_id}"),
        );
        let outcomes = tokio::time::timeout(self.read_timeout, read)
            .await
            .map_err(|_| unavailable("the Settings Service did not answer in time"))?
            .map_err(|error| refused("the read failed", &error))?;

        let mut values = HashMap::new();
        for outcome in outcomes {
            let answer = outcome.result.map_err(|error| {
                refused(
                    &format!("{} could not be read", outcome.key.leaf_slug()),
                    &error,
                )
            })?;
            values.insert(outcome.key, answer.value);
        }
        settings_from(keys, values)
    }
}

/// The settings out of the values answered for `keys`. Every setting or none: one left out would
/// be taken for its default.
fn settings_from(
    keys: &[SettingKey],
    mut values: HashMap<SettingKey, Value>,
) -> Result<TenantSettings, SettingsUnavailable> {
    let mut settings = TenantSettings {
        defaults: TenantDefaults {
            default_timing: String::new(),
            default_gl: None,
            default_tax_category: None,
            invoice_line_templates: BTreeMap::new(),
        },
        offered_currencies: Vec::new(),
    };
    for key in keys {
        let name = key.leaf_slug();
        let value = values
            .remove(key)
            .ok_or_else(|| unavailable(format!("{name} was not answered")))?;
        let misshapen = || unavailable(format!("{name} holds a value of another shape"));
        let defaults = &mut settings.defaults;
        match name {
            "default_timing" => {
                defaults.default_timing = text(value)
                    .filter(|timing| matches!(timing.as_str(), "advance" | "arrears"))
                    .ok_or_else(misshapen)?;
            }
            "default_gl" => defaults.default_gl = set(text(value).ok_or_else(misshapen)?),
            "default_tax_category" => {
                defaults.default_tax_category = set(text(value).ok_or_else(misshapen)?);
            }
            "offered_currencies" => {
                settings.offered_currencies =
                    serde_json::from_value(value).map_err(|_| misshapen())?;
            }
            _ => {
                let kind = name
                    .strip_prefix(TEMPLATE_PREFIX)
                    .ok_or_else(|| unavailable(format!("{name} is declared and not read")))?;
                if let Some(template) = set(text(value).ok_or_else(misshapen)?) {
                    defaults
                        .invoice_line_templates
                        .insert(kind.to_owned(), template);
                }
            }
        }
    }
    Ok(settings)
}

/// The leaf name of a template setting is this prefix and the SKU kind the template is for.
const TEMPLATE_PREFIX: &str = "invoice_line_template_";

fn unavailable(detail: impl Into<String>) -> SettingsUnavailable {
    SettingsUnavailable {
        detail: detail.into(),
        status: None,
        diagnostic: None,
    }
}

/// The Settings Service answered with an error. An internal failure says why only in its
/// diagnostic, so that is kept beside the status for the log.
fn refused(what: &str, error: &CanonicalError) -> SettingsUnavailable {
    SettingsUnavailable {
        detail: format!("{what}: {error}"),
        status: Some(error.status_code()),
        diagnostic: error.diagnostic().map(str::to_owned),
    }
}

fn text(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        _ => None,
    }
}

/// The settings declare the empty string as their default: it stands for a setting nobody set.
fn set(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// A double of the Settings Service's reader for the tests of the settings reads.
#[cfg(any(test, feature = "test-support"))]
#[path = "tenant_settings_doubles.rs"]
pub mod doubles;

#[cfg(test)]
#[path = "tenant_settings_tests.rs"]
mod tenant_settings_tests;
