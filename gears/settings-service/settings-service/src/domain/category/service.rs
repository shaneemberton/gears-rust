// Created: 2026-08-13 by Constructor Tech
//! Category operations.
//!
//! Generic over the repository rather than holding a trait object: the
//! repository's methods are generic over [`DBRunner`] so the same code runs on
//! a connection and inside a transaction, and a generic method cannot be part
//! of an object-safe trait. The cost is a type parameter; the benefit is that
//! every operation below can run in a transaction when its caller needs one.

use toolkit_db::secure::DBRunner;
use toolkit_security::AccessScope;
use uuid::Uuid;

use super::visibility::{self, DomainVisibility};
use super::{Category, CategoryDraft, CategoryRepository};
use crate::api::precondition::{self, ETag};
use crate::domain::error::DomainError;

/// Whether a draft renames onto a key another category already holds.
///
/// Extracted so the rule is testable: every operation below takes a
/// [`DBRunner`], and `toolkit-db` deliberately exposes no way to construct one
/// outside a live gear, so the orchestration around this can only be exercised
/// end to end. The rule itself is the part worth pinning — a re-check that
/// forgot to exclude the row being updated would make renaming a category
/// impossible without also changing its key.
#[must_use]
pub(crate) fn is_rename_collision(
    current: &super::CategoryKey,
    draft: &super::CategoryKey,
    key_is_taken: bool,
) -> bool {
    draft != current && key_is_taken
}

/// Refuse query options this resource does not implement.
///
/// `$select` is parsed by the platform but not honoured here: supporting it
/// means a response whose shape varies per request, and no caller has asked
/// for it. Refusing is deliberate rather than ignoring — a caller whose
/// projection was silently dropped receives every field believing it asked for
/// two, which is the same failure the declared filter surface exists to
/// prevent.
///
/// # Errors
/// [`DomainError::Validation`] naming the unsupported option.
fn reject_unsupported_options(query: &toolkit_odata::ODataQuery) -> Result<(), DomainError> {
    if query.select.is_some() {
        return Err(DomainError::Validation {
            field: "$select".to_owned(),
            code: crate::field::ODATA_UNSUPPORTED_OPTION,
            message: "$select is not supported on categories; omit it to receive the full \
                      representation"
                .to_owned(),
        });
    }
    Ok(())
}

/// Category create, read, update and delete.
pub struct CategoryService<R> {
    repo: R,
}

impl<R: CategoryRepository> CategoryService<R> {
    /// Build the service over a repository.
    pub const fn new(repo: R) -> Self {
        Self { repo }
    }

    /// Fetch one category.
    ///
    /// # Errors
    /// [`DomainError::NotFound`] when no category has that id **or** the caller
    /// cannot see it — the two are one answer on purpose, since a distinct
    /// "exists but forbidden" would let a caller enumerate ids it has no access
    /// to.
    pub async fn get<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        id: Uuid,
    ) -> Result<Category, DomainError> {
        let found = self.repo.find(conn, scope, id).await?;

        // @cpt-begin:cpt-cf-settings-service-algo-category-management-visibility-filter:p1:inst-cat-visfilter-3
        // The row-level arm of the visibility rule. `list` applies it as a SQL
        // predicate; a single-row fetch has no query to augment, so it is
        // applied here — and applied as *not found* rather than *forbidden*,
        // because a distinct denial would confirm the category exists.
        let visible = visibility::domain_visibility(scope);
        let found = found.filter(|category| {
            visibility::is_visible(&visible, category.domain_affinity.as_deref())
        });
        // @cpt-end:cpt-cf-settings-service-algo-category-management-visibility-filter:p1:inst-cat-visfilter-3

        found.ok_or(DomainError::NotFound {
            resource: "category",
        })
    }

    /// The caller's domain restriction, for a handler to fold into a list query.
    ///
    /// Exposed rather than applied here because `list` must put the predicate
    /// **inside** the query: filtering a page after the fact gives short pages
    /// and a cursor that skips rows the caller was entitled to.
    #[must_use]
    pub fn visibility(scope: &AccessScope) -> DomainVisibility {
        visibility::domain_visibility(scope)
    }

    /// List categories for the caller.
    ///
    /// # Errors
    /// [`DomainError::Validation`] when the query names an unmapped field, uses
    /// an unsupported operator, requests an unsupported option, or carries an
    /// undecodable cursor.
    pub async fn list<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        query: &toolkit_odata::ODataQuery,
    ) -> Result<toolkit_odata::Page<Category>, DomainError> {
        reject_unsupported_options(query)?;
        let visible = visibility::domain_visibility(scope);
        self.repo.list(conn, scope, &visible, query).await
    }

    /// Create a category.
    ///
    /// # Errors
    /// [`DomainError::Conflict`] when the key or name is taken.
    pub async fn create<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        draft: CategoryDraft,
    ) -> Result<Category, DomainError> {
        // A courtesy check, not the guarantee. `uq_category_key` decides, and
        // the repository surfaces its violation — two administrators creating
        // the same key concurrently both pass here and only one insert wins.
        // Skipping this would be correct but would report the collision as a
        // database error rather than as the conflict it is.
        if self
            .repo
            .find_by_key(conn, scope, &draft.key)
            .await?
            .is_some()
        {
            return Err(DomainError::Conflict {
                detail: format!("a category with key `{}` already exists", draft.key),
            });
        }
        self.repo.insert(conn, scope, draft).await
    }

    /// Update a category, guarded by `If-Match`.
    ///
    /// # Errors
    /// [`DomainError::PreconditionRequired`] when no `If-Match` was supplied,
    /// [`DomainError::PreconditionFailed`] when it is stale,
    /// [`DomainError::NotFound`] when the category is not visible to the caller.
    pub async fn update<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        id: Uuid,
        if_match: Option<&str>,
        draft: CategoryDraft,
    ) -> Result<Category, DomainError> {
        let current = self.get(conn, scope, id).await?;
        precondition::evaluate(if_match, &current.etag)?;

        // Re-checked against the *other* rows: a rename onto a key another
        // category already holds is a conflict, but keeping your own key is not.
        let key_is_taken = self
            .repo
            .find_by_key(conn, scope, &draft.key)
            .await?
            .is_some();
        if is_rename_collision(&current.key, &draft.key, key_is_taken) {
            return Err(DomainError::Conflict {
                detail: format!("a category with key `{}` already exists", draft.key),
            });
        }

        self.repo.update(conn, scope, id, draft).await
    }

    /// Delete a category, guarded by `If-Match` and the no-orphan rule.
    ///
    /// # Errors
    /// [`DomainError::PreconditionRequired`] / [`DomainError::PreconditionFailed`]
    /// as for [`Self::update`], [`DomainError::NotFound`] when not visible, and
    /// [`DomainError::Conflict`] while any declaration still references it.
    pub async fn delete<C: DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        id: Uuid,
        if_match: Option<&str>,
    ) -> Result<(), DomainError> {
        let current = self.get(conn, scope, id).await?;
        precondition::evaluate(if_match, &current.etag)?;

        // @cpt-begin:cpt-cf-settings-service-flow-category-management-delete:p1:inst-cat-delete-8
        // Advisory, by design. The foreign key's `ON DELETE RESTRICT` is the
        // authoritative guard and catches a declaration created between this
        // check and the delete; what this adds is a specific message instead of
        // a constraint error, for the overwhelmingly common case.
        //
        // The check answering `Err` still denies: the guard exists to prevent an
        // orphan, and an unanswerable question is not a negative answer.
        let referenced = self.repo.has_referencing_declarations(conn, id).await?;
        // @cpt-end:cpt-cf-settings-service-flow-category-management-delete:p1:inst-cat-delete-8

        // @cpt-begin:cpt-cf-settings-service-flow-category-management-delete:p1:inst-cat-delete-9
        if referenced {
            return Err(DomainError::Conflict {
                detail: "the category still has declarations referencing it".to_owned(),
            });
        }
        // @cpt-end:cpt-cf-settings-service-flow-category-management-delete:p1:inst-cat-delete-9

        self.repo.delete(conn, scope, id).await
    }

    /// The tag a caller must echo to mutate this category.
    ///
    /// Exposed so a handler can set the `ETag` response header from the same
    /// value the next `If-Match` will be compared against.
    #[must_use]
    pub fn etag_of(category: &Category) -> &ETag {
        &category.etag
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod service_tests;
