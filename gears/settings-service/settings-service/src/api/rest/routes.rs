// Created: 2026-08-13 by Constructor Tech
//! Category route registration.
//!
//! Every route is built through `OperationBuilder` rather than mounted on the
//! router directly. That is what makes `.authenticated()` and the declared
//! error responses part of the route's definition instead of something a
//! handler is trusted to remember, and it is what registers the operation in
//! the `OpenAPI` document at the same time.

use std::sync::Arc;

use axum::Router;
use toolkit::api::canonical_prelude::*;
use toolkit::api::{OpenApiRegistry, OperationBuilder};
use toolkit_db::{DBProvider, DbError};

use crate::api::rest::dto::CategoryDto;
use crate::api::rest::handlers;
use crate::domain::category::CategoryService;
use crate::infra::storage::category_repo::CategoryRepo;

/// `OpenAPI` grouping for these operations.
const TAG: &str = "settings-categories";

/// Register the category routes.
///
/// The service, the database handle and the enforcement point travel as
/// extensions so a handler receives them as ordinary arguments — in particular
/// the enforcer, which is what makes obtaining an `AccessScope` the only way to
/// reach the service.
pub fn register_routes(
    router: Router,
    openapi: &dyn OpenApiRegistry,
    service: Arc<CategoryService<CategoryRepo>>,
    db: Arc<DBProvider<DbError>>,
    enforcer: Arc<authz_resolver_sdk::PolicyEnforcer>,
) -> Router {
    // @cpt-begin:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-1
    let router = OperationBuilder::get("/settings-service/v1/categories")
        .operation_id("settings_service.list_categories")
        .summary("List settings categories")
        .description(
            "List categories visible to the caller, ordered by sort order then name. \
             Supports OData `$filter` and `$orderby` over `key`, `name` and \
             `domainAffinity`, with cursor pagination. `$select` is not supported and \
             is rejected rather than ignored.",
        )
        .tag(TAG)
        .authenticated()
        .no_license_required()
        .handler(handlers::list_categories::<CategoryRepo>)
        .json_response_with_schema::<toolkit_odata::Page<CategoryDto>>(
            openapi,
            StatusCode::OK,
            "A page of categories with its pagination cursors",
        )
        .error_400(openapi)
        .error_401(openapi)
        .error_403(openapi)
        .error_500(openapi)
        .error_503(openapi)
        .register(router, openapi);
    // @cpt-end:cpt-cf-settings-service-flow-category-management-list:p1:inst-cat-list-1

    // @cpt-begin:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-1
    let router = OperationBuilder::get("/settings-service/v1/categories/{id}")
        .operation_id("settings_service.get_category")
        .summary("Get a settings category")
        .description(
            "Fetch one category by id. A category outside the caller's administrative \
             domain answers 404 rather than 403, so a hidden category's existence is \
             not disclosed. The response carries the category's ETag, which a later \
             PATCH or DELETE must echo in `If-Match`.",
        )
        .tag(TAG)
        .authenticated()
        .no_license_required()
        .handler(handlers::get_category::<CategoryRepo>)
        .json_response_with_schema::<CategoryDto>(
            openapi,
            StatusCode::OK,
            "The category, with its ETag",
        )
        .error_401(openapi)
        .error_403(openapi)
        .error_404(openapi)
        .error_500(openapi)
        .error_503(openapi)
        .register(router, openapi);
    // @cpt-end:cpt-cf-settings-service-flow-category-management-get:p1:inst-cat-get-1

    router
        .layer(axum::Extension(service))
        .layer(axum::Extension(db))
        .layer(axum::Extension(enforcer))
}
