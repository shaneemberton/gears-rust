//! The Settings Service's reader as pricing's settings reads see it: it holds what each tenant
//! set, answers as told, and records what it was asked.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use async_trait::async_trait;
use serde_json::{Value, json};
use settings_service_sdk::models::{EffectiveValueResponse, GetEffectiveRequest};
use settings_service_sdk::precondition::SETTING_RETIRED;
use settings_service_sdk::{
    BulkOutcome, BulkSelector, EffectiveSource, SecretHandle, SecretString, SettingKey,
    SettingsReaderClient,
};
use toolkit::api::canonical_prelude::resource_error;
use toolkit::client_hub::ClientHub;
use toolkit_canonical_errors::CanonicalError;
use toolkit_db::Db;
use toolkit_security::SecurityContext;
use uuid::Uuid;

use crate::infra::settings_declarations::declarations;

#[resource_error(gts_id!("cf.core.settings.declaration.v1~"))]
struct DeclarationResource;

/// A way a read fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The Settings Service cannot answer now.
    Unavailable,
    /// The Settings Service failed.
    Internal,
    /// No declaration stands under the key.
    Undeclared,
    /// The declaration was retired.
    Retired,
}

impl Failure {
    fn error(self) -> CanonicalError {
        match self {
            Self::Unavailable => CanonicalError::service_unavailable()
                .with_detail("database unreachable")
                .create(),
            Self::Internal => CanonicalError::internal("the settings store failed").create(),
            Self::Undeclared => DeclarationResource::not_found("no declaration for key")
                .with_resource("setting")
                .create(),
            Self::Retired => DeclarationResource::failed_precondition()
                .with_precondition_violation("setting", "declaration was retired", SETTING_RETIRED)
                .create(),
        }
    }
}

/// One bulk read the double was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkRead {
    /// The keys asked for, as the Settings Service files them.
    pub keys: Vec<String>,
    pub scope: String,
    /// Who asked.
    pub subject_id: Uuid,
    pub subject_tenant_id: Uuid,
}

#[derive(Default)]
struct State {
    /// What was set, by scope and setting name.
    values: BTreeMap<(String, String), (Value, EffectiveSource)>,
    failing_call: Option<Failure>,
    failing_keys: BTreeMap<String, Failure>,
    left_out: BTreeSet<String>,
    silent: bool,
    bulk_reads: Vec<BulkRead>,
    single_reads: usize,
    reads_in_transaction: usize,
}

/// A Settings Service reader that knows pricing's declared settings and nothing else.
///
/// A setting nobody set answers its declared default, as the Settings Service does. Settings are
/// named by their leaf name (`default_gl`), a tenant by its id.
pub struct FakeSettingsReader {
    declared: BTreeMap<String, (SettingKey, Value)>,
    // The Settings Service takes a database connection on every read, which the database layer
    // refuses inside a transaction. With a handle the double refuses the same reads.
    guard: Option<Db>,
    state: Mutex<State>,
}

// A test that panicked while holding the lock has already failed; the record stays readable.
fn locked(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn scope_of(tenant_id: Uuid) -> String {
    format!("/tenants/{tenant_id}")
}

impl FakeSettingsReader {
    /// A reader over pricing's declarations.
    ///
    /// # Panics
    ///
    /// When a declared setting key does not parse, which is a defect in the gear.
    #[must_use]
    pub fn new() -> Self {
        let declared = declarations()
            .unwrap_or_else(|error| panic!("pricing's declarations do not build: {error}"))
            .into_iter()
            .map(|d| (d.key.leaf_slug().to_owned(), (d.key, d.default_value)))
            .collect();
        Self {
            declared,
            guard: None,
            state: Mutex::new(State::default()),
        }
    }

    /// Refuse, and count, every read made inside a transaction of `db`'s database layer.
    #[must_use]
    pub fn refusing_reads_in_transactions(mut self, db: Db) -> Self {
        self.guard = Some(db);
        self
    }

    /// Put the reader into `hub`, both as the Settings Service's reader and as itself, so a test
    /// holding only the hub finds it again.
    pub fn install(self, hub: &ClientHub) -> Arc<Self> {
        let reader = Arc::new(self);
        hub.register::<dyn SettingsReaderClient>(reader.clone());
        hub.register::<Self>(reader.clone());
        reader
    }

    /// The tenant sets `name` to `value`.
    pub fn set(&self, tenant_id: Uuid, name: &str, value: Value) {
        self.put(tenant_id, name, value, EffectiveSource::OwnOverride);
    }

    /// `name` reaches the tenant as `value` from an ancestor.
    pub fn inherit(&self, tenant_id: Uuid, name: &str, value: Value) {
        self.put(tenant_id, name, value, EffectiveSource::Inherited);
    }

    fn put(&self, tenant_id: Uuid, name: &str, value: Value, source: EffectiveSource) {
        assert!(
            self.declared.contains_key(name),
            "pricing declares no setting {name}"
        );
        locked(&self.state)
            .values
            .insert((scope_of(tenant_id), name.to_owned()), (value, source));
    }

    /// Every read fails as `failure` until `None` is given.
    pub fn fail_reads(&self, failure: Option<Failure>) {
        locked(&self.state).failing_call = failure;
    }

    /// The read of `name` alone fails as `failure`, inside an otherwise answered bulk read, until
    /// `None` is given.
    pub fn fail_key(&self, name: &str, failure: Option<Failure>) {
        let mut state = locked(&self.state);
        match failure {
            Some(failure) => state.failing_keys.insert(name.to_owned(), failure),
            None => state.failing_keys.remove(name),
        };
    }

    /// Whether a bulk read leaves `name` out of its answer, saying nothing about it.
    pub fn leave_out(&self, name: &str, left_out: bool) {
        let mut state = locked(&self.state);
        if left_out {
            state.left_out.insert(name.to_owned());
        } else {
            state.left_out.remove(name);
        }
    }

    /// Whether a read is never answered.
    pub fn never_answer(&self, silent: bool) {
        locked(&self.state).silent = silent;
    }

    /// The bulk reads sent, in the order they came.
    #[must_use]
    pub fn bulk_reads(&self) -> Vec<BulkRead> {
        locked(&self.state).bulk_reads.clone()
    }

    /// How many single-key reads were sent.
    #[must_use]
    pub fn single_reads(&self) -> usize {
        locked(&self.state).single_reads
    }

    /// How many reads came from inside a database transaction. Counted only by a reader built
    /// with [`Self::refusing_reads_in_transactions`].
    #[must_use]
    pub fn reads_in_transaction(&self) -> usize {
        locked(&self.state).reads_in_transaction
    }

    /// The full key the Settings Service files `name` under.
    ///
    /// # Panics
    ///
    /// When pricing declares no such setting.
    #[must_use]
    pub fn key_of(&self, name: &str) -> String {
        let Some((key, _)) = self.declared.get(name) else {
            panic!("pricing declares no setting {name}");
        };
        key.to_string()
    }

    /// Record the read and say how the call as a whole ends, if it does not reach the keys.
    fn admit(&self) -> Result<bool, CanonicalError> {
        let in_transaction = self.guard.as_ref().is_some_and(|db| db.conn().is_err());
        let mut state = locked(&self.state);
        if in_transaction {
            state.reads_in_transaction += 1;
            return Err(CanonicalError::internal(
                "a connection was requested inside a transaction",
            )
            .create());
        }
        if let Some(failure) = state.failing_call {
            return Err(failure.error());
        }
        Ok(state.silent)
    }

    fn answer(
        &self,
        key: &SettingKey,
        scope: &str,
    ) -> Result<EffectiveValueResponse, CanonicalError> {
        let name = key.leaf_slug();
        let state = locked(&self.state);
        if let Some(failure) = state.failing_keys.get(name) {
            return Err(failure.error());
        }
        let Some((declared, default)) = self.declared.get(name).filter(|(k, _)| k == key) else {
            return Err(Failure::Undeclared.error());
        };
        let (value, source) = state
            .values
            .get(&(scope.to_owned(), name.to_owned()))
            .cloned()
            .unwrap_or_else(|| (default.clone(), EffectiveSource::SchemaDefault));
        let source_scope = match source {
            EffectiveSource::OwnOverride => Some(scope.to_owned()),
            EffectiveSource::Inherited => Some("/".to_owned()),
            EffectiveSource::SchemaDefault => None,
        };
        Ok(EffectiveValueResponse {
            key: declared.clone(),
            scope: scope.to_owned(),
            value,
            source,
            source_scope,
            traits: json!({}),
            inheritance_trail: Vec::new(),
        })
    }
}

impl Default for FakeSettingsReader {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SettingsReaderClient for FakeSettingsReader {
    async fn get_effective(
        &self,
        _ctx: &SecurityContext,
        req: GetEffectiveRequest,
    ) -> Result<EffectiveValueResponse, CanonicalError> {
        locked(&self.state).single_reads += 1;
        if self.admit()? {
            return std::future::pending().await;
        }
        self.answer(&req.key, &req.scope)
    }

    async fn get_effective_bulk(
        &self,
        ctx: &SecurityContext,
        selector: BulkSelector,
        scope: String,
    ) -> Result<Vec<BulkOutcome>, CanonicalError> {
        let keys = match selector {
            BulkSelector::Keys(keys) => keys,
            BulkSelector::Category(_) => {
                return Err(CanonicalError::internal("pricing reads its settings by key").create());
            }
        };
        locked(&self.state).bulk_reads.push(BulkRead {
            keys: keys.iter().map(ToString::to_string).collect(),
            scope: scope.clone(),
            subject_id: ctx.subject_id(),
            subject_tenant_id: ctx.subject_tenant_id(),
        });
        if self.admit()? {
            return std::future::pending().await;
        }
        let left_out = locked(&self.state).left_out.clone();
        Ok(keys
            .into_iter()
            .filter(|key| !left_out.contains(key.leaf_slug()))
            .map(|key| BulkOutcome {
                result: self.answer(&key, &scope),
                key,
            })
            .collect())
    }

    async fn resolve_secret(
        &self,
        _ctx: &SecurityContext,
        _handle: SecretHandle,
    ) -> Result<SecretString, CanonicalError> {
        Err(CanonicalError::internal("pricing declares no secret setting").create())
    }
}
