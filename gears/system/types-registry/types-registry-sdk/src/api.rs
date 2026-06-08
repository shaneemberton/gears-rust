//! `TypesRegistryClient` trait definition.
//!
//! This trait defines the public API for the `types-registry` gear.
//! GTS type-schemas and instances are global resources, so no security context
//! is required.

use std::collections::HashMap;

use async_trait::async_trait;
use uuid::Uuid;

use crate::error::TypesRegistryError;
use crate::models::{GtsInstance, GtsTypeSchema, InstanceQuery, RegisterResult, TypeSchemaQuery};

/// Public API trait for the `types-registry` gear.
///
/// This trait can be consumed by other gears via `ClientHub`:
/// ```ignore
/// let client = hub.get::<dyn TypesRegistryClient>()?;
/// let schema = client.get_type_schema("gts.acme.core.events.user.v1~").await?;
/// ```
///
/// GTS type-schemas and instances are global resources (not tenant-scoped),
/// so no security context is required for these operations.
#[async_trait]
pub trait TypesRegistryClient: Send + Sync {
    // ------------------------------------------------------------------
    // Generic batch register (kind detected from gts_id suffix per item).
    // ------------------------------------------------------------------

    /// Register GTS entities (type-schemas or instances) in batch.
    ///
    /// Each JSON value must contain a valid GTS identifier in one of the
    /// configured ID fields (`$id`, `gtsId`, `id`). The batch is sorted
    /// lexicographically by GTS id before processing so parents are
    /// registered before their children within the same batch.
    ///
    /// Per-item failures are reported via [`RegisterResult::Err`]; success
    /// carries only the canonical [`gts_id`](RegisterResult::Ok). To inspect
    /// the typed view of a registered entity, follow up with
    /// [`Self::get_type_schema`] / [`Self::get_instance`].
    ///
    /// # Errors
    ///
    /// Returns `Err` only for catastrophic failures (e.g., backend unavailable).
    async fn register(
        &self,
        entities: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError>;

    // ------------------------------------------------------------------
    // Type-schema operations (internal — no tenant scoping).
    // ------------------------------------------------------------------

    /// Register GTS type-schemas in batch.
    ///
    /// Each input value must have a GTS id ending with `~`. Inputs whose
    /// identifier does not match the type-schema kind are returned as
    /// per-item `RegisterResult::Err` with `InvalidGtsTypeId`. In ready
    /// phase, items whose chain parent is not yet registered fail with
    /// `ParentTypeSchemaNotRegistered` (callers may register the parent
    /// then retry the failed item).
    ///
    /// # Errors
    ///
    /// Returns `Err` only for catastrophic failures.
    async fn register_type_schemas(
        &self,
        type_schemas: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError>;

    /// Retrieve a registered GTS type-schema by its type id.
    ///
    /// # Errors
    ///
    /// * `GtsTypeSchemaNotFound` — no type-schema with this id is registered.
    /// * `InvalidGtsTypeId` — id format is invalid, kind-mismatched, or
    ///   resolves to a non-type-schema entity.
    async fn get_type_schema(&self, type_id: &str) -> Result<GtsTypeSchema, TypesRegistryError>;

    /// Retrieve a registered GTS type-schema by its deterministic UUID v5.
    ///
    /// # Errors
    ///
    /// * `GtsTypeSchemaNotFound` — no type-schema is registered with this UUID
    ///   (also returned when the UUID exists but points to an instance).
    async fn get_type_schema_by_uuid(
        &self,
        type_uuid: Uuid,
    ) -> Result<GtsTypeSchema, TypesRegistryError>;

    /// Retrieve multiple type-schemas by id in one call.
    ///
    /// Returns a map keyed by the input ids; each value is a per-item
    /// `Result` carrying either the resolved schema or the per-item error
    /// ([`GtsTypeSchemaNotFound`](TypesRegistryError::GtsTypeSchemaNotFound),
    /// [`InvalidGtsTypeId`](TypesRegistryError::InvalidGtsTypeId), …).
    /// Duplicate ids in the input collapse to a single entry. The map
    /// always has a value for every distinct input id.
    async fn get_type_schemas(
        &self,
        type_ids: Vec<String>,
    ) -> HashMap<String, Result<GtsTypeSchema, TypesRegistryError>>;

    /// Retrieve multiple type-schemas by deterministic UUID v5 in one call.
    ///
    /// Same per-key semantics as [`Self::get_type_schemas`]: a map keyed
    /// by the input UUIDs, per-item failures carried in the inner
    /// `Result`. Duplicates collapse.
    async fn get_type_schemas_by_uuid(
        &self,
        type_uuids: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsTypeSchema, TypesRegistryError>>;

    /// List registered GTS type-schemas matching the query.
    async fn list_type_schemas(
        &self,
        query: TypeSchemaQuery,
    ) -> Result<Vec<GtsTypeSchema>, TypesRegistryError>;

    // ------------------------------------------------------------------
    // Instance operations (internal — no tenant scoping).
    // ------------------------------------------------------------------

    /// Register GTS instances in batch.
    ///
    /// Each input value must have a GTS id that does NOT end with `~`. Inputs
    /// whose identifier does not match the instance kind are returned as
    /// per-item `RegisterResult::Err` with `InvalidGtsInstanceId`. In ready
    /// phase, items whose declaring type-schema is not yet registered fail
    /// with `ParentTypeSchemaNotRegistered`.
    ///
    /// # Errors
    ///
    /// Returns `Err` only for catastrophic failures.
    async fn register_instances(
        &self,
        instances: Vec<serde_json::Value>,
    ) -> Result<Vec<RegisterResult>, TypesRegistryError>;

    /// Retrieve a registered GTS instance by its instance id.
    ///
    /// # Errors
    ///
    /// * `GtsInstanceNotFound` — no instance with this id is registered.
    /// * `InvalidGtsInstanceId` — id format is invalid, kind-mismatched, or
    ///   resolves to a non-instance entity.
    async fn get_instance(&self, id: &str) -> Result<GtsInstance, TypesRegistryError>;

    /// Retrieve a registered GTS instance by its deterministic UUID v5.
    ///
    /// # Errors
    ///
    /// * `GtsInstanceNotFound` — no instance is registered with this UUID
    ///   (also returned when the UUID exists but points to a type-schema).
    async fn get_instance_by_uuid(&self, uuid: Uuid) -> Result<GtsInstance, TypesRegistryError>;

    /// Retrieve multiple instances by id in one call.
    ///
    /// Returns a map keyed by the input ids; each value is a per-item
    /// `Result` carrying either the resolved instance or the per-item
    /// error ([`GtsInstanceNotFound`](TypesRegistryError::GtsInstanceNotFound),
    /// [`InvalidGtsInstanceId`](TypesRegistryError::InvalidGtsInstanceId),
    /// …). Duplicate ids in the input collapse to a single entry.
    async fn get_instances(
        &self,
        ids: Vec<String>,
    ) -> HashMap<String, Result<GtsInstance, TypesRegistryError>>;

    /// Retrieve multiple instances by deterministic UUID v5 in one call.
    ///
    /// Same per-key semantics as [`Self::get_instances`]: a map keyed by
    /// the input UUIDs, per-item failures carried in the inner `Result`.
    /// Duplicates collapse.
    async fn get_instances_by_uuid(
        &self,
        uuids: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsInstance, TypesRegistryError>>;

    /// List registered GTS instances matching the query.
    async fn list_instances(
        &self,
        query: InstanceQuery,
    ) -> Result<Vec<GtsInstance>, TypesRegistryError>;
}
