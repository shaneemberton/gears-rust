//! Public error types for the `types-registry` gear.
//!
//! These errors are safe to expose to other gears and consumers. The
//! taxonomy is symmetric across kinds: every kind-specific failure gets a
//! kind-specific variant (`*GtsTypeSchema*` vs `*GtsInstance*`) so callers
//! can match on the variant they care about without parsing messages.

use std::time::Duration;

use thiserror::Error;

/// Errors that can be returned by the `TypesRegistryClient`.
#[derive(Error, Debug, Clone)]
pub enum TypesRegistryError {
    /// The string is not a valid GTS type-schema identifier.
    ///
    /// Covers parse failures, kind mismatches (an instance id was passed
    /// where a type-schema id was expected), and lookups that resolved to
    /// a non-type-schema entity.
    #[error("Invalid GTS type-schema id: {0}")]
    InvalidGtsTypeId(String),

    /// The string is not a valid GTS instance identifier.
    ///
    /// Covers parse failures, kind mismatches, missing chain prefix, and
    /// chain-prefix mismatches against a passed type-schema.
    #[error("Invalid GTS instance id: {0}")]
    InvalidGtsInstanceId(String),

    /// No GTS type-schema is registered under the given id or UUID.
    #[error("GTS type-schema not found: {0}")]
    GtsTypeSchemaNotFound(String),

    /// No GTS instance is registered under the given id or UUID.
    #[error("GTS instance not found: {0}")]
    GtsInstanceNotFound(String),

    /// Cannot register an entity because its required parent type-schema is
    /// not yet registered. The client should register the parent first and
    /// retry the failed entity.
    #[error(
        "Cannot register {dependent_id}: required type-schema {parent_type_id} is not registered"
    )]
    ParentTypeSchemaNotRegistered {
        /// The parent type-schema id that must be registered first.
        parent_type_id: String,
        /// The id of the entity whose registration failed.
        dependent_id: String,
    },

    /// An entity with the same GTS ID already exists.
    #[error("Entity already exists: {0}")]
    AlreadyExists(String),

    /// The list/query parameters are syntactically invalid (e.g., a pattern
    /// that doesn't follow GTS wildcard rules — section 10 of the GTS spec
    /// requires a single trailing `*` anchored at a segment boundary).
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Validation of the entity content failed.
    #[error("Validation failed: {0}")]
    ValidationFailed(String),

    /// The service is not currently available (e.g., still initializing).
    /// `retry_after` is a hint for how long the caller should wait before
    /// retrying; `message` carries a human-readable reason.
    #[error("Service unavailable: {message} (retry after {retry_after:?})")]
    ServiceUnavailable {
        /// Human-readable reason the service is not available.
        message: String,
        /// Suggested delay before retrying.
        retry_after: Duration,
    },

    /// An internal error occurred.
    #[error("Internal error: {0}")]
    Internal(String),
}

impl TypesRegistryError {
    /// Creates an `InvalidGtsTypeId` error.
    #[must_use]
    pub fn invalid_gts_type_id(message: impl Into<String>) -> Self {
        Self::InvalidGtsTypeId(message.into())
    }

    /// Creates an `InvalidGtsInstanceId` error.
    #[must_use]
    pub fn invalid_gts_instance_id(message: impl Into<String>) -> Self {
        Self::InvalidGtsInstanceId(message.into())
    }

    /// Creates a `GtsTypeSchemaNotFound` error.
    #[must_use]
    pub fn gts_type_schema_not_found(id_or_uuid: impl Into<String>) -> Self {
        Self::GtsTypeSchemaNotFound(id_or_uuid.into())
    }

    /// Creates a `GtsInstanceNotFound` error.
    #[must_use]
    pub fn gts_instance_not_found(id_or_uuid: impl Into<String>) -> Self {
        Self::GtsInstanceNotFound(id_or_uuid.into())
    }

    /// Creates a `ParentTypeSchemaNotRegistered` error.
    #[must_use]
    pub fn parent_type_schema_not_registered(
        parent_type_id: impl Into<String>,
        dependent_id: impl Into<String>,
    ) -> Self {
        Self::ParentTypeSchemaNotRegistered {
            parent_type_id: parent_type_id.into(),
            dependent_id: dependent_id.into(),
        }
    }

    /// Creates an `AlreadyExists` error.
    #[must_use]
    pub fn already_exists(gts_id: impl Into<String>) -> Self {
        Self::AlreadyExists(gts_id.into())
    }

    /// Creates an `InvalidQuery` error.
    #[must_use]
    pub fn invalid_query(message: impl Into<String>) -> Self {
        Self::InvalidQuery(message.into())
    }

    /// Creates a `ValidationFailed` error.
    #[must_use]
    pub fn validation_failed(message: impl Into<String>) -> Self {
        Self::ValidationFailed(message.into())
    }

    /// Creates a `ServiceUnavailable` error with a human-readable `message`
    /// and a `retry_after` hint.
    #[must_use]
    pub fn service_unavailable(message: impl Into<String>, retry_after: Duration) -> Self {
        Self::ServiceUnavailable {
            message: message.into(),
            retry_after,
        }
    }

    /// Returns `true` if this is a `ServiceUnavailable` error.
    #[must_use]
    pub const fn is_service_unavailable(&self) -> bool {
        matches!(self, Self::ServiceUnavailable { .. })
    }

    /// Creates an `Internal` error.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    /// Returns `true` if this is an `InvalidGtsTypeId` error.
    #[must_use]
    pub const fn is_invalid_gts_type_id(&self) -> bool {
        matches!(self, Self::InvalidGtsTypeId(_))
    }

    /// Returns `true` if this is an `InvalidGtsInstanceId` error.
    #[must_use]
    pub const fn is_invalid_gts_instance_id(&self) -> bool {
        matches!(self, Self::InvalidGtsInstanceId(_))
    }

    /// Returns `true` if this is a `GtsTypeSchemaNotFound` error.
    #[must_use]
    pub const fn is_gts_type_schema_not_found(&self) -> bool {
        matches!(self, Self::GtsTypeSchemaNotFound(_))
    }

    /// Returns `true` if this is a `GtsInstanceNotFound` error.
    #[must_use]
    pub const fn is_gts_instance_not_found(&self) -> bool {
        matches!(self, Self::GtsInstanceNotFound(_))
    }

    /// Returns `true` if this is any kind of not-found error
    /// (`GtsTypeSchemaNotFound` or `GtsInstanceNotFound`).
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::GtsTypeSchemaNotFound(_) | Self::GtsInstanceNotFound(_)
        )
    }

    /// Returns `true` if this is a `ParentTypeSchemaNotRegistered` error.
    #[must_use]
    pub const fn is_parent_type_schema_not_registered(&self) -> bool {
        matches!(self, Self::ParentTypeSchemaNotRegistered { .. })
    }

    /// Returns `true` if this is an already-exists error.
    #[must_use]
    pub const fn is_already_exists(&self) -> bool {
        matches!(self, Self::AlreadyExists(_))
    }

    /// Returns `true` if this is an `InvalidQuery` error.
    #[must_use]
    pub const fn is_invalid_query(&self) -> bool {
        matches!(self, Self::InvalidQuery(_))
    }

    /// Returns `true` if this is a validation error.
    #[must_use]
    pub const fn is_validation_failed(&self) -> bool {
        matches!(self, Self::ValidationFailed(_))
    }
}

#[cfg(test)]
#[path = "error_tests.rs"]
mod tests;
