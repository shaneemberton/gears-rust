// Created: 2026-08-13 by Constructor Tech
//! Category read handlers.
//!
//! Each obtains its `AccessScope` from the enforcement point **before** it
//! touches the service, so authorization is not something a handler can forget:
//! the scope is the argument every read needs, and there is no way to get one
//! except by asking the policy decision point.

use std::sync::Arc;

use axum::extract::Path;
use axum::{Extension, Json};
use toolkit::api::canonical_prelude::*;
use toolkit_security::SecurityContext;
use uuid::Uuid;

use crate::api::authz::{self, resource};
use crate::api::rest::dto::CategoryDto;
use crate::domain::category::{CategoryRepository, CategoryService};
use crate::domain::error::DomainError;

/// The concrete service the routes carry.
pub type ConcreteCategoryService =
    CategoryService<crate::infra::storage::category_repo::CategoryRepo>;

/// The action names authorization decisions are made against.
const READ: &str = "read";

/// `GET /settings-service/v1/categories/{id}`
///
/// # Errors
/// `403` when the caller is not entitled to read categories, `404` when no such
/// category exists **or** it falls outside the caller's administrative domain.
pub async fn get_category<R: CategoryRepository>(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<CategoryService<R>>>,
    Extension(db): Extension<Arc<toolkit_db::DBProvider<toolkit_db::DbError>>>,
    Extension(enforcer): Extension<Arc<authz_resolver_sdk::PolicyEnforcer>>,
    Path(id): Path<Uuid>,
) -> ApiResult<impl IntoResponse> {
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-2
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-3
    let scope = authz::access_scope(&enforcer, &ctx, &resource::CATEGORY, READ, Some(id)).await?;
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-3
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-2

    let conn = db.conn().map_err(|err| DomainError::Internal {
        diagnostic: err.to_string(),
    })?;

    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-4
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-5
    // The service applies the visibility rule and answers not-found for a
    // category outside the caller's domain, so this handler cannot leak an
    // existence signal by handling the two cases differently — it receives one
    // error for both.
    let category = svc.get(&conn, &scope, id).await?;
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-5
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-4

    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-8
    // The tag travels as a header, never in the body: one source, so a client
    // cannot echo back a stale copy it read from the wrong place.
    let etag = category.etag.as_str().to_owned();
    Ok((
        [(axum::http::header::ETAG, etag)],
        Json(CategoryDto::from(category)),
    ))
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-8
}

/// `GET /settings-service/v1/categories`
///
/// # Errors
/// `403` when the caller is not entitled, `400` when the query names an
/// unmapped field, uses an unsupported option, or carries an undecodable
/// cursor.
pub async fn list_categories<R: CategoryRepository>(
    Extension(ctx): Extension<SecurityContext>,
    Extension(svc): Extension<Arc<CategoryService<R>>>,
    Extension(db): Extension<Arc<toolkit_db::DBProvider<toolkit_db::DbError>>>,
    Extension(enforcer): Extension<Arc<authz_resolver_sdk::PolicyEnforcer>>,
    OData(query): OData,
) -> ApiResult<impl IntoResponse> {
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-2
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-3
    let scope = authz::access_scope(&enforcer, &ctx, &resource::CATEGORY, READ, None).await?;
    // @cpt-end:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-3
    // @cpt-end:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-2

    // Steps 4, 5 and 8 are the `OData` extractor's: it binds `$filter`,
    // `$orderby`, the page size and the cursor off the URL and rejects a
    // malformed expression or an undecodable cursor before this body runs. The
    // unmapped-field rejection happens a layer deeper, when the parsed tree is
    // resolved against the declared `CategoryFilterField` surface.

    let conn = db.conn().map_err(|err| DomainError::Internal {
        diagnostic: err.to_string(),
    })?;

    // @cpt-begin:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-9
    let page = svc.list(&conn, &scope, &query).await?;
    let items: Vec<CategoryDto> = page.items.into_iter().map(CategoryDto::from).collect();
    Ok(Json(toolkit_odata::Page {
        items,
        page_info: page.page_info,
    }))
    // @cpt-end:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-9
}
