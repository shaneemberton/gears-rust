// Created: 2026-08-12 by Constructor Tech
//! The single conversion from [`DomainError`] to the platform error type.
//!
//! One arm per variant, deliberately flat: a reviewer checks this against the
//! enum by eye, and any nesting would hide an unmapped variant. Everything the
//! problem document needs — the `gts://` type URI, `title`, `status`, `trace_id`
//! — is derived by the platform from the canonical category chosen here, so the
//! only decision this file makes is *which category*, and that decision is what
//! the caller sees as an HTTP status.

use toolkit_canonical_errors::{CanonicalError, resource_error};

use crate::domain::error::DomainError;
use crate::precondition;

/// The resource this gear attributes its errors to.
#[resource_error("gts.cf.toolkit.settings.declaration.v1~")]
struct SettingsResource;

impl From<DomainError> for CanonicalError {
    fn from(err: DomainError) -> Self {
        match err {
            // 422 — the only path that carries field-level detail.
            DomainError::Validation {
                field,
                code,
                message,
            } => SettingsResource::invalid_argument()
                .with_field_violation(field, message, code)
                .create(),

            // 412 — a conditional write whose precondition no longer holds.
            DomainError::PreconditionFailed { detail } => SettingsResource::failed_precondition()
                .with_precondition_violation("setting", detail, precondition::ETAG_MISMATCH)
                .create(),

            // 409 — the request conflicts with current state.
            DomainError::Conflict { detail } => SettingsResource::already_exists(detail)
                .with_resource("setting")
                .create(),

            // 403 — carries nothing about the target, by construction: the
            // variant holds no identifier to leak, so a denial for a setting
            // that exists is byte-identical to one for a setting that does not.
            DomainError::Unauthorized => SettingsResource::permission_denied()
                .with_reason("SETTING_NOT_ENTITLED")
                .create(),

            // 404 — names the kind of resource, never the caller's identifier.
            DomainError::NotFound { resource } => {
                SettingsResource::not_found(format!("no such {resource}"))
                    .with_resource(resource)
                    .create()
            }

            // 503 — this service does not mask its own unavailability, so the
            // caller can retry or degrade on its own terms.
            DomainError::Unavailable { detail } => CanonicalError::service_unavailable()
                .with_detail(detail)
                .create(),

            // 500 — the diagnostic goes to the in-process channel the platform
            // strips from the wire, so nothing internal reaches the caller.
            DomainError::Internal { diagnostic } => CanonicalError::internal(diagnostic).create(),
        }
    }
}

#[cfg(test)]
#[path = "sdk_error_mapping_tests.rs"]
mod sdk_error_mapping_tests;
