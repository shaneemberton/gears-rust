//! The tenant pricing settings at startup.
//!
//! The gear's place in the start order; that a refused declaration or an absent Settings Service
//! fails its start; and the gear itself over the real types registry and the real Settings
//! Service: `init` registers the value types, `post_init` the declarations, a tenant that stored
//! nothing reads each declared default, and the service's validator judges values by the
//! registered types.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use bss_pricing::domain::resolve::TenantDefaults;
use bss_pricing::domain::tenant_settings::{TenantSettings, TenantSettingsSource};
use bss_pricing::infra::reference_ticker::system_actor;
use bss_pricing::infra::settings_declarations::doubles::{
    Answer, REFUSAL_CODE, RecordingRegistry, RecordingSettings,
};
use bss_pricing::infra::settings_declarations::{
    declarations, register_value_types, value_type_schemas,
};
use bss_pricing::infra::tenant_settings::SettingsServiceSource;
use bss_pricing::module::BssPricingGear;
use serde_json::{Value, json};
use settings_service::SettingsService;
use settings_service::domain::category::DomainVisibility;
use settings_service::domain::ports::StepUpOperation;
use settings_service::domain::validation::TypeValidator as _;
use settings_service::domain::writes::service::StepUpPolicy;
use settings_service::domain::writes::{ABSENT_VALUE_TAG, Change, WriteActor};
use settings_service::infra::type_validator::GtsTypeValidator;
use settings_service_sdk::models::{GetEffectiveRequest, ReconcileResult};
use settings_service_sdk::{
    EffectiveSource, SettingKey, SettingsContributionClient, SettingsReaderClient,
};
use tenant_resolver_sdk::{
    GetAncestorsOptions, GetAncestorsResponse, GetDescendantsOptions, GetDescendantsResponse,
    GetTenantsOptions, IsAncestorOptions, TenantId, TenantInfo, TenantRef, TenantResolverClient,
    TenantResolverError, TenantStatus,
};
use tokio_util::sync::CancellationToken;
use toolkit::config::ConfigProvider;
use toolkit::contracts::{DatabaseCapability, SystemCapability};
use toolkit::gts_publication::{GtsPublicationCapability as _, PreparedGtsPublication};
use toolkit::registry::GearRegistry;
use toolkit::runtime::{GearManager, GrpcInstallerStore, SystemContext};
use toolkit::{ClientHub, Gear, GearCtx};
use toolkit_canonical_errors::CanonicalError;
use toolkit_db::migration_runner::run_migrations_for_testing;
use toolkit_db::{ConnectOpts, DBProvider, DbError, connect_db};
use toolkit_security::SecurityContext;
use types_registry::gear::TypesRegistryGear;
use types_registry_sdk::TypesRegistryClient;
use uuid::Uuid;

mod common;

const PRICING: &str = "bss-pricing";
const SETTINGS: &str = "settings-service";
const REGISTRY: &str = "types-registry";

/// The full key of the setting named `name`.
fn key_of(name: &str) -> String {
    format!("gts.cf.core.settings.setting_type.v1~cf.bss.pricing.{name}.v1~")
}

/// Each declared setting with the value a tenant that stored nothing reads.
fn defaults() -> Vec<(&'static str, Value)> {
    vec![
        ("default_timing", json!("advance")),
        ("default_gl", json!("")),
        ("default_tax_category", json!("")),
        ("invoice_line_template_recurring", json!("")),
        ("invoice_line_template_usage", json!("")),
        ("invoice_line_template_one_time", json!("")),
        ("invoice_line_template_bundle", json!("")),
        ("offered_currencies", json!([])),
    ]
}

/// A config provider answering every gear with an empty section.
struct EmptySections(Value);

impl ConfigProvider for EmptySections {
    fn get_gear_config(&self, _gear_name: &str) -> Option<&Value> {
        Some(&self.0)
    }
}

fn empty_sections() -> Arc<EmptySections> {
    Arc::new(EmptySections(json!({ "config": {} })))
}

// ── The gear's place in the start order ─────────────────────────────────────

/// The gear as `#[toolkit::gear]` registers it, with the gears it names linked in.
#[test]
fn the_gear_starts_after_the_settings_service() {
    let registry = GearRegistry::discover_and_build().expect("the linked gears form a start order");
    let position = |name: &str| {
        registry
            .gears()
            .iter()
            .position(|gear| gear.name() == name)
            .unwrap_or_else(|| panic!("`{name}` is not registered"))
    };
    let pricing = &registry.gears()[position(PRICING)];
    assert!(
        pricing.deps().contains(&SETTINGS),
        "`{PRICING}` names {:?} and not `{SETTINGS}`: the contribution client it resolves at \
         startup is bound by that gear's init",
        pricing.deps()
    );
    assert!(position(SETTINGS) < position(PRICING));
    assert!(
        pricing.deps().contains(&REGISTRY),
        "the registry the value types go to stays a dependency"
    );
}

// ── A start that must fail, over doubles ────────────────────────────────────

/// A policy decision point that is down: startup asks it nothing.
struct UnreachablePdp;

#[async_trait]
impl authz_resolver_sdk::AuthZResolverApi for UnreachablePdp {
    async fn evaluate(
        &self,
        _ctx: toolkit_security::PlatformSecurityContext,
        _request: authz_resolver_sdk::EvaluationRequest,
    ) -> Result<authz_resolver_sdk::EvaluationResponse, CanonicalError> {
        Err(CanonicalError::service_unavailable().create())
    }
}

/// A pricing gear not yet started, in a context whose types registry admits everything and
/// whose Settings Service is `settings`, or is not there.
async fn gear_with(settings: Option<Answer>) -> (BssPricingGear, GearCtx) {
    let hub = Arc::new(ClientHub::new());
    hub.register::<dyn authz_resolver_sdk::AuthZResolverApi>(Arc::new(UnreachablePdp));
    hub.register::<dyn TypesRegistryClient>(Arc::new(RecordingRegistry::answering(Answer::Admits)));
    if let Some(answer) = settings {
        hub.register::<dyn SettingsContributionClient>(Arc::new(RecordingSettings::answering(
            answer,
        )));
    }
    let db = common::migrated_db().await.expect("a migrated database");
    (BssPricingGear::default(), context(PRICING, &hub, db))
}

#[tokio::test]
async fn the_gear_does_not_start_when_a_declaration_is_refused() {
    let (gear, ctx) = gear_with(Some(Answer::RefusesFirst)).await;
    gear.init(&ctx).await.expect("init");
    let refused = gear
        .post_init(&ctx)
        .await
        .expect_err("a refusal reported inside a successful reconcile still fails the start");
    assert!(
        format!("{refused:#}").contains(REFUSAL_CODE),
        "the failure carries the reason: {refused:#}"
    );
}

#[tokio::test]
async fn the_gear_does_not_start_without_the_settings_service() {
    let (gear, ctx) = gear_with(None).await;
    gear.init(&ctx).await.expect("init");
    let refused = gear
        .post_init(&ctx)
        .await
        .expect_err("no settings client is registered");
    assert!(
        format!("{refused:#}").contains("SettingsContributionClient"),
        "the failure names what is missing: {refused:#}"
    );
}

// ── The gear over the real registry and the real Settings Service ───────────

fn system() -> SystemContext {
    SystemContext::new(
        Uuid::new_v4(),
        Arc::new(GearManager::new()),
        Arc::new(GrpcInstallerStore::new()),
    )
}

async fn in_memory(
    migrations: Vec<Box<dyn sea_orm_migration::MigrationTrait>>,
) -> DBProvider<DbError> {
    let db = connect_db(
        "sqlite::memory:",
        ConnectOpts {
            max_conns: Some(1),
            min_conns: Some(1),
            ..ConnectOpts::default()
        },
    )
    .await
    .expect("an in-memory database");
    run_migrations_for_testing(&db, migrations)
        .await
        .expect("the migrations apply");
    DBProvider::new(db)
}

fn context(gear: &'static str, hub: &Arc<ClientHub>, db: DBProvider<DbError>) -> GearCtx {
    GearCtx::new(
        gear,
        Uuid::new_v4(),
        empty_sections(),
        Arc::clone(hub),
        CancellationToken::new(),
    )
    .with_db(db)
}

/// A platform of two tenants: the root and one tenant directly under it.
struct TwoTenants {
    root: Uuid,
    tenant: Uuid,
}

impl TwoTenants {
    fn reference(&self, id: Uuid) -> Result<TenantRef, TenantResolverError> {
        let parent_id = if id == self.root {
            None
        } else if id == self.tenant {
            Some(TenantId(self.root))
        } else {
            return Err(TenantResolverError::TenantNotFound {
                tenant_id: TenantId(id),
            });
        };
        Ok(TenantRef {
            id: TenantId(id),
            status: TenantStatus::Active,
            tenant_type: None,
            parent_id,
            self_managed: false,
        })
    }

    fn info(&self, id: Uuid) -> Result<TenantInfo, TenantResolverError> {
        let reference = self.reference(id)?;
        Ok(TenantInfo {
            id: reference.id,
            name: id.to_string(),
            status: reference.status,
            tenant_type: None,
            parent_id: reference.parent_id,
            self_managed: false,
        })
    }
}

#[async_trait]
impl TenantResolverClient for TwoTenants {
    async fn get_tenant(
        &self,
        _ctx: &SecurityContext,
        id: TenantId,
    ) -> Result<TenantInfo, TenantResolverError> {
        self.info(id.0)
    }

    async fn get_root_tenant(
        &self,
        _ctx: &SecurityContext,
    ) -> Result<TenantInfo, TenantResolverError> {
        self.info(self.root)
    }

    async fn get_tenants(
        &self,
        _ctx: &SecurityContext,
        ids: &[TenantId],
        _options: &GetTenantsOptions,
    ) -> Result<Vec<TenantInfo>, TenantResolverError> {
        Ok(ids.iter().filter_map(|id| self.info(id.0).ok()).collect())
    }

    async fn get_ancestors(
        &self,
        _ctx: &SecurityContext,
        id: TenantId,
        _options: &GetAncestorsOptions,
    ) -> Result<GetAncestorsResponse, TenantResolverError> {
        let tenant = self.reference(id.0)?;
        let ancestors = match tenant.parent_id {
            Some(parent) => vec![self.reference(parent.0)?],
            None => Vec::new(),
        };
        Ok(GetAncestorsResponse { tenant, ancestors })
    }

    async fn get_descendants(
        &self,
        _ctx: &SecurityContext,
        id: TenantId,
        _options: &GetDescendantsOptions,
    ) -> Result<GetDescendantsResponse, TenantResolverError> {
        let tenant = self.reference(id.0)?;
        let descendants = if id.0 == self.root {
            vec![self.reference(self.tenant)?]
        } else {
            Vec::new()
        };
        Ok(GetDescendantsResponse {
            tenant,
            descendants,
        })
    }

    async fn is_ancestor(
        &self,
        _ctx: &SecurityContext,
        ancestor_id: TenantId,
        descendant_id: TenantId,
        _options: &IsAncestorOptions,
    ) -> Result<bool, TenantResolverError> {
        Ok(ancestor_id.0 == self.root && descendant_id.0 == self.tenant)
    }
}

/// The types registry and the Settings Service as a host has them when pricing's `init` runs:
/// the registry initialized, the Settings Service's own types admitted and its clients bound.
struct Platform {
    hub: Arc<ClientHub>,
    registry: TypesRegistryGear,
    settings: SettingsService,
}

impl Platform {
    /// A started platform in which `tenant` sits directly under the root tenant.
    async fn started_with(tenant: Uuid) -> Self {
        let hub = Arc::new(ClientHub::new());
        hub.register::<dyn TenantResolverClient>(Arc::new(TwoTenants {
            root: Uuid::new_v4(),
            tenant,
        }));

        let registry = TypesRegistryGear::default();
        let ctx = context(REGISTRY, &hub, in_memory(registry.migrations()).await);
        registry.init(&ctx).await.expect("types-registry init");

        // The runtime admits a gear's publication manifest before any application `init`.
        let settings = SettingsService::default();
        let ctx = context(SETTINGS, &hub, in_memory(settings.migrations()).await);
        PreparedGtsPublication::prepare(settings.descriptor(), ctx.config_provider(), &hub)
            .expect("the Settings Service's manifest is prepared")
            .publish(ctx.cancellation_token(), Arc::new(|_: &str| {}))
            .await
            .expect("the registry admits the Settings Service's own types");

        hub.register::<dyn credstore_sdk::CredStoreClientV1>(Arc::new(
            credstore_sdk::test_util::MockCredStoreClient::empty(),
        ));
        settings.init(&ctx).await.expect("settings-service init");

        Self {
            hub,
            registry,
            settings,
        }
    }

    /// `tenant` stores `value` under the setting `name`, through the Settings Service's own write
    /// path: its gates, its validator and the eviction of what its reader cached. Who may write
    /// and the step-up are decided at the service's REST door, before this path, so they are not
    /// asked here.
    async fn store(&self, tenant: Uuid, name: &str, value: Value) {
        let writes = self.settings.writes().expect("the write coordinator");
        let db = self.settings.db().expect("the Settings Service's database");
        let actor = WriteActor {
            ctx: SecurityContext::builder()
                .subject_id(Uuid::new_v4())
                .subject_tenant_id(tenant)
                .subject_type("user")
                .build()
                .expect("an administrator of the tenant"),
            request_id: Uuid::new_v4().to_string(),
            step_up_token: None,
            visibility: DomainVisibility::Unrestricted,
        };
        let key = SettingKey::parse(&key_of(name)).expect("a setting key");
        let gated = writes
            .writer()
            .gate(
                &db.conn().expect("a connection"),
                &actor,
                &key,
                Some(tenant),
                StepUpPolicy::AlreadyVerified,
                StepUpOperation::Set,
            )
            .await
            .unwrap_or_else(|error| panic!("{name} may be written: {error:?}"));
        writes
            .commit(
                &actor,
                gated,
                Change::Set(value),
                Some(ABSENT_VALUE_TAG),
                Uuid::new_v4(),
            )
            .await
            .unwrap_or_else(|error| panic!("{name} is stored: {error:?}"));
    }

    /// What the runtime does between the last application `init` and the first `post_init`.
    async fn finalize_what_init_registered(&self) -> anyhow::Result<()> {
        self.registry
            .legacy_finalize_after_application_init(&system())
            .await
    }
}

/// The value types the gear publishes, each with its schema.
fn published_value_types() -> Vec<(String, Value)> {
    let types: Vec<(String, Value)> = value_type_schemas()
        .into_iter()
        .map(|schema| {
            let id = schema["$id"].as_str().expect("a schema names itself");
            (id.trim_start_matches("gts://").to_owned(), schema)
        })
        .collect();
    assert!(!types.is_empty(), "the gear publishes its value types");
    types
}

fn effective(name: &str, tenant: Uuid) -> GetEffectiveRequest {
    GetEffectiveRequest {
        key: SettingKey::parse(&key_of(name)).expect("a setting key"),
        scope: format!("/tenants/{tenant}"),
    }
}

/// The gear itself, started as a host starts it: `init`, the registry's finalize, `post_init`.
#[tokio::test]
async fn the_gear_declares_its_settings_in_the_real_settings_service() {
    let tenant = Uuid::new_v4();
    let platform = Platform::started_with(tenant).await;
    let registry = platform
        .hub
        .get::<dyn TypesRegistryClient>()
        .expect("the registry client is bound");
    let contributions = platform
        .hub
        .get::<dyn SettingsContributionClient>()
        .expect("the contribution client is bound");
    let reader = platform
        .hub
        .get::<dyn SettingsReaderClient>()
        .expect("the reader is bound");
    let actor = system_actor(tenant).expect("the gear's own actor");

    platform
        .hub
        .register::<dyn authz_resolver_sdk::AuthZResolverApi>(Arc::new(UnreachablePdp));
    let pricing = BssPricingGear::default();
    let ctx = context(
        PRICING,
        &platform.hub,
        common::migrated_db().await.expect("a migrated database"),
    );

    // `init` declares nothing: the value types it registers are not served yet.
    pricing.init(&ctx).await.expect("pricing init");
    for (name, _) in defaults() {
        let undeclared = reader
            .get_effective(&actor, effective(name, tenant))
            .await
            .expect_err("nothing is declared before post_init");
        assert!(
            matches!(undeclared, CanonicalError::NotFound { .. }),
            "{name}: {undeclared}"
        );
    }

    // The registry takes each schema as it is: the enumeration, the length bounds, the
    // patterns and the uniqueness.
    platform
        .finalize_what_init_registered()
        .await
        .expect("the registry admits the value types when it finalizes");
    for (type_id, published) in published_value_types() {
        let stored = registry
            .get_type_schema(&type_id)
            .await
            .unwrap_or_else(|error| panic!("{type_id} is in the registry: {error}"));
        assert_eq!(
            stored.raw_schema, published,
            "{type_id}: the registry holds the schema the gear published"
        );
    }

    // `post_init` declares: a tenant that stored nothing reads each declared default.
    pricing.post_init(&ctx).await.expect("pricing post_init");
    for (name, default) in defaults() {
        let read = reader
            .get_effective(&actor, effective(name, tenant))
            .await
            .unwrap_or_else(|error| panic!("{name} is readable: {error}"));
        assert_eq!(read.source, EffectiveSource::SchemaDefault, "{name}");
        assert_eq!(read.value, default, "{name}");
    }

    // A start over what is already declared succeeds and leaves it as it is: the same set,
    // reconciled by hand afterwards, has nothing to register, update, retire or refuse.
    pricing
        .post_init(&ctx)
        .await
        .expect("post_init over the gear's own declarations");
    let again = contributions
        .register_declarations(
            &actor,
            PRICING.to_owned(),
            declarations().expect("every key parses"),
        )
        .await
        .expect("the reconcile runs");
    assert_eq!(again, ReconcileResult::default());
}

/// The read every pricing decision makes, against the real Settings Service: its bulk read knows
/// the declared keys, answers their defaults for a tenant that stored nothing, and answers what
/// the tenant stores, to that tenant alone.
#[tokio::test]
async fn the_settings_source_reads_the_real_settings_service() {
    let (tenant, other) = (Uuid::new_v4(), Uuid::new_v4());
    let platform = Platform::started_with(tenant).await;
    platform
        .hub
        .register::<dyn authz_resolver_sdk::AuthZResolverApi>(Arc::new(UnreachablePdp));
    let pricing = BssPricingGear::default();
    let ctx = context(
        PRICING,
        &platform.hub,
        common::migrated_db().await.expect("a migrated database"),
    );
    let source = SettingsServiceSource::new(Arc::clone(&platform.hub));

    // Before the declarations are there the settings are unavailable, not defaults.
    pricing.init(&ctx).await.expect("pricing init");
    platform
        .finalize_what_init_registered()
        .await
        .expect("the registry admits the value types when it finalizes");
    source
        .effective(tenant)
        .await
        .expect_err("nothing is declared before post_init");

    pricing.post_init(&ctx).await.expect("pricing post_init");
    let nothing_set = TenantSettings {
        defaults: TenantDefaults {
            default_timing: "advance".to_owned(),
            default_gl: None,
            default_tax_category: None,
            invoice_line_templates: BTreeMap::new(),
        },
        offered_currencies: Vec::new(),
    };
    assert_eq!(
        source
            .effective(tenant)
            .await
            .expect("the declared defaults are read"),
        nothing_set
    );

    platform
        .store(tenant, "default_timing", json!("arrears"))
        .await;
    platform.store(tenant, "default_gl", json!("9000")).await;
    platform
        .store(tenant, "invoice_line_template_usage", json!("{sku} usage"))
        .await;
    platform
        .store(tenant, "offered_currencies", json!(["EUR", "USD"]))
        .await;
    assert_eq!(
        source
            .effective(tenant)
            .await
            .expect("the stored values are read"),
        TenantSettings {
            defaults: TenantDefaults {
                default_timing: "arrears".to_owned(),
                default_gl: Some("9000".to_owned()),
                default_tax_category: None,
                invoice_line_templates: BTreeMap::from([(
                    "usage".to_owned(),
                    "{sku} usage".to_owned()
                )]),
            },
            offered_currencies: vec!["EUR".to_owned(), "USD".to_owned()],
        }
    );

    // The settings are local: what one tenant stores reaches no other.
    assert_eq!(
        source
            .effective(other)
            .await
            .expect("another tenant reads the declared defaults"),
        nothing_set
    );
}

/// The Settings Service's own validator over the registered types: what a write of a value
/// would be judged by.
#[tokio::test]
async fn the_settings_service_validator_judges_values_by_the_registered_types() {
    let platform = Platform::started_with(Uuid::new_v4()).await;
    let registry = platform
        .hub
        .get::<dyn TypesRegistryClient>()
        .expect("the registry client is bound");
    register_value_types(registry.as_ref())
        .await
        .expect("the registry takes the value types");
    platform
        .finalize_what_init_registered()
        .await
        .expect("the registry admits the value types when it finalizes");
    let validator = GtsTypeValidator::new(registry);

    let cases = [
        (
            "default_timing",
            vec![json!("advance"), json!("arrears")],
            vec![json!("monthly"), json!("")],
        ),
        (
            "default_gl",
            vec![json!(""), json!("4000-REV")],
            vec![json!("x".repeat(65))],
        ),
        (
            "invoice_line_template_usage",
            vec![json!(""), json!("Usage"), json!("{sku} for {period}")],
            vec![
                json!("Charge {foo}"),
                json!("}"),
                json!("{sku"),
                json!("{{sku}}"),
                json!("x".repeat(2001)),
            ],
        ),
        (
            "offered_currencies",
            vec![json!([]), json!(["USD", "EUR"])],
            vec![json!(["usd"]), json!(["USD", "USD"])],
        ),
    ];
    let declared = declarations().expect("every key parses");
    for (name, admitted, refused) in cases {
        let key = key_of(name);
        let type_id = &declared
            .iter()
            .find(|declaration| declaration.key.as_str() == key)
            .unwrap_or_else(|| panic!("`{name}` is declared"))
            .value_type_id;
        for value in admitted {
            let judged = validator
                .validate_value(type_id, &value)
                .await
                .unwrap_or_else(|error| panic!("{name}: {value} is judged: {error:?}"));
            assert_eq!(judged.violations, Vec::new(), "{name}: {value}");
        }
        for value in refused {
            let judged = validator
                .validate_value(type_id, &value)
                .await
                .unwrap_or_else(|error| panic!("{name}: {value} is judged: {error:?}"));
            assert!(!judged.violations.is_empty(), "{name}: {value} is refused");
        }
    }
}
