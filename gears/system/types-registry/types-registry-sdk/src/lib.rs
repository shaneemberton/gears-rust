//! Types Registry SDK
//!
//! This crate provides the public API for the `types-registry` gear:
//! - `TypesRegistryClient` trait for inter-gear communication
//! - `GtsTypeSchema` / `GtsInstance` typed entity models
//! - `TypeSchemaQuery` / `InstanceQuery` for filtering
//! - `GtsTypeId` / `GtsInstanceId` typed identifiers
//! - `TypesRegistryError` for error handling
//!
//! ## Usage
//!
//! Consumers obtain the client from `ClientHub`:
//! ```ignore
//! use types_registry_sdk::{TypeSchemaQuery, TypesRegistryClient};
//!
//! let client = hub.get::<dyn TypesRegistryClient>()?;
//!
//! let schema = client.get_type_schema("gts.acme.core.events.user.v1~").await?;
//! let schemas = client
//!     .list_type_schemas(TypeSchemaQuery::default().with_pattern("gts.acme.*"))
//!     .await?;
//! ```

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod api;
pub mod error;
pub mod models;

#[cfg(feature = "test-util")]
pub mod testing;

pub use api::TypesRegistryClient;
pub use error::TypesRegistryError;
pub use models::{
    GtsInstance, GtsTypeId, GtsTypeSchema, InstanceQuery, RegisterResult, RegisterSummary,
    TypeSchemaQuery, is_type_schema_id,
};

// Re-export the underlying gts identifier types so consumers don't need a
// direct dependency on `gts` for typed IDs.
pub use gts::GtsInstanceId;
