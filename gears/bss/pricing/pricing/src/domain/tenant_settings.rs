//! The tenant's pricing settings as the pricing rules read them, and the port they come through.

use async_trait::async_trait;
use uuid::Uuid;

use crate::domain::resolve::TenantDefaults;

/// A tenant's effective pricing settings.
#[toolkit_macros::domain_model]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantSettings {
    /// What an invoice line falls back to when neither its entry nor its SKU says.
    pub defaults: TenantDefaults,
    /// The currencies a new price book may take. Empty offers every currency.
    pub offered_currencies: Vec<String>,
}

/// The settings could not be read. Pricing does not guess them: a guessed default would bill a
/// tenant by somebody else's rules.
#[toolkit_macros::domain_model]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the tenant pricing settings are unavailable: {detail}")]
pub struct SettingsUnavailable {
    /// What went wrong, for the log. It names settings, never a tenant or a value.
    pub detail: String,
    /// The status the Settings Service answered with, when it answered.
    pub status: Option<u16>,
    /// The Settings Service's own diagnostic of an internal failure, which its answer's text
    /// leaves out.
    pub diagnostic: Option<String>,
}

/// Where a tenant's effective pricing settings are read from.
#[async_trait]
pub trait TenantSettingsSource: Send + Sync {
    /// The effective settings of `tenant_id`, all read together.
    ///
    /// # Errors
    ///
    /// [`SettingsUnavailable`] whenever the settings cannot be told for certain.
    async fn effective(&self, tenant_id: Uuid) -> Result<TenantSettings, SettingsUnavailable>;
}
