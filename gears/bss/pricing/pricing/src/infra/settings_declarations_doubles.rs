//! The types registry and the Settings Service as the gear's startup sees them, each recording
//! what it was sent and answering as told.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use serde_json::Value;
use settings_service_sdk::models::{ContributedDeclaration, ContributionError, ReconcileResult};
use settings_service_sdk::{SettingKey, SettingsContributionClient};
use toolkit_canonical_errors::CanonicalError;
use toolkit_security::SecurityContext;
use types_registry_sdk::{
    GtsInstance, GtsTypeSchema, InstanceQuery, RegisterResult, TypeSchemaQuery, TypesRegistryClient,
};
use uuid::Uuid;

/// The reason a refusing Settings Service double gives.
pub const REFUSAL_CODE: &str = "VALUE_TYPE_UNKNOWN";

/// How a double answers what it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Everything sent is taken.
    Admits,
    /// The call succeeds and reports the first item sent as refused.
    RefusesFirst,
    /// The call itself fails.
    Fails,
}

// A test that panicked while holding the lock has already failed; the record stays readable.
fn locked<T>(record: &Mutex<T>) -> MutexGuard<'_, T> {
    record.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_used() -> CanonicalError {
    CanonicalError::internal("not used at pricing's startup").create()
}

/// A types registry that records every batch sent through `register`.
pub struct RecordingRegistry {
    answer: Answer,
    batches: Mutex<Vec<Vec<Value>>>,
}

impl RecordingRegistry {
    #[must_use]
    pub const fn answering(answer: Answer) -> Self {
        Self {
            answer,
            batches: Mutex::new(Vec::new()),
        }
    }

    /// The batches sent, in the order they came.
    #[must_use]
    pub fn batches(&self) -> Vec<Vec<Value>> {
        locked(&self.batches).clone()
    }
}

#[async_trait]
impl TypesRegistryClient for RecordingRegistry {
    async fn register(&self, entities: Vec<Value>) -> Result<Vec<RegisterResult>, CanonicalError> {
        let ids: Vec<String> = entities
            .iter()
            .map(|entity| entity["$id"].as_str().unwrap_or_default().to_owned())
            .collect();
        locked(&self.batches).push(entities);
        match self.answer {
            Answer::Admits => Ok(ids
                .into_iter()
                .map(|gts_id| RegisterResult::Ok { gts_id })
                .collect()),
            Answer::RefusesFirst => Ok(ids
                .into_iter()
                .enumerate()
                .map(|(index, gts_id)| {
                    if index == 0 {
                        RegisterResult::Err {
                            gts_id: Some(gts_id),
                            error: CanonicalError::internal("the schema is refused").create(),
                        }
                    } else {
                        RegisterResult::Ok { gts_id }
                    }
                })
                .collect()),
            Answer::Fails => Err(CanonicalError::service_unavailable()
                .with_detail("the registry is down")
                .create()),
        }
    }
    async fn register_type_schemas(
        &self,
        _type_schemas: Vec<Value>,
    ) -> Result<Vec<RegisterResult>, CanonicalError> {
        Err(not_used())
    }
    async fn get_type_schema(&self, _type_id: &str) -> Result<GtsTypeSchema, CanonicalError> {
        Err(not_used())
    }
    async fn get_type_schema_by_uuid(
        &self,
        _type_uuid: Uuid,
    ) -> Result<GtsTypeSchema, CanonicalError> {
        Err(not_used())
    }
    async fn get_type_schemas(
        &self,
        _type_ids: Vec<String>,
    ) -> HashMap<String, Result<GtsTypeSchema, CanonicalError>> {
        HashMap::new()
    }
    async fn get_type_schemas_by_uuid(
        &self,
        _type_uuids: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsTypeSchema, CanonicalError>> {
        HashMap::new()
    }
    async fn list_type_schemas(
        &self,
        _query: TypeSchemaQuery,
    ) -> Result<Vec<GtsTypeSchema>, CanonicalError> {
        Err(not_used())
    }
    async fn register_instances(
        &self,
        _instances: Vec<Value>,
    ) -> Result<Vec<RegisterResult>, CanonicalError> {
        Err(not_used())
    }
    async fn get_instance(&self, _id: &str) -> Result<GtsInstance, CanonicalError> {
        Err(not_used())
    }
    async fn get_instance_by_uuid(&self, _uuid: Uuid) -> Result<GtsInstance, CanonicalError> {
        Err(not_used())
    }
    async fn get_instances(
        &self,
        _ids: Vec<String>,
    ) -> HashMap<String, Result<GtsInstance, CanonicalError>> {
        HashMap::new()
    }
    async fn get_instances_by_uuid(
        &self,
        _uuids: Vec<Uuid>,
    ) -> HashMap<Uuid, Result<GtsInstance, CanonicalError>> {
        HashMap::new()
    }
    async fn list_instances(
        &self,
        _query: InstanceQuery,
    ) -> Result<Vec<GtsInstance>, CanonicalError> {
        Err(not_used())
    }
}

/// One reconcile a [`RecordingSettings`] was sent.
#[derive(Debug, Clone)]
pub struct Reconcile {
    pub owner_module: String,
    pub declarations: Vec<ContributedDeclaration>,
}

/// A Settings Service that records every reconcile sent to it.
pub struct RecordingSettings {
    answer: Answer,
    reconciles: Mutex<Vec<Reconcile>>,
}

impl RecordingSettings {
    #[must_use]
    pub const fn answering(answer: Answer) -> Self {
        Self {
            answer,
            reconciles: Mutex::new(Vec::new()),
        }
    }

    /// The reconciles sent, in the order they came.
    #[must_use]
    pub fn reconciles(&self) -> Vec<Reconcile> {
        locked(&self.reconciles).clone()
    }
}

#[async_trait]
impl SettingsContributionClient for RecordingSettings {
    async fn register_declarations(
        &self,
        _ctx: &SecurityContext,
        owner_module: String,
        declarations: Vec<ContributedDeclaration>,
    ) -> Result<ReconcileResult, CanonicalError> {
        let sent = declarations.len();
        let first = declarations.first().map(|first| first.key.to_string());
        locked(&self.reconciles).push(Reconcile {
            owner_module,
            declarations,
        });
        match self.answer {
            Answer::Admits => Ok(ReconcileResult {
                registered: sent,
                ..ReconcileResult::default()
            }),
            // A refusal comes back inside a successful answer, one entry per refused key.
            Answer::RefusesFirst => Ok(ReconcileResult {
                registered: sent.saturating_sub(1),
                errors: first
                    .into_iter()
                    .map(|key| ContributionError {
                        key,
                        code: REFUSAL_CODE.to_owned(),
                        message: "no type is registered under the id".to_owned(),
                    })
                    .collect(),
                ..ReconcileResult::default()
            }),
            Answer::Fails => Err(CanonicalError::service_unavailable()
                .with_detail("database unreachable")
                .create()),
        }
    }

    async fn retire_declarations(
        &self,
        _ctx: &SecurityContext,
        _owner_module: String,
        _keys: Vec<SettingKey>,
    ) -> Result<ReconcileResult, CanonicalError> {
        Err(not_used())
    }
}
