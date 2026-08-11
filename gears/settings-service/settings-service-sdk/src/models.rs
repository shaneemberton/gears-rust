// Created: 2026-08-11 by Constructor Tech
// @cpt-dod:cpt-cf-settings-service-dod-gear-foundation-sdk-models:p1
//! Public models exchanged with settings consumers.
//!
//! Phase 1 delivers the opaque secret handle and the effective-source
//! vocabulary. The reader request and response shapes arrive with the trait
//! contracts in phase 2.

use serde::{Deserialize, Serialize};

/// Where an effective value resolved from.
///
/// A successful read always carries a value, because every declaration has a
/// Schema Default and all three scope-class algorithms terminate in one. A
/// consumer distinguishing *an administrator set this* from *nobody has touched
/// it* therefore reads the source, never the value: a setting whose type admits
/// `null` may legitimately be set to `null`, which is indistinguishable by
/// inspection from a `null` default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveSource {
    /// An override exists at the requested scope.
    OwnOverride,
    /// Resolved from a nearest-ancestor override.
    Inherited,
    /// No override anywhere in the chain; the declaration's own default.
    SchemaDefault,
}

impl EffectiveSource {
    /// Whether this source means no override exists anywhere in the chain.
    ///
    /// For a `secret`-trait setting this is also how a machine consumer detects
    /// an unconfigured credential: the declaration's default is a non-secret
    /// placeholder, so a schema-default source means no credential is set at any
    /// scope and the placeholder must be treated as absent.
    #[must_use]
    pub const fn is_unconfigured(self) -> bool {
        matches!(self, Self::SchemaDefault)
    }
}

/// An opaque reference to a `secret`-trait value.
///
/// Returned in place of plaintext on every read of a secret-backed setting.
/// The handle deliberately carries **no** Credential Store coordinates, so a
/// consumer cannot bypass the audited resolution path by reading it apart; the
/// only way to obtain plaintext is to present the handle back to the reader,
/// which authorizes the caller against that specific setting and emits a
/// secret-use audit event.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretHandle(String);

impl SecretHandle {
    /// Wrap an opaque token as a secret handle.
    #[must_use]
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The opaque token, for transport only.
    #[must_use]
    pub fn as_token(&self) -> &str {
        &self.0
    }
}

/// Redacted on purpose: a secret handle must never widen a log line into a
/// disclosure path, so neither the token nor any derived material is printed.
impl std::fmt::Debug for SecretHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretHandle(<redacted>)")
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod models_tests;
