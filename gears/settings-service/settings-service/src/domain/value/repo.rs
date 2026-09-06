// Created: 2026-09-06 by Constructor Tech
//! Persistence port over `setting_values`.

use async_trait::async_trait;
use toolkit_db::secure::DBRunner;
use toolkit_security::AccessScope;
use uuid::Uuid;

use super::{StoredValue, ValueDraft};
use crate::domain::error::DomainError;

/// Persistence operations on stored values.
///
/// Every read here is over the **subject-less** track: rows carrying a
/// `(subject_type, subject_id)` pair belong to the subject dimension and are
/// never returned to a request that named no subject.
#[async_trait]
pub trait ValueRepository: Send + Sync {
    /// Re-sync the denormalized classification on every value row of a
    /// declaration, returning how many rows changed.
    ///
    /// # Errors
    /// [`DomainError`] when the update fails.
    async fn resync_classification<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        declaration_id: Uuid,
        data_classification: &str,
    ) -> Result<u64, DomainError>;

    /// The rows of one declaration whose tenant is in `tenant_ids` — the
    /// cascading walk's one exact-match set query, never a prefix scan.
    ///
    /// # Errors
    /// [`DomainError`] when the read fails.
    async fn find_in_tenants<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        declaration_id: Uuid,
        tenant_ids: &[Uuid],
    ) -> Result<Vec<StoredValue>, DomainError>;

    /// The row of one declaration at exactly one tenant.
    ///
    /// # Errors
    /// [`DomainError`] when the read fails.
    async fn find_one<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        declaration_id: Uuid,
        tenant_id: Uuid,
    ) -> Result<Option<StoredValue>, DomainError>;

    /// The rows flagged for review among `declaration_ids`, at any of
    /// `tenant_ids` — the administrative needs-review listing.
    ///
    /// # Errors
    /// [`DomainError`] when the read fails.
    async fn list_flagged<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        declaration_ids: &[Uuid],
        tenant_ids: &[Uuid],
    ) -> Result<Vec<StoredValue>, DomainError>;

    /// Insert a row.
    ///
    /// # Errors
    /// [`DomainError::Conflict`] when a row already exists at the scope;
    /// [`DomainError`] when the write fails.
    async fn insert<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        draft: ValueDraft,
    ) -> Result<StoredValue, DomainError>;
}
