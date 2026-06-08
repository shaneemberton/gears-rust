# Usage Collector Plugin SPI Reference

<!-- toc -->

- [Overview](#overview)
- [Scope](#scope)
  - [In scope](#in-scope)
  - [Out of scope](#out-of-scope)
- [ToolKit Plugin SPI placement](#toolkit-plugin-spi-placement)
  - [Crate layout](#crate-layout)
  - [Trait declaration shape](#trait-declaration-shape)
  - [Two-trait split](#two-trait-split)
- [Plugin registration and discovery](#plugin-registration-and-discovery)
  - [GTS spec](#gts-spec)
  - [Plugin-side `init()` flow](#plugin-side-init-flow)
  - [Host-side resolution flow](#host-side-resolution-flow)
  - [Compile-time linkage](#compile-time-linkage)
  - [Vendor + priority selection](#vendor--priority-selection)
- [Domain Model](#domain-model)
  - [Core ingestion and identity types](#core-ingestion-and-identity-types)
  - [Query types and views](#query-types-and-views)
  - [Plugin-specific outcome types](#plugin-specific-outcome-types)
  - [Trace context propagation](#trace-context-propagation)
  - [Cross-entity invariants honored by the Plugin SPI](#cross-entity-invariants-honored-by-the-plugin-spi)
- [Public Plugin SPI Trait](#public-plugin-spi-trait)
- [Method Contracts](#method-contracts)
  - [Method 1 — Persist single usage record](#method-1--persist-single-usage-record)
  - [Method 2 — Persist batched usage records](#method-2--persist-batched-usage-records)
  - [Method 3 — Aggregated query](#method-3--aggregated-query)
  - [Method 4 — Raw keyset-paginated query](#method-4--raw-keyset-paginated-query)
  - [Method 5 — Deactivate usage event](#method-5--deactivate-usage-event)
  - [Method 6 — Register metric type](#method-6--register-metric-type)
  - [Method 7 — Read metric type](#method-7--read-metric-type)
  - [Method 8 — List metric types](#method-8--list-metric-types)
  - [Method 9 — Delete metric type](#method-9--delete-metric-type)
- [Catalog and validation surface](#catalog-and-validation-surface)
- [Data Model](#data-model)
  - [Table: metric_catalog](#table-metric_catalog)
  - [Table: usage_records](#table-usage_records)
- [Contract Tests](#contract-tests)
  - [`spi-contract-test-deactivate-cascade-usage`](#spi-contract-test-deactivate-cascade-usage)
  - [`spi-contract-test-deactivate-cascade-compensation`](#spi-contract-test-deactivate-cascade-compensation)
  - [`spi-contract-test-counter-only-compensation`](#spi-contract-test-counter-only-compensation)
  - [`spi-contract-test-value-matrix`](#spi-contract-test-value-matrix)
  - [`spi-contract-test-aggregation-sum-nets-and-usage-only-others`](#spi-contract-test-aggregation-sum-nets-and-usage-only-others)
- [Error Taxonomy](#error-taxonomy)
- [Consistency profile](#consistency-profile)
- [Versioning/Compatibility](#versioningcompatibility)
- [Exclusions/Non-goals](#exclusionsnon-goals)
  - [SDK-trait-only exclusions](#sdk-trait-only-exclusions)
  - [REST-only exclusions](#rest-only-exclusions)
  - [Gear non-goals reaffirmed on the Plugin SPI](#gear-non-goals-reaffirmed-on-the-plugin-spi)
- [Traceability](#traceability)
  - [Surface identifier and consumer contract](#surface-identifier-and-consumer-contract)
  - [Capabilities exposed by the Plugin SPI](#capabilities-exposed-by-the-plugin-spi)
  - [Domain entities](#domain-entities)
  - [Components allocated to the Plugin SPI](#components-allocated-to-the-plugin-spi)
  - [Persistence anchors](#persistence-anchors)
  - [Authorization, fail-closed, and attribution anchors (exclusions)](#authorization-fail-closed-and-attribution-anchors-exclusions)
  - [Versioning, stability, and quality NFR anchors](#versioning-stability-and-quality-nfr-anchors)
- [Open Questions](#open-questions)
- [Document Changelog](#document-changelog)

<!-- /toc -->

## Overview

The Usage Collector Plugin SPI is the in-process async Rust service
provider interface (SPI) that storage-backend authors implement so the
Usage Collector gear can persist, query, deactivate, and read usage
data without binding to any specific backend technology. The SPI is
the canonical realization of `cpt-cf-usage-collector-interface-plugin`
and the `cpt-cf-usage-collector-contract-storage-plugin` contract, and
it is the only path through which the Usage Collector's core reaches
durable state (per `cpt-cf-usage-collector-principle-pluggable-storage`
and `cpt-cf-usage-collector-adr-pluggable-storage`). Per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
(see [`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)),
this surface includes the Metric Catalog (managed via the Plugin SPI,
persisted in the active storage plugin's database): catalog rows live
alongside `usage_records` in the plugin backend and the
`usage_records → metric_catalog` reference is enforced by a real
in-database `ON DELETE RESTRICT` foreign key on the `gts_id` column.
The catalog carries the GTS-typed metric metadata schema per ADR 0012;
the gateway is the semantic owner of catalog operations (registration
API, PDP, validation, schema authority) and the plugin owns durable
storage and FK enforcement. The in-plugin reference scheme (column
type, index choice, or any other implementation choice) used to store
or look up `gts_id` is the plugin author's choice and is explicitly
out of SPI scope.

This document is the reference specification for the SPI trait. It
captures the operation set, method contracts (inputs, outputs, error
behaviour), domain types shared with the SDK trait, the SPI-only error
taxonomy, ToolKit crate placement, versioning and stability policy,
trace-context propagation requirements, and exclusions. The exact
Rust signature lives in `usage-collector-sdk/src/plugin_api.rs`;
this reference defines what every implementation and the calling
Plugin Host (`cpt-cf-usage-collector-component-plugin-host`) must
satisfy.

The Plugin SPI is one of three independently versioned public surfaces
described in DESIGN §3.3 — alongside `cpt-cf-usage-collector-interface-sdk-client`
(in-process SDK trait, see `sdk-trait.md`) and `cpt-cf-usage-collector-interface-rest-api`
(REST API, see `usage-collector-v1.yaml`). Each surface
evolves under the major-version stability contract anchored by
`cpt-cf-usage-collector-adr-contract-stability`,
`cpt-cf-usage-collector-principle-contract-stability`, and
`cpt-cf-usage-collector-nfr-plugin-contract-stability`.

Sources: DESIGN §1.2 Architecture Drivers (Plugin SPI driver rows for
`cpt-cf-usage-collector-fr-pluggable-storage`,
`cpt-cf-usage-collector-fr-query-aggregation`,
`cpt-cf-usage-collector-fr-query-raw`,
`cpt-cf-usage-collector-fr-event-deactivation`); §3.3 "Plugin SPI —
`cpt-cf-usage-collector-interface-plugin`"; §3.5 "Storage Plugin
Contract"; §3.6 sequences; §3.12.9 Package and Namespace Conventions.

## Scope

### In scope

The Plugin SPI realizes the following Usage Collector functional
capabilities at the persistence boundary:

- Durable persistence of single and batched `UsageRecord` submissions
  with caller-supplied idempotency keys, including dedup-on-conflict
  enforcement on the composite `(tenant_id, gts_id, idempotency_key)`
  (`cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-seq-emit-usage`).
- Server-side aggregated query execution with pushed-down SUM /
  COUNT / MIN / MAX / AVG and group-by, returning bucketed results
  (`cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-seq-query-aggregated`,
  `cpt-cf-usage-collector-nfr-query-latency`).
- Cursor-paginated raw record retrieval, including plugin-owned cursor
  token generation and validation
  (`cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-seq-query-raw`).
- Atomic one-way `active → inactive` deactivation of an individual
  `UsageRecord`
  (`cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-seq-deactivate-event`,
  `cpt-cf-usage-collector-adr-monotonic-deactivation`,
  `cpt-cf-usage-collector-principle-monotonic-deactivation`).
- Durable storage of the Metric Catalog
  (`cpt-cf-usage-collector-dbtable-metric-catalog`) alongside
  `usage_records`, with in-database `ON DELETE RESTRICT` referential
  integrity between `usage_records.gts_id` and `metric_catalog.gts_id`
  per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)).
  The SPI exposes registration, read-by-`gts_id`, list, and delete so
  the gateway can administer the catalog and hydrate its flat L1
  catalog cache `Map<gts_id, {kind, metadata_fields}>` (`kind` derived
  once on cache load from the `gts_id` prefix; `metadata_fields` lifted
  verbatim from the row). The catalog row carries `metadata_fields:
Vec<String>` (the closed, declared list of allowed metadata key
  names; all values typed as String end-to-end); the plugin does NOT
  validate the closed-shape membership rule itself — that hot path is
  gateway L1 per ADR 0012. The in-plugin reference scheme used by the
  storage plugin (column type, index choice, etc.) is the plugin
  author's choice and out of SPI scope.

The Plugin SPI does NOT expose a plugin-side readiness probe or
flush hook. Plugin availability is detected structurally by the
Plugin Host via `ClientHub::try_get_scoped` (an empty scoped slot
means the plugin gear has not yet registered or is gone); there
is no trait method that asks the plugin "are you ready?" and no
trait method that asks the plugin to drain. Graceful shutdown is
handled by the Plugin Host's process-level lifecycle, not by an SPI
call. See OQ-4 for the resolution.

Sources: DESIGN §3.3 Plugin SPI capability list; §3.6 sequences;
§3.11.5 Health-check endpoint paths.

### Out of scope

The Plugin SPI does not perform authentication, PDP authorization,
attribution validation, idempotency-key presence enforcement, kind
invariant enforcement against the Metric Catalog, metadata size
checking, PDP constraint composition, or any pricing / billing /
quota logic. Every call arrives at the SPI already authorized and
structurally validated by the core (`cpt-cf-usage-collector-component-ingestion-gateway`,
`cpt-cf-usage-collector-component-query-gateway`,
`cpt-cf-usage-collector-component-deactivation-handler`, and
`cpt-cf-usage-collector-component-metric-catalog`), each of which
performs PDP enforcement inside its own service via the shared
`authz_scope` helper (calling `PolicyEnforcer::access_scope_with`)
per `cpt-cf-usage-collector-principle-pdp-centric-authorization` and
`cpt-cf-usage-collector-principle-fail-closed`.

The SPI does not own REST wire shapes, OpenAPI generation, RFC-9457
`Problem` mapping, CORS, TLS termination, or output encoding; those
are platform API gateway and gear-REST-handler responsibilities.
The SPI also does not declare per-tenant access tables, role
matrices, or PDP-decision caching — gear-side caching of PDP
decisions is forbidden by `cpt-cf-usage-collector-principle-pdp-centric-authorization`.

Sources: DESIGN §3.2 "Plugin Host (ClientHub-bound)" Responsibility
boundaries; §3.3 capability scope notes; §3.9.6 Authorization
Architecture; §3.11.1 Performance Patterns / Caching.

## ToolKit Plugin SPI placement

### Crate layout

The Plugin SPI trait belongs in the Usage Collector's single
`usage-collector-sdk` crate alongside the consumer SDK trait, the
GTS spec for plugin discovery, the domain models, and the public
error enum, following the platform-standard `<gear>` +
`<gear>-sdk` two-crate layout documented in DESIGN §3.12.9 Package
and Namespace Conventions. There is no separate `-contracts` crate
and no separate `-plugin-api` crate. Required files under
`usage-collector-sdk/src/`, all transport-agnostic:

- `lib.rs` — crate root and re-exports.
- `api.rs` — public consumer SDK trait declaration
  (`UsageCollectorClientV1`, the subject of `sdk-trait.md`).
- `plugin_api.rs` — public Plugin SPI trait declaration
  (`UsageCollectorPluginV1`, this document's subject).
- `gts.rs` — GTS spec for plugin discovery and binding (reserved; populated by the plugin-registration step per DESIGN §3.12.9).
- `models.rs` — pure-data domain types (UsageRecord, Metric, queries,
  results, decisions, constraints) shared by both traits.
- `error.rs` — public, domain-classified error enum (see §"Error
  Taxonomy") surfaced through both the SDK trait and the Plugin SPI
  trait.

One concrete `usage-collector-plugin-<backend>` crate per backend
(for example `usage-collector-plugin-clickhouse`,
`usage-collector-plugin-timescaledb`) implements this trait and lives
under `gears/system/usage-collector/plugins/<backend>/`. Each
concrete-plugin crate depends on `usage-collector-sdk` only, never on
the host `usage-collector` crate, and is owned by the plugin's
authoring team.

Sources: DESIGN §3.12.9 "Cargo crate naming" two-crate layout;
§3.12.9 "Cross-gear imports" plugin-direction rule.

### Trait declaration shape

- The trait is declared `async` (via the `async_trait` pattern), is
  `Send + Sync + 'static`, and is used through ClientHub as a trait
  object.
- The canonical trait name is `UsageCollectorPluginV1`, mirroring the
  `UsageCollectorClientV1` naming used by the SDK trait per DESIGN
  §3.12.9 and the ToolKit naming convention that places the gear
  name and capability before the `V1` suffix. The `V1` suffix encodes
  the Plugin SPI's major version and aligns with the gear's
  major-version stability contract per
  `cpt-cf-usage-collector-adr-contract-stability` and
  `cpt-cf-usage-collector-nfr-plugin-contract-stability`.
- Every method takes `&self` as the receiver and accepts only the
  per-method domain inputs declared in §"Method Contracts" (see
  §"Trace context propagation" for the ambient-context model). Tracing
  is propagated via the ambient `tracing::Span` / OpenTelemetry context
  — no explicit `TraceContext` parameter is required (mirrors the
  reference plugin traits in `gears/credstore/credstore-sdk/src/plugin_api.rs:12-19`,
  `gears/system/authn-resolver/authn-resolver-sdk/src/plugin_api.rs:31-55`,
  and `gears/system/authz-resolver/authz-resolver-sdk/src/plugin_api.rs:19-22`,
  none of which carry a `TraceContext` parameter).
- The SPI does not accept a `SecurityContext` either, because
  authorization is already enforced upstream inside each domain
  component's `authz_scope` helper call per
  `cpt-cf-usage-collector-principle-pdp-centric-authorization`.
- Methods return a `Result` whose `Err` variant is the
  `UsageCollectorPluginError` enum declared in
  `usage-collector-sdk/src/error.rs` (see §"Error Taxonomy"); the
  `Ok` variant is the method-specific output type declared in
  `usage-collector-sdk/src/models.rs` or, for SPI-local outcome
  enums, in `usage-collector-sdk/src/plugin_api.rs`.
- The trait is registered into ClientHub with **GTS instance scope**
  by each `usage-collector-plugin-<backend>` crate's own
  `#[toolkit::gear]` `init()` (the host
  `cpt-cf-usage-collector-component-plugin-host` does not register
  scoped plugin clients itself). The Plugin Host resolves the bound
  instance lazily on the first dispatch call after the
  `types-registry` has become consistent — the host's
  `GtsPluginSelector` runs the `[usage_collector].vendor`-driven
  match against `TypesRegistryClient::list_instances` exactly once
  via `get_or_init`, then caches the resolved `GtsInstanceId` for the
  `Service`'s lifetime; subsequent dispatches reuse the cached id
  through `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>`
  under `ClientScope::gts_id(&instance_id)` per
  `cpt-cf-usage-collector-contract-gts-registry` and
  `cpt-cf-usage-collector-fr-pluggable-storage`. The
  `[usage_collector].vendor` value itself is read once in
  `Gear::init` via `ctx.config_or_default()?` and is never re-read
  at runtime (mirrors `gears/credstore/credstore/src/gear.rs:44-47`
  and `gears/credstore/credstore/src/domain/service.rs:53-75`);
  changing the binding requires a gear restart.

Sources: DESIGN §3.3 "Plugin SPI" technology row ("Async Rust SPI
trait registered in ClientHub with GTS instance scope"); §3.12.9
"Rust gear path stems" and "Type and trait naming"; §3.4 Internal
Dependencies (`gts-registry`); §3.2 "Plugin Host" Responsibility scope.

### Two-trait split

The public Plugin SPI trait, `UsageCollectorPluginV1`, is the
storage-backend-facing trait. The Usage Collector's separate public
SDK trait, `UsageCollectorClientV1`, is the consumer-facing trait
used by source gears and downstream readers and is described in
`sdk-trait.md`. Both traits live side by side in the single
`usage-collector-sdk` crate (`api.rs` for the consumer SDK trait,
`plugin_api.rs` for the Plugin SPI trait) and share the same
`models.rs` domain types and `error.rs` error enum — there is no
separate `-contracts` or `-plugin-api` crate.

Sources: DESIGN §3.12.9 "Cargo crate naming" two-crate layout; phase
mirror to `sdk-trait.md` §"Two-trait split".

## Plugin registration and discovery

The Plugin SPI is wired into the runtime through the platform-standard
`PluginV1<P>` GTS base type + `types-registry` + `ClientHub` scoped
registration pattern — the same pattern used by `credstore`,
`authn-resolver`, and `authz-resolver`. Per
`cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`
(DESIGN §2.1), the SDK declares a unit-struct GTS spec; each plugin
publishes a `PluginV1<UsageCollectorPluginSpecV1>` instance to
`types-registry` and registers a scoped `dyn UsageCollectorPluginV1`
client in `ClientHub`; the host's plugin component lazily resolves
the bound instance by GTS schema id + configured vendor and caches it
for per-request in-memory dispatch.

### GTS spec

The SDK declares the GTS spec for usage-collector plugins in
`usage-collector-sdk/src/gts.rs`:

```rust
use toolkit::gts::PluginV1;
use toolkit_gts::gts_type_schema;

#[derive(Default)]
#[gts_type_schema(
    dir_path = "schemas",
    base = PluginV1,
    schema_id = "gts.cf.toolkit.plugins.plugin.v1~cf.core.usage_collector.plugin.v1~",
    description = "Usage Collector plugin specification",
    properties = "",
)]
pub struct UsageCollectorPluginSpecV1;
```

The empty `properties = ""` is intentional — plugin instance metadata
(`vendor`, `priority`) is carried by the `PluginV1<P>` base type and
is not duplicated in usage-collector-specific spec data. The
`schema_id` `gts.cf.toolkit.plugins.plugin.v1~cf.core.usage_collector.plugin.v1~`
is the type identifier under which every concrete plugin instance is
registered with `types-registry`.

### Plugin-side `init()` flow

Each `usage-collector-plugin-<backend>` crate's `#[toolkit::gear]`
`init(...)` follows a four-step pattern: `build_registration` →
publish to `types-registry` → register the scoped client in
`ClientHub` → ready for dispatch.

```rust
let (instance_id, instance_json) = PluginV1::<UsageCollectorPluginSpecV1>::build_registration(
    "<vendor>.<package>.usage_collector_plugin.v1",
    cfg.vendor,
    cfg.priority,
)?;
let registry = ctx.client_hub().get::<dyn TypesRegistryClient>()?;
let results = registry.register(vec![instance_json]).await?;
RegisterResult::ensure_all_ok(&results)?;
let api: Arc<dyn UsageCollectorPluginV1> = service;
ctx.client_hub()
    .register_scoped::<dyn UsageCollectorPluginV1>(ClientScope::gts_id(&instance_id), api);
```

The final `GtsInstanceId` is
`UsageCollectorPluginSpecV1::SCHEMA_ID` concatenated with the supplied
instance segment (for example
`gts.cf.toolkit.plugins.plugin.v1~cf.core.usage_collector.plugin.v1~<vendor>.<package>.usage_collector_plugin.v1`).
The `RegisterResult::ensure_all_ok` gate enforces that the
`types-registry` accepted every payload before the `ClientHub`
scoped registration commits, so callers never see a half-registered
plugin.

### Host-side resolution flow

The Plugin Host (`cpt-cf-usage-collector-component-plugin-host`,
implemented in `usage-collector/src/domain/service.rs`) owns a
`GtsPluginSelector` that lazily resolves the bound plugin instance.
The resolution flow:

```rust
let plugin_type_id = UsageCollectorPluginSpecV1::gts_schema_id().clone();
let instances = registry
    .list_instances(InstanceQuery::new().with_pattern(format!("{plugin_type_id}*")))
    .await?;
let instance_id = choose_plugin_instance::<UsageCollectorPluginSpecV1>(
    &self.vendor,
    instances.iter().map(|e| (e.id.as_ref(), &e.object)),
)?;
let scope = ClientScope::gts_id(instance_id.as_ref());
let client = self.hub.try_get_scoped::<dyn UsageCollectorPluginV1>(&scope)?;
```

The selector caches the resolved `GtsInstanceId` for the `Service`'s
lifetime. Warm-path dispatch reuses the cached id and performs a
single `ClientHub::try_get_scoped` lookup; the `types-registry`
round-trip happens only on the cold path (first call after bootstrap).
When no matching instance is registered, the host returns the
`plugin-unavailable` outcome documented in §"Error Taxonomy"; the
same structural fact (selector cached AND `try_get_scoped is Some`)
drives the `usage_collector.plugin.ready` gauge surfaced via OTLP
per the foundation feature.

### Compile-time linkage

Plugins are statically linked at the workspace level — every
`usage-collector-plugin-<backend>` crate is compiled in as a Cargo
workspace member and registered with ToolKit via `#[toolkit::gear]`
at startup. The host `usage-collector` crate has **no compile-time
dependency** on any concrete `usage-collector-plugin-<backend>`
crate; binding is purely a runtime concern resolved through
`types-registry` + `ClientHub`. Adding or swapping a plugin is a
workspace-build + config-vendor change, not a host-crate change, and
no dynamic-loading (`dlopen`, `libloading`, …) machinery is involved.

### Vendor + priority selection

When multiple plugin instances are registered under the same
`UsageCollectorPluginSpecV1` schema id (for example a deployment that
ships both ClickHouse and TimescaleDB plugins), the host's configured
vendor (field on `usage-collector`'s `config.rs`, populated from the
`[usage_collector].vendor` configuration key) drives
`choose_plugin_instance`. Matching is exact on `PluginV1.vendor`;
ties are broken by the lowest `PluginV1.priority` (lower number =
higher priority, mirroring the `PluginV1<P>` contract documented at
`libs/toolkit-gts/src/plugin.rs`). The resolved instance is cached
for the `Service`'s lifetime; binding changes require a gear
restart. There is no parallel cache and no retain-prior fallback —
`ClientHub::register_scoped` is a plain `HashMap::insert` under a
`parking_lot::RwLock` (see `libs/toolkit/src/client_hub.rs:155-165`),
and the host service performs `try_get_scoped` per call: `None` is
lifted to a per-call `plugin-unavailable` error rather than
substituting a prior binding.

Sources: DESIGN §2.1
`cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`;
DESIGN §3.5 "Plugin Resolution and Dispatch"; DESIGN §3.3
"Startup-time plugin binding"; DESIGN §3.6 sequence diagrams (every
plugin-dispatching sequence threads through
`ClientHub::try_get_scoped`); DECOMPOSITION §4.3 "Plugin discovery
and dispatch"; reference gears `credstore`, `authn-resolver`, and
`authz-resolver` for the canonical pattern; `libs/toolkit-gts/src/plugin.rs`
for the `PluginV1<P>` base type and `build_registration` helper.

## Domain Model

The Plugin SPI operates exclusively on the canonical Usage Collector
domain types from `domain-model.md`. All domain types are declared in
`usage-collector-sdk/src/models.rs` and remain transport-agnostic. Field
names are snake_case; struct and enum names are UpperCamelCase.
Identifiers (`tenant_id`, `resource_id`, `subject_id`, `source_gear`,
`gts_id`) are opaque platform identifiers; the Usage Collector neither
parses nor classifies them per `cpt-cf-usage-collector-constraint-pii-identity-layer`.
All timestamps are UTC instants.

Sources: DESIGN §3.1 Domain Model; `domain-model.md` §1 Modeling
Conventions, §2 Core Entities, §3 Query Domain.

### Core ingestion and identity types

These types are shared verbatim with the SDK trait through
`usage-collector-sdk/src/models.rs`. Field-level schemas are authoritatively
defined in `domain-model.md`; the bullets below restate only the
plugin-relevant invariants.

- `UsageRecord` (`cpt-cf-usage-collector-entity-usage-record`). A
  single attributed measurement with status. On the SPI, `id` is
  plugin-minted on first acceptance and returned on every subsequent
  read; the plugin is the authority for `id` allocation. The accepted
  row is immutable except for the one-way `Active → Inactive` status
  transition issued through Method 5 (`transition_active_to_inactive`).
  `UsageRecord` carries `entry_type: EntryType` (mandatory; `usage` or
  `compensation`), `value: Decimal` (signed; sign jointly constrained
  by `MetricKind` × `entry_type` per Method 1 §"Value-sign matrix"),
  and `corrects_id: Optional<UsageRecord.id>` (present iff
  `entry_type = compensation`; references the active `entry_type =
usage` row being corrected).
- `EntryType` (`cpt-cf-usage-collector-entity-entry-type`). Enum with
  values `usage` (ordinary measurement) and `compensation` (counter
  value-reversal). Per
  `cpt-cf-usage-collector-adr-usage-compensation`, the SPI accepts
  both via the same persist call; there is NO separate `compensate`
  SPI method. Entries carry the same idempotency-key, PDP-attribution,
  and timestamp shape as ordinary usage records; the discriminator
  drives the value-sign matrix (Method 1) and the aggregation contract
  (Method 3).
- `ResourceRef` (`cpt-cf-usage-collector-entity-resource-ref`),
  `SubjectRef` (`cpt-cf-usage-collector-entity-subject-ref`),
  `IdempotencyKey` (`cpt-cf-usage-collector-entity-idempotency-key`),
  `RecordMetadata` (`cpt-cf-usage-collector-entity-record-metadata`),
  `DeactivationStatus` (`cpt-cf-usage-collector-entity-deactivation-status`).
  Consumed verbatim. The SPI MUST persist `metadata` byte-for-byte
  and return it verbatim on read; the plugin MUST NOT index,
  aggregate, normalize, classify, or transform `metadata` content
  per `cpt-cf-usage-collector-fr-record-metadata`.
- `Metric` (`cpt-cf-usage-collector-entity-metric`) and `MetricKind`
  (`cpt-cf-usage-collector-entity-metric-kind`). Per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)),
  the Metric Catalog (managed via the Plugin SPI, persisted in the
  active storage plugin's database) is the sole metric catalog and
  lives alongside `usage_records` in the same backend so that
  `usage_records.gts_id → metric_catalog.gts_id` can be enforced by a
  real in-database `ON DELETE RESTRICT` foreign key. Each metric is
  flat for v1 (no parent pointer, no abstract / non-abstract
  distinction); the catalog row carries `gts_id` (the GTS identifier
  string used both as catalog PK and as the reference value on every
  usage record), `metadata_fields: Vec<String>` (the closed, declared
  list of allowed metadata key names for this metric; all values typed
  as String end-to-end), and `created_at`. `kind ∈ {counter, gauge}`
  is **derived** from the `gts_id` prefix matching one of the two
  reserved kind base type ids (`gts.cf.core.usage.counter.v1~`,
  `gts.cf.core.usage.gauge.v1~`); it is not stored as a column. The
  plugin sees `gts_id` as an opaque platform identifier — the plugin
  MUST NOT classify, parse, or interpret it, MUST NOT re-implement
  closed-shape metadata-key validation (validation runs at the gateway
  L1 against the per-metric `metadata_fields` set hydrated from the
  catalog row), and MUST NOT prescribe an in-plugin reference scheme
  on the SPI surface. The
  in-plugin reference scheme (column type, index choice, or any other
  implementation choice) used to store or look up `gts_id` is the
  plugin author's choice and out of SPI scope. The Plugin SPI exposes
  catalog register, read, list, and delete methods per §"Public Plugin
  SPI Trait" / Methods 6–9.

Sources: `domain-model.md` §2.1–§2.9; DESIGN §3.7 Database schemas &
tables (`metric_catalog`, `usage_records`);
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

### Query types and views

- `AggregationQuery` (`cpt-cf-usage-collector-entity-aggregation-query`).
  Reaches the SPI already constrained: PDP-returned
  `cpt-cf-usage-collector-entity-pdp-constraint` filters have been
  intersected with user-supplied filters in
  `cpt-cf-usage-collector-component-query-gateway` so the
  authorization boundary is encoded into the filters the SPI
  receives. The plugin MUST treat every filter as authoritative and
  MUST NOT widen the result set beyond it.
- `RawQuery` (`cpt-cf-usage-collector-entity-raw-query`). Same
  constraint-application contract as `AggregationQuery`. The Plugin
  SPI does NOT receive `RawQuery` directly; the Query Gateway
  decomposes it into the structured Method 4 tuple
  (`filter_ast`, `order`, `page_after`, `limit`) before plugin
  dispatch (see §"Method Contracts" / Method 4).
- `AggregationResult` (`cpt-cf-usage-collector-entity-aggregation-result`).
  Plugin-produced output returned through the SPI.
- `UsageRecordRow` — the per-row payload Method 4 emits. Carries every
  `UsageRecord` field the gateway needs to assemble
  `toolkit_odata::Page<UsageRecord>`; semantically a `UsageRecord`,
  named `Row` here to mark the SPI-internal nature of the type alias.
- `UsageRecordFilterField`
  (`cpt-cf-usage-collector-entity-usage-record-filter-field`). The
  gear-owned Rust enum implementing `toolkit_odata::filter::FilterField`.
  Plugins receive the parsed `FilterNode<UsageRecordFilterField>`
  produced by the gateway after PDP-constraint composition; per-field
  operator allowances follow `domain-model.md` §2.10.
- `Keyset` (`cpt-cf-usage-collector-entity-keyset`). The canonical
  `(timestamp, id)` sort-key tuple used by Method 4 keyset
  pagination. Plugins receive an optional `Keyset` as the `page_after`
  bound and return the last-row `Keyset` of the emitted page; the
  gateway serializes the `Keyset` into the `CursorV1` envelope
  exposed to callers (kept opaque on the caller surface). Plugins
  never see the `CursorV1` envelope itself.
- `ODataOrderBy` — the parsed sort directive carried as part of the
  Method 4 input tuple. For raw queries it is the canonical
  `timestamp asc, id asc` order; the gateway rejects any other
  ordering before plugin dispatch.

Cursor encoding, decoding, validity, and lifecycle are owned by the
ToolKit gateway (`toolkit_odata::CursorV1` plus
`toolkit_odata::validate_cursor_against`); plugins do not mint,
decode, or validate cursor envelopes.

Sources: DESIGN §3.3 Plugin SPI capability list (keyset pagination);
§3.6 raw-query and aggregated-query sequences; §3.2 Query Gateway
Responsibility scope (constraint composition and cursor lifecycle);
phase-01 `out/phase-01-domain-contracts.md` (Keyset definition,
`UsageRecordFilterField` operator matrix).

### Plugin-specific outcome types

The following outcome enums are SPI-local and declared in
`usage-collector-sdk/src/plugin_api.rs`. They are the typed
shape callers (Plugin Host) MUST match against to interpret a `Ok`
result; failures use error variants instead.

- `PersistOutcome` — values:
  - `Persisted { id }` — the record was newly persisted; the plugin
    returns the freshly minted `UsageRecord.id`.
  - `Deduplicated { id }` — a prior record with the same
    `(tenant_id, gts_id, idempotency_key)` was already present
    **and the incoming record's caller-supplied canonical fields are
    exactly equal to the stored record** (an exact-equality retry); the
    plugin returns the prior record's `id`. This is the silent-absorb
    success and duplicates MUST NOT accumulate the counter total per
    `cpt-cf-usage-collector-principle-idempotency-by-key`.
  - `Conflict { id }` — a prior record with the same
    `(tenant_id, gts_id, idempotency_key)` was already present
    **but the incoming record's caller-supplied canonical fields differ
    from the stored record** (a canonical-field mismatch); the plugin
    returns the existing record's `id`. `Conflict` is NOT silently
    absorbed — the Plugin Host translates it to
    `DedupOutcome::Conflict` and the core lifts it to a fail-closed
    `idempotency_conflict` rejection (AIP-193 AlreadyExists / `409`,
    DESIGN §3.3) per `cpt-cf-usage-collector-adr-mandatory-idempotency`;
    the second write is never silently dropped.
    On a key collision the plugin compares the incoming record's
    canonical fields — `value`, `timestamp`, `resource_ref`,
    `subject_ref`, `source_gear`, and `metadata` — against the stored
    record under the same `(tenant_id, gts_id, idempotency_key)`.
    The dedup-key tuple itself is excluded (it is the match key) and the
    server-owned fields (`id`, `status`) are excluded. ALL compared
    fields equal → `Deduplicated`; ANY compared field differs —
    including a metadata-only difference → `Conflict`.
- `BatchPersistOutcome` — a list of per-record `Result<PersistOutcome,
UsageCollectorPluginError>` in the same length and order as the
  input batch. Per-record errors do not cause the batch call as a
  whole to fail; the batch returns `Ok` on the list and the Plugin
  Host surfaces per-record outcomes to the Ingestion Gateway.
- `DeactivationOutcome` — values:
  - `Transitioned { primary_id, cascaded_compensation_ids }` — the primary record
    was `Active` and is now `Inactive`. The outcome carries the
    `primary_id` (the record-id the caller passed to deactivate) plus
    `cascaded_compensation_ids: List<UsageRecord.id>` — the list of record ids of
    the active `entry_type = compensation` rows whose `corrects_id`
    referenced the primary `entry_type = usage` row and that were
    flipped from `Active` to `Inactive` in the same atomic transition
    per `cpt-cf-usage-collector-adr-usage-compensation`. The list is
    **empty** when the primary row is itself a compensation
    (single-row, no cascade), and **empty** when the primary row is a
    usage row with no active referencing compensations. The cascade is
    strictly **depth-1** by construction — compensating a
    compensation is a non-goal per ADR-0008, so no second hop is
    possible. `cascaded_compensation_ids` ordering is unspecified and downstream
    consumers MUST NOT depend on it.
  - `AlreadyInactive` — the record exists but its status is already
    `Inactive`; no state change occurred. This realizes the
    monotonicity invariant at the storage boundary per
    `cpt-cf-usage-collector-principle-monotonic-deactivation`. The
    one-way `Active → Inactive` latch applies to BOTH primary rows
    AND cascade-flipped compensation rows — no reverse transition
    exists.
  - `NotFound` — no record exists with the supplied `id`.
    These outcome enums are SPI-internal synthesis types derived from the
    behaviours required by DESIGN §3.6 sequences (emit, deactivate-event)
    and §3.7 referential rules; they do not appear on the SDK trait or
    REST surface, which translate them into the SDK-trait-level
    `UsageRecordAck` / `DeactivationAck` outputs and the REST-level
    confirmation responses respectively. The catalog methods (Methods
    6–9) return plain data shapes rather than dedicated outcome enums:
    a structured `MetricReferenced` error (see §"Error Taxonomy")
    surfaces FK-rejected deletes, and `Option` / list shapes cover
    "not found" / pagination concerns.

- `CatalogRow` — the per-row payload returned by Methods 7 and 8.
  Carries every `metric_catalog` column the gateway needs to hydrate
  its flat L1 catalog cache: `gts_id` (PK; the GTS identifier string),
  `metadata_fields: Vec<String>` (the closed, declared list of allowed
  metadata key names for this metric; all values typed as String
  end-to-end), and `created_at`. There is no `kind` column — `kind ∈
{counter, gauge}` is **derived** from the `gts_id` prefix matching
  one of the two reserved kind base type ids per ADR 0012, not stored.
  Field shapes are defined in DESIGN §3.7 Table: `metric_catalog`; the
  SPI exposes them verbatim per ADR 0012.
- `MetricListPage` — the keyset-paginated shape Method 8 emits. Carries
  the page rows plus the last-row `Keyset` (or its catalog-domain
  equivalent over `(created_at, gts_id)`) so the gateway can mint
  the next cursor. The shape mirrors Method 4's
  `(Vec<UsageRecordRow>, Option<Keyset>)` convention; SPI crate MAY
  wrap in a named struct in a future minor version without changing
  the variant catalog.

Sources: DESIGN §3.6 Emit Usage Record, Deactivate Usage Event,
Register Metric, Delete Metric sequences; §3.7 `usage_records` UNIQUE
on `(tenant_id, gts_id, idempotency_key)` and referential rule;
§3.7 Table `metric_catalog`;
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

### Trace context propagation

- Tracing is propagated via the ambient `tracing::Span` /
  OpenTelemetry context — no explicit `TraceContext` parameter is
  declared on any SPI method. The W3C Trace Context propagation values
  required by DESIGN §3.11.5 Distributed Tracing (`traceparent`,
  required, and `tracestate`, optional, per
  [W3C Trace Context Level 1](https://www.w3.org/TR/trace-context/))
  are carried by the active span / OpenTelemetry context that the
  Plugin Host establishes around each dispatch.
- The Plugin Host opens the per-call span before dispatching to the
  trait method (the host's `Service::*` methods are annotated with
  `#[tracing::instrument(...)]` mirroring
  `gears/credstore/credstore/src/domain/service.rs:109`); the SPI
  implementation runs inside that ambient span and MUST continue the
  span over its backend dispatch so end-to-end traces span gateway
  → core → plugin → backend.
- The reference plugin traits in
  `gears/credstore/credstore-sdk/src/plugin_api.rs:12-19`,
  `gears/system/authn-resolver/authn-resolver-sdk/src/plugin_api.rs:31-55`,
  and `gears/system/authz-resolver/authz-resolver-sdk/src/plugin_api.rs:19-22`
  carry no `TraceContext` parameter; this SPI follows the same pattern
  .
- `SecurityContext` is deliberately not passed to the SPI either,
  because authorization is already enforced upstream per
  `cpt-cf-usage-collector-principle-pdp-centric-authorization`.

Sources: DESIGN §3.11.5 "Plugin SPI surface
(`cpt-cf-usage-collector-interface-plugin`)" bullet; §3.9.6
Authorization Architecture.

### Cross-entity invariants honored by the Plugin SPI

- Records persisted through the SPI honour the
  `(tenant_id, gts_id, idempotency_key)` UNIQUE constraint
  per `cpt-cf-usage-collector-dbtable-usage-records` (the reference
  column carries the GTS identifier string `gts_id` per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`;
  the in-plugin column type and index choice are the plugin author's
  choice and out of SPI scope). On a key
  collision the plugin compares the incoming record's caller-supplied
  canonical fields against the stored record (see §"Plugin-specific
  outcome types"): an exact-equality retry is silently deduplicated and
  surfaced through the `Deduplicated` `PersistOutcome` variant on the
  `Ok` arm (not an error), while a canonical-field mismatch is surfaced
  through the `Conflict` `PersistOutcome` variant on the `Ok` arm,
  which the core then lifts to a fail-closed `idempotency_conflict`
  rejection (AlreadyExists / `409`) rather than a silent absorb.
- **Strict dedup-key preservation (normative).** The idempotency window
  is unbounded per `cpt-cf-usage-collector-adr-mandatory-idempotency`
  and `cpt-cf-usage-collector-dbtable-usage-records`: the
  `(tenant_id, gts_id, idempotency_key)` dedup key never
  expires, has no TTL, and is never intentionally reusable, so the
  UNIQUE constraint is permanent. A storage plugin MUST preserve the
  `(tenant_id, gts_id, idempotency_key)` tuple permanently —
  even when the corresponding record bodies are purged or archived by
  the plugin's own retention policy. Retention, purge, and archival
  remain plugin-owned (`cpt-cf-usage-collector-adr-pluggable-storage`;
  see also §"Exclusions/Non-goals"), and this obligation refines, not
  contradicts, that ownership: the plugin still owns retention but
  MUST NOT free a dedup key. Retention / purge / archival MUST NOT
  release a `(tenant_id, gts_id, idempotency_key)` tuple, so
  a replayed key always resolves to `Deduplicated` (exact-equality
  retry) or `Conflict` (canonical-field mismatch), never a fresh
  `Persisted`.
- The referential rule `usage_records.gts_id → metric_catalog.gts_id`
  is enforced by a real `ON DELETE RESTRICT` foreign key inside the
  plugin's backend database per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)).
  Both tables live in the same plugin backend so the FK rejection is
  atomic with the delete attempt — no cross-replica protocol, no
  distributed coordination, no gear-side `MetricCatalogRepo`. The
  plugin attempts the delete via Method 9 (`delete_metric`) and
  lets the FK fire; on rejection the plugin returns a structured
  `MetricReferenced` error (see §"Error Taxonomy") that the gateway
  surfaces deterministically. Backends that cannot enforce a native
  `ON DELETE RESTRICT` FK MUST emulate the check with a
  transactionally serializable read-before-delete inside the same
  transaction as the delete attempt; this is a plugin obligation.
  Historical `usage_records` rows cannot become orphaned at all
  because the FK rejects any unsafe delete; deleting a metric
  without first deactivating its `usage_records` is structurally
  impossible.
- Deactivation is a status-only update; no other column of
  `usage_records` may be mutated by the SPI per
  `cpt-cf-usage-collector-principle-monotonic-deactivation`.
  Deactivation is a **depth-1 atomic set flip** per
  `cpt-cf-usage-collector-adr-usage-compensation` (see Method 5):
  deactivating an `entry_type = usage` row atomically flips the
  primary row plus every active `entry_type = compensation` row
  whose `corrects_id` references the primary; deactivating an
  `entry_type = compensation` row flips that single row only (no
  cascade). The set commits as one atomic unit; partial cascades MUST
  be structurally impossible. The one-way `Active → Inactive` latch
  applies to primary rows AND cascade-flipped compensation rows.
- **`entry_type`-aware persistence (compensation primitive).** Per
  `cpt-cf-usage-collector-adr-usage-compensation`, the plugin
  persists `entry_type` (`usage` | `compensation`), signed `value`,
  and optional `corrects_id` (present iff
  `entry_type = compensation`) on every accepted record. The
  value-sign matrix in Method 1 is enforced as a **structural
  precondition** at the persistence boundary; violations are surfaced
  as `ContractViolation` and no row is inserted. The L1 referent
  checks for `corrects_id` (existence, `entry_type = usage`, shared
  `(tenant_id, gts_id)`, `Active`) are caller responsibilities
  per `cpt-cf-usage-collector-fr-usage-compensation`; the plugin
  enforces only the structural shape (`corrects_id` present iff
  `entry_type = compensation`).
- **No business logic (normative; refined for the compensation
  primitive).** The Plugin SPI defines no business logic. The plugin
  MUST NOT decide refunds, credits, credit-notes, quotas, lots,
  per-record remaining amounts, or net-non-negative enforcement. The
  plugin stores caller-supplied signed deltas and reports aggregates;
  recording a caller-supplied negative quantity is **recording, not
  computing**. A negative `SUM(value)` is an ordinary aggregation
  outcome — the plugin MUST NOT emit a negative-net detection signal
  per `cpt-cf-usage-collector-constraint-no-business-logic` and
  DESIGN §3.10.3.
- The plugin is the authority for `id` allocation on accepted
  records. Cursor allocation, decode, and validation are
  gateway-owned (`toolkit_odata::CursorV1` plus
  `validate_cursor_against`); plugins receive structured
  `(filter_ast, order, page_after: Option<Keyset>, limit)` inputs and
  return the last-row `Keyset` of the emitted page. Plugins are the
  authority for keyset-pagination ordering over the canonical
  `(timestamp, id)` sort keys (per
  `cpt-cf-usage-collector-principle-cursor-gateway-ownership`).
- The plugin MUST classify backend errors into the
  `UsageCollectorPluginError` taxonomy below so the Plugin Host can
  apply retry, circuit-break, or fail-closed behaviour without
  backend-specific parsing per `cpt-cf-usage-collector-nfr-error-experience`.

Sources: DESIGN §3.6 Delete Metric (catalog delete via the Plugin SPI
per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`,
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md));
§3.6 Deactivate Usage Event (atomic status transition); §3.7
Constraints; §3.5 Storage Plugin Contract (error classification);
§3.11.7 `usage_collector.plugin.accept_errors` label vocabulary.

## Public Plugin SPI Trait

The Usage Collector exposes one public Plugin SPI trait,
`UsageCollectorPluginV1`. The trait is async, `Send + Sync + 'static`,
declared in `usage-collector-sdk/src/plugin_api.rs`, and registered
into ClientHub with GTS instance scope by each
`usage-collector-plugin-<backend>` crate's own `init()` (not by the
Plugin Host). The Plugin Host
(`cpt-cf-usage-collector-component-plugin-host`) resolves the bound
instance lazily on the first dispatch after the `types-registry` is
consistent and looks the client up through
`ClientHub::try_get_scoped`.

The trait carries the methods listed below, one per SPI-exposed
capability:

| Method (logical)           | Realizes                                                                                                                         | Inputs                                                                                                                                                                                                                                                                                                                                                                                        | Output (Ok variant)                                                                                                                                                                                                                                                            |
| -------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Persist single record      | `fr-pluggable-storage`, `fr-ingestion`, `fr-idempotency`, `fr-usage-compensation`, `seq-emit-usage`                              | One `UsageRecord` (caller-supplied fields; `id` is plugin-allocated). Carries `entry_type` (`usage` \| `compensation`), signed `value`, and optional `corrects_id` (present iff `entry_type = compensation`).                                                                                                                                                                                 | `PersistOutcome` (`Persisted { id }`, `Deduplicated { id }` on an exact-equality retry, or `Conflict { id }` on a canonical-field mismatch).                                                                                                                                   |
| Persist batched records    | `fr-pluggable-storage`, `fr-usage-compensation`, `nfr-throughput`, `nfr-batch-and-report-timing`, `seq-emit-usage`               | Non-empty list of `UsageRecord`. Per-record fields carry `entry_type`, signed `value`, and optional `corrects_id` as in Method 1.                                                                                                                                                                                                                                                             | `BatchPersistOutcome` (per-record `Result<PersistOutcome, _>` in input order) (per OQ-1, declared as a bare `Vec<Result<PersistOutcome, UsageCollectorPluginError>>` in this reference; SPI crate MAY wrap in a named struct in a future minor version).                       |
| Aggregated query           | `fr-pluggable-storage`, `fr-query-aggregation`, `nfr-query-latency`, `seq-query-aggregated`                                      | One `AggregationQuery` (filters already PDP-constrained).                                                                                                                                                                                                                                                                                                                                     | `AggregationResult`.                                                                                                                                                                                                                                                           |
| Raw keyset-paginated query | `fr-pluggable-storage`, `fr-query-raw`, `nfr-batch-and-report-timing`, `seq-query-raw`                                           | `filter_ast: FilterNode<UsageRecordFilterField>` (already PDP-constrained), `order: ODataOrderBy`, `page_after: Option<Keyset>`, `limit: u64`.                                                                                                                                                                                                                                                | `(Vec<UsageRecordRow>, Option<Keyset>)` — page rows plus the last-row keyset; the gateway mints the next `CursorV1` from `last_keyset`.                                                                                                                                        |
| Deactivate usage event     | `fr-event-deactivation`, `fr-usage-compensation`, `adr-monotonic-deactivation`, `adr-usage-compensation`, `seq-deactivate-event` | `id` (`UsageRecord.id`); accepts any `entry_type`.                                                                                                                                                                                                                                                                                                                                            | `DeactivationOutcome` — `Transitioned { primary_id, cascaded_compensation_ids }` (depth-1 atomic set flip; `cascaded_compensation_ids` lists active referencing compensations cascade-flipped when primary is a usage row, empty otherwise), `AlreadyInactive`, or `NotFound`. |
| Register metric            | `fr-metric-registration`, `fr-pluggable-storage`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`, `seq-register-metric`  | `RegisterMetricRequest`: `gts_id` (GTS identifier string; catalog PK and the reference value on every usage record; MUST begin with one of the two reserved kind prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`), `metadata_fields: Vec<String>` (the closed, declared list of allowed metadata key names for this metric; all values typed as String end-to-end). | `CatalogRow` (the stored row keyed by `gts_id`).                                                                                                                                                                                                                               |
| Read metric                | `fr-metric-existence-and-kind`, `fr-pluggable-storage`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`                   | `gts_id: String`.                                                                                                                                                                                                                                                                                                                                                                             | `Option<CatalogRow>` — `Some` with the full row when present, `None` when no row matches.                                                                                                                                                                                      |
| List metrics               | `fr-pluggable-storage`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`                                                   | `ListMetricsFilter`: `kind: Option<String>` (derived from the `gts_id` prefix per ADR 0012; the gateway translates the supplied value to a `gts_id` prefix match — the plugin matches on the row's `gts_id` prefix, not a stored column), plus keyset-pagination fields (`page_after: Option<Keyset>`, `limit: u64`).                                                                         | `MetricListPage` — page rows plus the last-row keyset.                                                                                                                                                                                                                         |
| Delete metric              | `fr-metric-deletion`, `fr-pluggable-storage`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`, `seq-delete-metric`        | `gts_id: String`.                                                                                                                                                                                                                                                                                                                                                                             | `()` on successful delete. The plugin attempts the row delete and relies on the `usage_records.gts_id` `ON DELETE RESTRICT` FK to fire on a referenced row; FK violations surface as the `MetricReferenced` error variant (see §"Error Taxonomy"), not as `Ok`.                |

The trait carries exactly nine methods (five ingest / query / deactivate
plus four catalog); there is no plugin-side readiness probe and no
plugin-side flush. All methods return a `Result` over the listed Ok
variant and `UsageCollectorPluginError` (see §"Error Taxonomy"). The
Metric Catalog (managed via the Plugin SPI, persisted in the active
storage plugin's database) is the sole metric catalog per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md));
the SPI exposes the four catalog methods above so the gateway can
administer the catalog and hydrate its flat L1 catalog cache
(`Map<gts_id, {kind: derived, metadata_fields: HashSet<String>}>`)
via the plugin SoR. The in-plugin
reference scheme (column type, index choice, or any other
implementation choice) used to store or look up `gts_id` is the plugin
author's choice and out of SPI scope.

Sources: DESIGN §3.3 Plugin SPI capability list; §3.6 sequences
(emit, aggregated query, raw query, deactivate, register, delete);
§3.11.7 `usage_collector.plugin.ready` gauge (structural readiness
defined as "selector cached AND `try_get_scoped is Some`"; structural
check, not a plugin-side probe).

Note on batched persistence: DESIGN §3.3 and §1.2
(`cpt-cf-usage-collector-nfr-throughput-profile`) require the SPI to
expose batch ingestion so each plugin can drive its native bulk-write
paths. The two-method form (single + batched) mirrors the SDK trait's
two ingestion methods and matches the per-record acceptance
acknowledgement promise of `cpt-cf-usage-collector-component-ingestion-gateway`.

Note on trace-context propagation: DESIGN §3.11.5 requires
trace-context propagation on every SPI call so end-to-end traces span
gateway → core → plugin → backend. Propagation is carried by the
ambient `tracing::Span` / OpenTelemetry context opened by the Plugin
Host around each dispatch — no explicit `TraceContext` parameter
appears on any method (the reference plugin traits in `credstore`,
`authn-resolver`, and `authz-resolver` follow the same pattern).

## Method Contracts

Each method contract below lists the realized FR / sequence
identifiers, the structural inputs, the success output, and the
error categories the method may surface. Trace-context propagation is
ambient (see §"Trace context propagation") and is not declared per
method. Concrete error variant names are defined in §"Error
Taxonomy".

### Method 1 — Persist single usage record

- Identifier: `persist_usage_record`.
- Realizes: `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-seq-emit-usage`.
- Structural inputs: a `UsageRecord` value with caller-supplied
  fields. `id` is allocated by the plugin and is not present on the
  input. The persist capability accepts (in addition to the prior
  fields):
  - `entry_type: EntryType` — mandatory; one of `usage` or
    `compensation` per `cpt-cf-usage-collector-entity-entry-type`. The
    discriminator separating ordinary measurements from counter
    value-reversal entries. Never mutated after acceptance.
  - `value: Decimal` — **signed**; sign constrained jointly with
    `entry_type` and the referenced Metric's `kind` by the value matrix
    encoded below.
  - `corrects_id: Optional<UsageRecord.id>` — present iff
    `entry_type = compensation`; references the `UsageRecord.id` of the
    `entry_type = usage` row being corrected. MUST be absent when
    `entry_type = usage`.
    All caller-supplied fields have already been structurally validated
    and PDP-authorized by the core; see "Caller/plugin validation split"
    below.
- **Caller/plugin validation split (explicit).** The caller (Usage
  Collector core; specifically
  `cpt-cf-usage-collector-component-ingestion-gateway`) performs the
  L1 ingestion-time validations BEFORE invoking this method:
  - PDP attribution and authorization
    (`cpt-cf-usage-collector-principle-pdp-centric-authorization`).
  - Mandatory idempotency-key presence
    (`cpt-cf-usage-collector-adr-mandatory-idempotency`).
  - Metric existence and `kind` lookup against the gateway's L1
    catalog cache (hydrated from the plugin SoR via Method 7 per
    `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`;
    `kind` is derived once on cache load from the `gts_id` prefix).
  - Closed-shape metadata-key validation: every key in incoming
    `metadata` MUST be a member of the catalog row's `metadata_fields`
    set per ADR 0012; undeclared keys are rejected with
    `UnknownMetadataKey { gts_id, key }`. The plugin does NOT
    re-execute closed-shape validation at the persistence boundary.
  - L1 `corrects_id` referential integrity when
    `entry_type = compensation`: the referenced row MUST exist, MUST
    have `entry_type = usage`, MUST share `(tenant_id, gts_id)`
    with the incoming compensation, and MUST be `active`.
    The plugin enforces **structural constraints only** at the persistence
    boundary: schema shape (presence/absence of `corrects_id` consistent
    with `entry_type`), idempotency-key uniqueness on the dedup tuple,
    atomicity of the write, and the **value-sign matrix** below. The
    plugin MUST NOT re-execute L1 PDP, idempotency-presence, or
    `corrects_id` referent existence/kind/tenancy/active-state checks —
    those are caller responsibilities; a malformed or unauthorized call
    reaching the SPI is a Plugin Host contract violation surfaced as
    `ContractViolation`.
- **Value-sign matrix (structural; enforced at the persistence boundary).**

  | MetricKind | `entry_type`   | Allowed `value`                 | Outcome on violation          |
  | ---------- | -------------- | ------------------------------- | ----------------------------- |
  | `counter`  | `usage`        | `value >= 0` (unchanged)        | reject as `ContractViolation` |
  | `counter`  | `compensation` | `value < 0` (strictly negative) | reject as `ContractViolation` |
  | `gauge`    | `usage`        | Any signed value                | accept                        |
  | `gauge`    | `compensation` | REJECTED before persistence     | reject as `ContractViolation` |

  Counter compensation entries are append-only signed-negative rows
  that reduce the running `SUM` per
  `cpt-cf-usage-collector-adr-usage-compensation` and
  `cpt-cf-usage-collector-fr-usage-compensation`. The plugin records
  the caller-supplied signed delta; it does NOT compute the delta.

- Invariants the plugin MUST enforce:
  1. UNIQUE `(tenant_id, gts_id, idempotency_key)` per
     `cpt-cf-usage-collector-dbtable-usage-records`. On a key collision
     the plugin MUST compare the incoming record's caller-supplied
     canonical fields (`value`, `timestamp`, `resource_ref`,
     `subject_ref`, `source_gear`, `metadata`, `entry_type`,
     `corrects_id`) against the stored record under the same dedup-key
     tuple (the tuple itself and the server-owned `id` / `status` are
     excluded from the comparison). ALL compared fields equal → the
     duplicate is silently absorbed and surfaced as
     `PersistOutcome::Deduplicated { id }` with the prior record's
     `id`, and duplicates MUST NOT accumulate the counter total. ANY
     compared field differs — including a metadata-only difference, or
     a divergent `entry_type` / `corrects_id` — → the submission is
     surfaced as `PersistOutcome::Conflict { id }` with the existing
     record's `id` and MUST NOT be silently absorbed.
  2. Persist `metadata` byte-for-byte per
     `cpt-cf-usage-collector-fr-record-metadata`. The size cap is
     enforced upstream; the SPI MUST NOT silently truncate.
  3. Persist the record's `status` as `Active` on first acceptance.
  4. Persist `entry_type` and `corrects_id` exactly as supplied; the
     value-sign matrix above is enforced as a structural precondition
     and a violation is surfaced as `ContractViolation` (no row is
     inserted on rejection).
  5. Preserve the `(tenant_id, gts_id, idempotency_key)` dedup
     key permanently. The idempotency window is unbounded
     (`cpt-cf-usage-collector-adr-mandatory-idempotency`,
     `cpt-cf-usage-collector-dbtable-usage-records`): the key never
     expires, has no TTL, and is never intentionally reusable, so the
     UNIQUE constraint is permanent. The plugin still owns retention,
     archival, and purge of record bodies, but retention / purge /
     archival MUST NOT free a dedup key — even after a record body is
     purged or archived, a replayed key MUST still resolve to
     `Deduplicated` or `Conflict`, never a fresh `Persisted`. See
     §"Cross-entity invariants honored by the Plugin SPI" for the
     normative statement of this strict key-preservation obligation.
- **Idempotency (restated).** Repeating a persist call with the same
  caller-supplied idempotency key (under the same
  `(tenant_id, gts_id)` scope) AND an equivalent payload (all
  compared canonical fields equal, including `entry_type` and
  `corrects_id`) MUST return the previously persisted record's `id`
  via `PersistOutcome::Deduplicated` without creating a duplicate row.
- **Single ingestion path (no dedicated `compensate` SPI call).** Per
  `cpt-cf-usage-collector-adr-usage-compensation`, this same persist
  capability accepts both `entry_type = usage` and
  `entry_type = compensation` payloads. NO separate `compensate` SPI
  method exists; compensation rides the unified persist call.
- Success output: `PersistOutcome::Persisted { id }` on first
  acceptance; `PersistOutcome::Deduplicated { id }` on an
  exact-equality same-key resubmission; `PersistOutcome::Conflict
{ id }` on a same-key resubmission with any differing canonical
  field. All three carry a canonical record `id` so the Plugin Host can
  return a deterministic acknowledgement (or, for `Conflict`, a
  deterministic `idempotency_conflict` rejection) to the Ingestion
  Gateway.
- Error variants the plugin may surface: `Timeout`, `BackendError`,
  `ContractViolation`. The value-sign-matrix rejections above are
  surfaced as `ContractViolation` with a deterministic detail. Other
  categories (`Validation`, `UnknownMetric`, `Authorization`) are not
  raised by the SPI because they are enforced upstream. "Not ready" is
  detected structurally by the Plugin Host before dispatch (no scoped
  client under `ClientScope::gts_id(instance_id)`); the SPI itself has
  no `Unready` error variant.
- Latency budget: 75 ms p95 of the 200 ms total ingestion p95 per
  DESIGN §3.11.2.

Sources: DESIGN §3.3 Unified ingestion request shape; §3.6 Emit Usage
Record (`persist(record)` step); §3.7 `usage_records` UNIQUE
constraint and conditional value constraint; §3.10.3 Correction
posture (compensation primitive); §3.11.2 Latency Budgets (Plugin SPI
ingestion allocation); `cpt-cf-usage-collector-adr-usage-compensation`
(ADR-0008); `cpt-cf-usage-collector-entity-entry-type`.

### Method 2 — Persist batched usage records

- Identifier: `persist_usage_records`.
- Realizes: `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`,
  `cpt-cf-usage-collector-seq-emit-usage`.
- Trace propagation: the ambient batch span carried by the active
  `tracing` / OpenTelemetry context is the parent of each per-record
  child span the plugin opens. No explicit `TraceContext` parameter is
  passed; see §"Trace context propagation".
- Structural inputs: a list of `UsageRecord` values; an empty list
  is a contract violation surfaced as `ContractViolation` (see
  §"Error Taxonomy"). The list size is bounded upstream by
  `cpt-cf-usage-collector-nfr-batch-and-report-timing` (batched
  ingestion ≤ 100 records); the SPI MAY accept the bound without
  enforcement, but the plugin's backend bulk-write path MUST be
  exercised so the throughput envelope per
  `cpt-cf-usage-collector-nfr-throughput-profile` is reachable.
- Invariants the plugin MUST enforce: same per-record invariants as
  Method 1. Per-record failures (UNIQUE-conflict-but-detected-after-bulk,
  transient backend errors) are reported in the result list rather
  than failing the call as a whole.
- Success output: `BatchPersistOutcome` — a list of per-record
  results, each `Result<PersistOutcome, UsageCollectorPluginError>`,
  in the same length and order as the input (per OQ-1, declared as
  a bare `Vec<Result<PersistOutcome, UsageCollectorPluginError>>` in
  this reference; SPI crate MAY wrap in a named struct in a future
  minor version).
- Error variants the plugin may surface at the call level:
  `Timeout`, `BackendError`, `ContractViolation`. Per-record errors
  appear inside the result list with the same variant catalog as
  Method 1.
- Latency budget: total end-to-end p95 envelope of 500 ms for a
  100-record batch (per
  `cpt-cf-usage-collector-nfr-batch-and-report-timing` / PRD §9);
  DESIGN §3.11.2 does not currently carve a Plugin-SPI sub-allocation
  for this operation. Plugins SHOULD treat the SPI fraction as the
  dominant share, with at least 25 ms (mirroring the
  ingestion/aggregated-query gateway+PDP-enforcement overhead pattern
  in §3.11.2) reserved jointly for upstream gateway, per-component PDP
  enforcement, and core overhead. See OQ-7.

Sources: DESIGN §3.3 Plugin SPI capability list (batch ingestion);
§1.2 NFR rows for `cpt-cf-usage-collector-nfr-throughput-profile`
and `cpt-cf-usage-collector-nfr-batch-and-report-timing`.

### Method 3 — Aggregated query

- Identifier: `aggregate_usage`.
- Realizes: `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-nfr-query-latency`,
  `cpt-cf-usage-collector-seq-query-aggregated`.
- Structural inputs: one `AggregationQuery` value. The query's
  filters have already been intersected with PDP-returned
  `cpt-cf-usage-collector-entity-pdp-constraint` filters by
  `cpt-cf-usage-collector-component-query-gateway`. The plugin MUST
  treat every filter as authoritative; it MUST NOT widen the result
  set beyond the filters supplied.
- Pushdown obligation: the plugin executes the chosen `aggregation`
  (SUM, COUNT, MIN, MAX, AVG) and any `group_by` dimensions
  server-side using its native acceleration structures
  (pre-aggregated materialized views, columnar indexes, etc.) per
  DESIGN §1.2 NFR row for `cpt-cf-usage-collector-nfr-query-latency`
  and §3.11.2 Latency Budgets. Fanning out per-row reads to the core
  is forbidden — the SPI exposes aggregation as a single call so
  the core never iterates rows itself.
- **Aggregation contract (`entry_type`-aware; normative).** Across
  every accepted filter scope, on rows where `status = Active`:
  - `SUM` MUST net across rows where
    `entry_type IN (usage, compensation)`, treating `value` as a
    signed quantity. The result is the **signed net total** per
    group; counter compensation entries (`value < 0`) reduce the
    running counter total.
  - `COUNT`, `MIN`, `MAX`, and `AVG` MUST operate over rows where
    `entry_type = usage` only. Rows where `entry_type = compensation`
    MUST be excluded from these four aggregations before they are
    computed.
  - Rationale (carried verbatim from `domain-model.md`,
    `cpt-cf-usage-collector-entity-aggregation-query`, and
    `features/usage-query.md`): **compensation entries adjust SUM;
    they are not events.** Counting a compensation as an event would
    double-count the original usage event (the referenced `usage` row
    is already counted); including a compensation's strictly-negative
    `value` in `MIN` / `MAX` / `AVG` would corrupt extremes and means.
  - Inactive rows (any `entry_type`) MUST be excluded from all five
    aggregations BEFORE the `entry_type` partition is applied. The
    `active`-status filter and the `entry_type` partition are
    orthogonal.
  - A negative `SUM(value)` is an ordinary aggregation outcome — the
    Plugin SPI MUST NOT validate non-negative net and MUST NOT raise
    an error on a negative-net result per the un-policed-net stance
    in DESIGN §3.10.3.
- Success output: `AggregationResult` with `gts_id`,
  `aggregation`, and a list of buckets bounded by
  `cpt-cf-usage-collector-nfr-batch-and-report-timing` (aggregation
  result ≤ 100,000 rows over a 90-day single-tenant window with
  ≤ 2 groupings). An empty result inside the authorized scope is
  returned with an empty `buckets` list, not as an error.
- Error variants: `Timeout`, `BackendError`,
  `ContractViolation`.
- Latency budget: 425 ms p95 of the 500 ms total query p95 per
  DESIGN §3.11.2.

Sources: DESIGN §3.6 Query Aggregated Usage (`aggregate(query)` step);
DESIGN §3.10.3 Correction posture (un-policed-net stance); §1.2 NFR
allocation row for `cpt-cf-usage-collector-nfr-query-latency`; §3.11.2
Plugin SPI query budget; §1.2 NFR row for
`cpt-cf-usage-collector-nfr-batch-and-report-timing`;
`cpt-cf-usage-collector-adr-usage-compensation` (ADR-0008);
`cpt-cf-usage-collector-entity-entry-type`.

### Method 4 — Raw keyset-paginated query

- Identifier: `raw_page`.
- Realizes: `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`,
  `cpt-cf-usage-collector-seq-query-raw`.
- Structural inputs (canonical Rust signature):

  ```rust
  /// Plugins MUST implement keyset pagination over the canonical sort
  /// keyset `(timestamp, id)`. Cursor decode + validation happen at
  /// the gateway; the plugin receives a structured `page_after`
  /// keyset, never an opaque token.
  ///
  /// Returns the page rows plus the last-row keyset, which the
  /// gateway uses to mint the next `CursorV1`. A `None` `last_keyset`
  /// signals end-of-stream. Trace context is ambient (active
  /// `tracing::Span` / OpenTelemetry context); no explicit context
  /// parameter is declared.
  async fn raw_page(
      &self,
      filter_ast: FilterNode<UsageRecordFilterField>,
      order: ODataOrderBy,
      page_after: Option<Keyset>,
      limit: u64,
  ) -> Result<(Vec<UsageRecordRow>, Option<Keyset>), PluginError>;
  ```

  Where:
  - `filter_ast` is the parsed, PDP-constrained `FilterNode` over
    `UsageRecordFilterField`; the operator allowances per filterable
    field are governed by `domain-model.md` §2.10 (see
    `out/phase-01-domain-contracts.md` §4).
  - `order` is the parsed `ODataOrderBy` directive. The Query Gateway
    rejects any order other than the canonical raw-query order
    (`timestamp asc, id asc`) before plugin dispatch; plugins MAY
    treat the order as a contract assertion.
  - `page_after` is `None` on the first call and `Some(Keyset)`
    derived by the gateway from the caller-supplied `CursorV1` on
    subsequent calls. Plugins MUST resume the keyset scan strictly
    after the supplied `(timestamp, id)` tuple.
  - `limit` is the gateway-clamped per-page limit and is bounded by
    `cpt-cf-usage-collector-nfr-batch-and-report-timing`
    (raw-query page ≤ 1,000 records over a 24-hour window).

- Keyset pagination obligation: plugins MUST implement keyset
  pagination over the canonical sort keyset `(timestamp, id)` so the
  combined order is total and stable across plugins. Offset / limit
  scans are forbidden. The last-row keyset returned by the plugin is
  serialized by the gateway into the opaque `CursorV1` exposed to
  callers; plugins do not mint, decode, or validate cursor envelopes.
- Cursor lifecycle (gateway-owned): cursor decode, structural
  validation, and order/filter-binding checks via
  `toolkit_odata::validate_cursor_against` happen at the gateway
  BEFORE plugin dispatch. Cursor-decode failure, order mismatch, and
  filter mismatch are surfaced to callers as canonical Problem
  responses (`cursor_decode`, `order_mismatch`, `filter_mismatch`);
  no plugin-error category exists for cursor validity. Plugins
  receive only the validated, structured `(filter_ast, order,
page_after, limit)` tuple.
- Success output: `(Vec<UsageRecordRow>, Option<Keyset>)`. The first
  element is the page rows (records carrying their `status`); the
  second is the last-row `Keyset` (`(timestamp, id)` of the final
  row) used by the gateway to mint the next `CursorV1`. A `None`
  `last_keyset` signals end-of-stream and tells the gateway to omit
  `page_info.next_cursor`. An empty match inside the authorized
  scope returns `(vec![], None)` — not an error.
- Error variants: `Timeout`, `BackendError`,
  `ContractViolation`. No cursor-validity plugin-error category
  exists (cursor decode / order-mismatch / filter-mismatch are
  enforced by the gateway before any plugin dispatch — see "Cursor
  lifecycle" above and §"Error Taxonomy"). Anchored by
  `cpt-cf-usage-collector-principle-cursor-gateway-ownership`.
- Latency budget: total end-to-end p95 envelope of 1 s for a
  1,000-record raw page (per
  `cpt-cf-usage-collector-nfr-batch-and-report-timing` / PRD §9);
  DESIGN §3.11.2 does not currently carve a Plugin-SPI sub-allocation
  for this operation. Plugins SHOULD treat the SPI fraction as the
  dominant share, with at least 25 ms (mirroring the
  ingestion/aggregated-query gateway+PDP-enforcement overhead pattern
  in §3.11.2) reserved jointly for upstream gateway, per-component PDP
  enforcement, and core overhead. See OQ-7.

Sources: DESIGN §3.6 Query Raw Usage Records (`raw_page(filter_ast,
order, page_after, limit)` step); §1.2 NFR row for
`cpt-cf-usage-collector-nfr-batch-and-report-timing`;
`research-toolkit-alignment.md` §1 D10 (gateway-owned cursor lifecycle,
structured plugin input tuple) and D11 (PDP placement); phase-01
`out/phase-01-domain-contracts.md` §3 (`Keyset` definition) and §4
(`UsageRecordFilterField` operator matrix).

### Method 5 — Deactivate usage event

- Identifier: `transition_active_to_inactive`.
- Realizes: `cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-seq-deactivate-event`,
  `cpt-cf-usage-collector-adr-monotonic-deactivation`,
  `cpt-cf-usage-collector-adr-usage-compensation`,
  `cpt-cf-usage-collector-principle-monotonic-deactivation`.
- Structural inputs: the target `UsageRecord.id`. The capability
  accepts the id of any active row regardless of `entry_type` per
  ADR-0005 (re-scoped) and ADR-0008 — both `entry_type = usage` and
  `entry_type = compensation` rows are deactivatable through the same
  call.
- **Outcome shape (depth-1 atomic set flip; normative).**

  ```text
  DeactivationOutcome::Transitioned {
    primary_id:   UsageRecord.id,           // the row the caller asked to deactivate
    cascaded_compensation_ids: List<UsageRecord.id>      // active compensation rows referencing the primary, flipped in the same atomic unit
  }
  ```

  The capability returns a **depth-1 atomic set flip**, NOT a
  single-row flip. The semantics are:
  - When the primary row has `entry_type = usage`: the plugin MUST
    flip the primary row's `status` from `Active` to `Inactive` AND
    flip every currently-active row where
    `entry_type = compensation`, `corrects_id = primary_id`,
    `tenant_id = primary.tenant_id`, and
    `gts_id = primary.gts_id` from `Active` to
    `Inactive` in the **same atomic transition**. The outcome
    `cascaded_compensation_ids` lists the ids of every cascade-flipped
    compensation row (possibly empty when no active compensations
    reference the primary).
  - When the primary row has `entry_type = compensation`: the plugin
    MUST flip only the primary row's `status` from `Active` to
    `Inactive`. NO cascade evaluation occurs because no row may
    reference a compensation per
    `cpt-cf-usage-collector-adr-usage-compensation` (compensating a
    compensation is a non-goal). `cascaded_compensation_ids` is the empty list.
  - The cascade depth bound is **structural**, not enforced by the
    algorithm — the `corrects_id → entry_type = usage` L1 rule
    (enforced by the caller at ingestion time) makes a
    `compensation → compensation` reference impossible by
    construction.

- **Atomicity invariant.** The set flip (primary row + all matched
  active referencing compensations) MUST commit as one unit. Either
  all rows in the set flip from `Active` to `Inactive` together, or
  none do. Partial cascades MUST be structurally impossible. Two
  concurrent deactivation requests targeting the same primary cannot
  both observe `Active` and both proceed — exactly one returns
  `Transitioned` (with its `cascaded_compensation_ids` set); the other returns
  `AlreadyInactive`. No column of `usage_records` other than `status`
  is mutated.
- **One-way latch invariant.** The `Active → Inactive` transition is
  permanent for every row touched — primary row AND cascade-flipped
  compensation rows alike. No reverse transition exists per
  `cpt-cf-usage-collector-principle-monotonic-deactivation`.
- **Concurrency rule (carried from the caller-side L1 check).** A
  compensation submission referencing a row R that arrives while R is
  being deactivated is rejected by the caller-side L1 "referenced
  record MUST be active" check BEFORE this method is invoked, so the
  plugin sees an inert request and never has to coordinate with the
  in-flight cascade. The atomicity of the set flip guarantees that no
  compensation can be admitted referencing a row that has already
  left `Active`.
- Success output:
  - `DeactivationOutcome::Transitioned { primary_id, cascaded_compensation_ids }`
    when the primary row was `Active` and is now `Inactive`. For a
    usage primary row with no active referencing compensations,
    `cascaded_compensation_ids` is the empty list. For a compensation primary
    row, `cascaded_compensation_ids` is always the empty list.
  - `DeactivationOutcome::AlreadyInactive` when the primary row
    exists but is already `Inactive`. No cascade re-evaluation
    occurs; no row's `status` changes.
  - `DeactivationOutcome::NotFound` when no record exists with the
    supplied `id` in the tenant scope.
- Error variants: `Timeout`, `BackendError`, `ContractViolation`.
  Cascade-flip failures (e.g., the storage layer refuses the atomic
  multi-row update) MUST surface as `BackendError` or `Timeout`; a
  partial commit MUST NOT be observable by the caller.

Sources: DESIGN §3.3 Deactivate response shape (depth-1 cascade);
§3.6 Deactivate Usage Event (`transition_active_to_inactive` step +
atomicity prose); §3.10.3 Correction posture (deactivation cascade);
`cpt-cf-usage-collector-adr-monotonic-deactivation` (ADR-0005,
re-scoped); `cpt-cf-usage-collector-adr-usage-compensation`
(ADR-0008); `features/event-deactivation.md` §3 Algorithms
(`cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`).

### Method 6 — Register metric type

- Identifier: `register_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-registration`,
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`,
  `cpt-cf-usage-collector-seq-register-metric`.
- Structural inputs: a `RegisterMetricRequest` value with:
  - `gts_id` — the GTS identifier string of the metric. Catalog PK
    AND the reference value stored on every `usage_records` row that
    references the metric (per ADR 0012 there is no separate UUID
    derivation; `gts_id` is the same value in both places).
    Deployment-unique. MUST begin with one of the two reserved kind
    base type prefixes `gts.cf.core.usage.counter.v1~` or
    `gts.cf.core.usage.gauge.v1~`; `kind ∈ {counter, gauge}` is
    derived from the prefix and is NOT stored as a column.
  - `metadata_fields: Vec<String>` — the closed, declared list of
    allowed metadata key names for this metric. All values are typed
    as String end-to-end (the catalog declares keys; values are
    conveyed as strings). Only declared keys are accepted at ingest;
    undeclared keys are validation errors. There is no free-form
    remainder and no `extras` map. The gateway L1 cache lifts this
    list verbatim into a `HashSet<String>` keyed by `gts_id`.
- **Caller/plugin validation split.** The gateway (specifically
  `cpt-cf-usage-collector-component-metric-catalog`) performs L1
  validations BEFORE invoking this method: PDP authorization, GTS
  identifier well-formedness, `gts_id` kind-prefix validation
  (rejected with `InvalidKindPrefix { gts_id }` when the identifier
  does not begin with one of the two reserved kind base type
  prefixes), and `metadata_fields` well-formedness (non-null, no
  duplicates). The plugin enforces **structural constraints only** at
  the persistence boundary: row uniqueness on `gts_id`, atomic
  insert.
- Invariants the plugin MUST enforce:
  1. Insert a new row in `metric_catalog` with the supplied `gts_id`
     as the primary key. The in-plugin reference scheme (column
     type, index choice, or any other implementation choice) for
     storing or looking up `gts_id` is the plugin author's choice
     and is out of SPI scope per ADR 0012.
  2. Reject with `MetricAlreadyExists { gts_id }` if a row with the
     same `gts_id` is already present and the resubmitted payload
     differs from the stored row.
  3. Persist `metadata_fields` verbatim (element order and content
     preserved); the plugin MUST NOT normalize, canonicalize,
     deduplicate, or otherwise interpret the list contents.
  4. Stamp `created_at` with the plugin's accept timestamp (UTC).
- Success output: a `CatalogRow` containing the stored row.
- Error variants the plugin may surface: `Timeout`, `BackendError`,
  `ContractViolation`, `MetricAlreadyExists { gts_id }`.
- Idempotency: `register_metric` is idempotent on `gts_id`. A
  resubmission of an identical `RegisterMetricRequest` (same
  `gts_id`, element-equal `metadata_fields`) MUST return the existing
  `CatalogRow` rather than `MetricAlreadyExists`. A resubmission
  whose payload differs from the stored row MUST surface
  `MetricAlreadyExists` so the gateway can lift it to the caller as
  a deterministic conflict.

Sources: DESIGN §3.6 Register Metric sequence; §3.7 Table
`metric_catalog`;
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

### Method 7 — Read metric type

- Identifier: `read_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
- Structural inputs: `gts_id: String`.
- Behaviour: return the matching row including `gts_id`,
  `metadata_fields`, and `created_at` (the full `CatalogRow`). Used
  by gateway L1 cache hydration on a miss and by query-time
  declared-key resolution per ADR 0012. The plugin MUST NOT
  post-process the row content; this is a verbatim read.
- Success output: `Option<CatalogRow>` — `Some(row)` when present,
  `None` when no row has that `gts_id` (the absence is **not**
  surfaced as an error so the gateway can distinguish "no such
  metric" from a backend failure without pattern-matching).
- Error variants: `Timeout`, `BackendError`.

Sources: DESIGN §3.7 Table `metric_catalog`; gateway L1 cache model in
DESIGN §3.7 "Catalog ownership and physical location";
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

### Method 8 — List metric types

- Identifier: `list_metrics`.
- Realizes: `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
- Structural inputs: a `ListMetricsFilter` value with:
  - `kind: Option<String>` — when `Some`, return only rows whose
    `gts_id` begins with the reserved base type prefix corresponding
    to the supplied value (`counter` ⇒ `gts.cf.core.usage.counter.v1~`,
    `gauge` ⇒ `gts.cf.core.usage.gauge.v1~`). `kind` is derived from
    the `gts_id` prefix per ADR 0012; it is NOT a stored column.
  - `page_after: Option<Keyset>` — keyset over `(created_at,
gts_id)` for pagination consistency with Method 4.
  - `limit: u64` — page-size bound; the gateway enforces the
    operator-facing cap before dispatch.
- Behaviour: filter `metric_catalog` rows by the supplied predicates
  and return a page. Results are ordered by `(created_at, gts_id)`
  ascending; the plugin MUST NOT impose any other ordering and MUST
  NOT mutate the rows in any way.
- Success output: `MetricListPage` — `(Vec<CatalogRow>,
Option<Keyset>)`. An empty page with `None` keyset means the
  filter produced no further rows; a populated page with `Some`
  keyset advances to the next page.
- Error variants: `Timeout`, `BackendError`, `ContractViolation`
  (e.g., `limit = 0` or an unsupported `kind` shape).
- Used by REST `GET /metrics` list endpoints and by operator audits
  of the catalog inventory.

Sources: DESIGN §3.6 Register Metric sequence; §3.7 Table
`metric_catalog`;
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

### Method 9 — Delete metric type

- Identifier: `delete_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-deletion`,
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`,
  `cpt-cf-usage-collector-seq-delete-metric`.
- Structural inputs: `gts_id: String`.
- Behaviour: attempt to delete the `metric_catalog` row with the
  supplied `gts_id`. The plugin's backend `usage_records.gts_id`
  `ON DELETE RESTRICT` foreign key fires natively inside the same
  transaction as the delete; the plugin MUST NOT perform any
  separate "is the metric referenced?" probe before the delete
  attempt — the FK is the single source of truth. On FK rejection
  the plugin MUST surface a structured `MetricReferenced` error
  carrying a sample reference count (a small, bounded sample
  sufficient to surface "this metric still has rows" without
  scanning the entire table; the exact bound is plugin-tunable but
  MUST be at least `1`). On success the row is gone and the plugin
  returns `()`.
- Backends that cannot enforce a native `ON DELETE RESTRICT` FK
  MUST emulate the check with a transactionally serializable
  read-before-delete inside the same transaction as the delete
  attempt; the emulation MUST NOT admit a window during which a
  concurrent `persist` could insert a row referencing the `gts_id`
  being deleted. This is a plugin obligation per ADR 0012.
- Invariants the plugin MUST enforce:
  1. Reject with `MetricNotFound { gts_id }` if no row has that
     `gts_id` (the absence is surfaced as an error, not as a silent
     success, so the gateway can distinguish "already gone" from
     "successfully deleted now").
  2. Reject with `MetricReferenced { gts_id, sample_ref_count }`
     if any `usage_records` row still references the row (FK fires
     or, for emulated backends, the read-before-delete sees a
     referent). The gateway lifts `MetricReferenced` to HTTP 409 on
     the REST surface.
- Success output: `()`.
- Error variants the plugin may surface: `Timeout`, `BackendError`,
  `ContractViolation`, `MetricNotFound { gts_id }`,
  `MetricReferenced { gts_id, sample_ref_count }`.

Sources: DESIGN §3.6 Delete Metric sequence; §3.7 Table
`metric_catalog` referential delete semantics; §3.7 Table
`usage_records` `gts_id REFERENCES metric_catalog(gts_id) ON DELETE
RESTRICT`;
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

<!-- Method 10 (Read metric chain) removed in ADR 0012 — metric types
are flat for v1; no ancestor-chain walk and no
`read_metric_chain` SPI method survives. The flat L1 catalog cache
keys `Map<gts_id, {kind, metadata_fields}>` entries by `gts_id` with
no cascade invalidation. -->

## Catalog and validation surface

Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)),
the Metric Catalog (managed via the Plugin SPI, persisted in the
active storage plugin's database) is the sole metric catalog and
lives alongside `usage_records`. The in-plugin reference scheme
(column type, index choice, or any other implementation choice) used
to store or look up `gts_id` is the plugin author's choice and is
out of SPI scope.
usage-collector owns the catalog **semantically** (registration API,
PDP, validation, schema authority); the plugin owns **durable
storage and FK enforcement**. The split is:

| Concern                                                            | Owner                            | Where it runs                                        |
| ------------------------------------------------------------------ | -------------------------------- | ---------------------------------------------------- |
| Catalog REST API (`POST` / `GET` / `DELETE /metrics`)              | usage-collector gateway          | gateway process                                      |
| PDP authorization on register / read / list / delete               | usage-collector gateway          | gateway process                                      |
| `gts_id` kind-prefix validation at register time                   | usage-collector gateway          | gateway process at register time                     |
| `metadata_fields` well-formedness at register time                 | usage-collector gateway          | gateway process at register time                     |
| Closed-shape metadata-key membership on incoming record `metadata` | usage-collector gateway L1 cache | gateway process at ingest hot path                   |
| Catalog rows (System of Record)                                    | storage plugin                   | plugin's backend DB, alongside `usage_records`       |
| `usage_records → metric_catalog` referential integrity             | storage plugin (engine)          | plugin's backend DB, via FK / serializable emulation |
| Catalog row inserts / reads / lists / deletes                      | storage plugin (engine)          | plugin's backend DB, via Methods 6 / 7 / 8 / 9       |

**Validation handoff (normative).** Closed-shape metadata-key
validation runs at the gateway L1, not the plugin. The plugin stores
`metadata_fields` verbatim; the gateway lifts each row's
`metadata_fields` into a `HashSet<String>` keyed by `gts_id`, per ADR 0012. Incoming `UsageRecord.metadata` keys are checked for membership
in the per-metric `metadata_fields` set at the gateway before Method 1
or Method 2 dispatch; an undeclared key is rejected as
`UnknownMetadataKey { gts_id, key }`. **Plugins do NOT re-implement
closed-shape validation** — and the plugin MUST NOT reject a
`persist_usage_record` call on metadata-key grounds (it MAY reject on
the structural value-sign matrix in Method 1, but the closed-shape
membership rule is the gateway's responsibility and arrives at the
SPI already enforced).

**Cache invalidation (normative).** Method 6 (`register_metric`) and
Method 9 (`delete_metric`) MUST be treated by the gateway as
cache-evict events keyed on `gts_id`: on successful registration of a
row R, the gateway L1 catalog cache refreshes the `Map<gts_id,
{kind, metadata_fields}>` entry for `gts_id = R`; on successful
deletion of R, the gateway L1 catalog cache evicts the entry for
`gts_id = R`. Because metric types are flat for v1 per ADR 0012,
there is no cascade invalidation to descendants — the keyspace is
flat. The plugin itself holds no catalog cache and MUST NOT emit
cache-invalidation events out-of-band — the gateway drives
invalidation from the synchronous success of Methods 6 / 9 per
ADR 0012.

Sources:
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md);
gateway L1 catalog cache load-bearing role per ADR 0012 (flat
`Map<gts_id, {kind, metadata_fields}>`; no merge core; do NOT depend
on `types-registry-sdk`).

## Data Model

The Plugin SPI's persistence boundary covers exactly two logical
tables, both physically located in the active storage plugin's
backend database per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)).
The shapes below are **logical contracts** — column shapes, primary
keys, foreign keys, and the architectural indexes that satisfy NFRs
— not DDL; concrete physical layout (additional indexes,
partitioning, retention, materialized views, acceleration structures,
in-plugin reference scheme) is plugin-owned per
`cpt-cf-usage-collector-principle-pluggable-storage` and DESIGN §3.7.

### Table: metric_catalog

**ID**: `cpt-cf-usage-collector-dbtable-metric-catalog`.

**Ownership**: plugin-owned per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
Semantic ownership of catalog operations (PDP, validation, schema
authority) remains with the gateway; durable rows live in the
plugin's backend alongside `usage_records` so the FK between the
two tables is enforced natively.

**Columns** (consistent with DESIGN §3.7 Table `metric_catalog`):

| Column            | Type        | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gts_id`          | TEXT        | Primary key. The GTS identifier string of the metric, used both as catalog PK and as the reference value on every `usage_records` row referencing this metric per ADR 0012. MUST begin with one of the two reserved kind base type prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`. The FK target referenced by `usage_records.gts_id`. The in-plugin column type and index choice are the plugin author's choice and out of SPI scope. |
| `metadata_fields` | TEXT[]      | Closed, declared list of allowed metadata key names for this metric. All values typed as String end-to-end. Stored verbatim (element order and content preserved). The gateway L1 catalog cache lifts this list into a `HashSet<String>` keyed by `gts_id`; closed-shape membership validation runs at the gateway. Defines the per-metric declared-key surface for filter / group-by.                                                                            |
| `created_at`      | TIMESTAMPTZ | Catalog-write timestamp captured by the plugin on accept (UTC).                                                                                                                                                                                                                                                                                                                                                                                                   |

**PK**: `gts_id`.

**Note on `kind` (derived, not stored).** There is no `kind` column.
`kind ∈ {counter, gauge}` is derived from the `gts_id` prefix matching
one of the two reserved kind base type prefixes per ADR 0012. The
gateway computes `kind` once on cache load; the plugin MUST NOT parse
or interpret `gts_id`.

**Constraints**: `gts_id`, `metadata_fields`, and `created_at` are
`NOT NULL`. `metadata_fields` is a list of declared metadata key names
validated by the gateway against the closed-shape rules per ADR 0012
before the row is forwarded for persistence; the plugin MUST NOT
re-execute that validation.

**Note (Removed in ADR 0012):** ancestor-pointer, abstract /
non-abstract distinction, type-uuid, type-id, and any per-property
indexability annotation are no longer carried on the catalog row.
Metric types are flat for v1; every registered metric is concrete;
the per-metric declared-key surface is `metadata_fields` (every
declared key is queryable — declared = queryable).

**Referential delete semantics (plugin obligation)**: a delete of a
`metric_catalog` row MUST be rejected by the backend inside the same
transaction as the delete attempt when any `usage_records.gts_id`
still references it (see the `ON DELETE RESTRICT` FK on
`usage_records` below). Backends that support real foreign keys MUST
declare it as `ON DELETE RESTRICT`; backends that do not MUST emulate
the check with a transactionally serializable read-before-delete
inside the same transaction. Either way, the plugin returns
`MetricReferenced { gts_id, sample_ref_count }` (see §"Error
Taxonomy") on rejection.

### Table: usage_records

**ID**: `cpt-cf-usage-collector-dbtable-usage-records`.

The full shape is normative in DESIGN §3.7 Table `usage_records`.
The Plugin SPI surface highlights three contracts per ADR 0012:

1. **`gts_id` reference column and FK**:

   ```text
   gts_id TEXT NOT NULL
     REFERENCES metric_catalog(gts_id) ON DELETE RESTRICT
   ```

   per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
   The reference is the GTS identifier string — the same value used
   as the catalog PK and as the reference field on every wire-format
   usage record. The in-plugin column type and index choice are the
   plugin author's choice and out of SPI scope.

2. **Dedup composite**: `UNIQUE (tenant_id, gts_id, idempotency_key)`
   per the unbounded-idempotency model in
   `cpt-cf-usage-collector-adr-mandatory-idempotency`. Plugins MUST
   preserve this tuple permanently even when record bodies are
   purged or archived (see §"Cross-entity invariants honored by the
   Plugin SPI" for the normative statement).

3. **Architectural index**: a `(tenant_id, gts_id, timestamp)` index
   is the architectural minimum the SPI relies on for raw-query and
   aggregated-query latency budgets per DESIGN §3.11.2; plugins MAY
   add further indexes (acceleration structures, materialized views)
   at their discretion.

The full column list, including `entry_type`, `corrects_id`,
`metadata`, `status`, the conditional value constraint, and the rest
of the dedup-on-conflict invariants, lives in DESIGN §3.7 Table
`usage_records` and is not duplicated here.

Sources: DESIGN §3.7 Database schemas & tables (`metric_catalog`,
`usage_records`);
[`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).

## Contract Tests

Every conforming plugin implementation MUST pass the contract tests
listed below. The tests are **behavioural**, not implementation-
specific: each test names a deterministic precondition (`setup`), a
deterministic action (`when`), and a deterministic acceptance
assertion (`then`). The tests are backend-agnostic — they MUST pass
regardless of the storage backend the plugin selects — and apply
uniformly to the persist capability (Method 1) and the deactivate
capability (Method 5). The pseudocode below uses a CDSL fence and
deliberately avoids any language-specific syntax.

The tests reinforce the **no-business-logic** invariant (see
§"Cross-entity invariants honored by the Plugin SPI" and the
`cpt-cf-usage-collector-constraint-no-business-logic` constraint):
the plugin records caller-supplied signed deltas and reports
aggregates; it never decides refunds, credits, quotas, or net-non-
negative enforcement.

### `spi-contract-test-deactivate-cascade-usage`

Cascade-on-usage: deactivating a usage row atomically flips every
active compensation referencing it.

```cdsl
setup:
  M is a counter metric in tenant T with gts_id G   // G is M's GTS identifier string
  R = persist({ tenant_id: T, gts_id: G, entry_type: usage, value: +10, idempotency_key: K_R })
  for i in 1..N:
    C[i] = persist({ tenant_id: T, gts_id: G, entry_type: compensation,
                     value: -1, corrects_id: R.id, idempotency_key: K_C_i })
  assert R.status = Active and every C[i].status = Active

when:
  outcome = deactivate(R.id)

then:
  assert outcome = Transitioned { primary_id: R.id, cascaded_compensation_ids: [C[1].id, ..., C[N].id] }
  assert R.status = Inactive
  for i in 1..N:
    assert C[i].status = Inactive
  assert no other row in M's tenant scope changed status
  assert the (N + 1) status flips committed in a single atomic transition
```

Acceptance assertion: the outcome MUST equal
`Transitioned { primary_id: R.id, cascaded_compensation_ids: [C[1].id..C[N].id] }`,
all N + 1 rows MUST be `status = Inactive` in a single atomic commit,
and no other rows MUST change.

### `spi-contract-test-deactivate-cascade-compensation`

No-cascade-on-compensation: deactivating a compensation row never
cascades.

```cdsl
setup:
  M is a counter metric in tenant T with gts_id G   // G is M's GTS identifier string
  R = persist({ tenant_id: T, gts_id: G, entry_type: usage, value: +10, idempotency_key: K_R })
  C = persist({ tenant_id: T, gts_id: G, entry_type: compensation,
                value: -3, corrects_id: R.id, idempotency_key: K_C })
  assert R.status = Active and C.status = Active

when:
  outcome = deactivate(C.id)

then:
  assert outcome = Transitioned { primary_id: C.id, cascaded_compensation_ids: [] }
  assert C.status = Inactive
  assert R.status = Active  // R is untouched
```

Acceptance assertion: the outcome MUST equal
`Transitioned { primary_id: C.id, cascaded_compensation_ids: [] }`, only C MUST
flip to `Inactive`, and R MUST remain `Active`.

### `spi-contract-test-counter-only-compensation`

Counter-only compensation: a persist of `entry_type = compensation`
against a gauge metric is rejected at the structural boundary.

```cdsl
setup:
  M_g is a gauge metric in tenant T with gts_id G_g   // G_g is M_g's GTS identifier string
  let pre_count = COUNT(*) over usage_records WHERE gts_id = G_g

when:
  attempt persist({ tenant_id: T, gts_id: G_g, entry_type: compensation,
                    value: -5, corrects_id: <any>, idempotency_key: K })

then:
  assert persist returned ContractViolation (deterministic rejection signal)
  assert COUNT(*) over usage_records WHERE gts_id = G_g = pre_count  // no row inserted
```

Acceptance assertion: persist MUST be rejected at the structural
boundary with a deterministic rejection signal
(`ContractViolation`), and no row MUST be inserted.

### `spi-contract-test-value-matrix`

Value-matrix: the persist capability enforces the four-cell value
sign matrix structurally.

```cdsl
setup:
  M_c is a counter metric in tenant T with gts_id G_c   // G_c is M_c's GTS identifier string
  M_g is a gauge metric in tenant T with gts_id G_g     // G_g is M_g's GTS identifier string
  let pre_count = COUNT(*) over usage_records WHERE tenant_id = T

when / then (each row independent):

  // counter + usage with negative value -> REJECTED
  attempt persist({ gts_id: G_c, entry_type: usage, value: -1, ... })
  assert result = ContractViolation
  assert COUNT(*) unchanged

  // counter + compensation with non-negative value -> REJECTED
  attempt persist({ gts_id: G_c, entry_type: compensation, value: 0, ..., corrects_id: <some active usage row> })
  assert result = ContractViolation
  assert COUNT(*) unchanged

  attempt persist({ gts_id: G_c, entry_type: compensation, value: +1, ..., corrects_id: <some active usage row> })
  assert result = ContractViolation
  assert COUNT(*) unchanged

  // gauge + usage with any signed value -> ACCEPTED
  attempt persist({ gts_id: G_g, entry_type: usage, value: -7, ... })
  assert result = Persisted { id: <id> }

  attempt persist({ gts_id: G_g, entry_type: usage, value: +9, ... })
  assert result = Persisted { id: <id> }

  // gauge + compensation (any value) -> REJECTED
  attempt persist({ gts_id: G_g, entry_type: compensation, value: -2, ... })
  assert result = ContractViolation
  assert no gauge+compensation row exists for tenant T
```

Acceptance assertion: rejections MUST be deterministic and no row
MUST be inserted on a rejected call. Accepted cells MUST produce a
`Persisted` outcome carrying a fresh `id`.

### `spi-contract-test-aggregation-sum-nets-and-usage-only-others`

Aggregation-semantics: `SUM` nets across usage and compensation;
`COUNT`, `MIN`, `MAX`, `AVG` operate over usage entries only.

```cdsl
setup:
  M is a counter metric in tenant T with gts_id G   // G is M's GTS identifier string
  for i in 1..k:
    U[i] = persist({ tenant_id: T, gts_id: G, entry_type: usage, value: U_i_value, idempotency_key: K_U_i })
  for j in 1..m:
    X[j] = persist({ tenant_id: T, gts_id: G, entry_type: compensation,
                     value: X_j_value, corrects_id: U[pick(j)].id, idempotency_key: K_X_j })
  assert U_i_value >= 0 for all i and X_j_value < 0 for all j

when:
  result_sum   = aggregate(SUM,   filter { gts_id: G, status: Active })
  result_count = aggregate(COUNT, filter { gts_id: G, status: Active })
  result_min   = aggregate(MIN,   filter { gts_id: G, status: Active })
  result_max   = aggregate(MAX,   filter { gts_id: G, status: Active })
  result_avg   = aggregate(AVG,   filter { gts_id: G, status: Active })

then:
  assert result_sum   = sum(U[i].value for i in 1..k) + sum(X[j].value for j in 1..m)   // signed net total
  assert result_count = k                                                                 // usage rows only
  assert result_min   = min(U[i].value for i in 1..k)                                     // usage rows only
  assert result_max   = max(U[i].value for i in 1..k)                                     // usage rows only
  assert result_avg   = sum(U[i].value for i in 1..k) / k                                 // usage rows only
  assert no X[j].value is included in COUNT, MIN, MAX, or AVG
```

Acceptance assertion: `SUM(value)` MUST return
`sum(U[i].value) + sum(X[j].value)` (compensation values are
negative, so SUM nets); `COUNT` MUST return `k` (the number of
usage rows); `MIN`, `MAX`, `AVG` MUST be computed over `{U[i].value}`
only and MUST NOT include any `X[j].value`. Compensation entries
adjust SUM; they are not events.

## Error Taxonomy

All Plugin SPI methods return `Result<…, UsageCollectorPluginError>`.
`UsageCollectorPluginError` is declared in
`usage-collector-sdk/src/error.rs` as a flat `thiserror::Error` enum
and is the plugin-side error vocabulary. The SDK crate (which owns
both `UsageCollectorError` and `UsageCollectorPluginError`) **does
NOT depend on `toolkit-canonical-errors`**; plugin authors and the
host's dispatch boundary pattern-match `UsageCollectorPluginError`
variants directly.

The host crate translates `UsageCollectorPluginError` into
`UsageCollectorError` variants at the dispatch boundary in
`usage-collector/src/domain/service.rs`. The translation is exhaustive
and per-variant:

| `UsageCollectorPluginError` variant             | `UsageCollectorError` variant |
| ----------------------------------------------- | ----------------------------- |
| `Timeout`                                       | `PluginTimeout`               |
| `BackendError { kind, detail }`                 | `PluginFailure`               |
| `ContractViolation { detail }`                  | `Internal`                    |
| `MetricAlreadyExists { gts_id }`                | `MetricAlreadyExists`         |
| `MetricNotFound { gts_id }`                     | `MetricNotFound`              |
| `MetricReferenced { gts_id, sample_ref_count }` | `MetricReferenced`            |
| `InvalidKindPrefix { gts_id }`                  | `InvalidKindPrefix`           |
| `UnknownMetadataKey { gts_id, key }`            | `UnknownMetadataKey`          |

`ContractViolation` lifts to `UsageCollectorError::Internal` (not
`PluginFailure`) because the Plugin Host classifies it as a
fail-closed gear-internal error rather than a backend issue; the
`PluginFailure` slot is reserved for backend-classified failures
matching the `BackendError` semantics (`sdk-trait.md` "Variant
catalog"). The catalog-domain variants `MetricAlreadyExists`,
`MetricNotFound`, and `MetricReferenced` are plugin-surfaced now that
the Metric Catalog (managed via the Plugin SPI, persisted in the
active storage plugin's database) is the sole metric catalog per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)):
they originate from Methods 6 / 9 inside the plugin and are
translated by the dispatch boundary to the corresponding
`UsageCollectorError` variants for the SDK / REST surface (HTTP
mapping: `MetricAlreadyExists → 409`, `MetricNotFound → 404`,
`MetricReferenced → 409`). The lift to the canonical `Problem`
envelope happens **downstream** of this translation, in the host's
REST layer at `usage-collector/src/infra/sdk_error_mapping.rs`
(`From<UsageCollectorError> for CanonicalError`); the plugin SDK
itself never sees `toolkit-canonical-errors`.

**Removed in ADR 0012:** the `DeclaredMetricImmutable` variant and
the "declared metric" boot-seeded notion are gone — the
gateway-local-from-config catalog has been retired and the
Metric Catalog is the sole metric catalog.

The Plugin Host also projects each variant onto the operational-metric
`error_category` label set documented for
`usage_collector.plugin.accept_errors` in DESIGN §3.11.7 (`unready`,
`backend_error`, `timeout`, `contract_violation`). The `unready` label
records structural unavailability — `ClientHub::try_get_scoped` returns
`None` and the host lifts that to `PluginUnavailable`; the SPI itself
exposes no `Unready` error variant and no `ready()` probe. Cursor
validity is NOT a plugin-error category — cursor
decode failure, order mismatch, and filter mismatch are caught by the
gateway before plugin dispatch and surfaced as
`UsageCollectorError::Validation` variants whose host lift produces
canonical `Problem` responses (`cursor_decode`, `order_mismatch`,
`filter_mismatch`), anchored by
`cpt-cf-usage-collector-principle-cursor-gateway-ownership`.

Variant catalog:

- `Timeout` — the SPI call exceeded its declared per-method timeout.
  Maps to the `timeout` label on `usage_collector.plugin.accept_errors`.
- `BackendError { kind, detail }` — the backend reported a classified
  error other than a timeout (for example transient I/O failure,
  malformed backend response, exhausted connection pool); structural
  unavailability is host-side and surfaces as `PluginUnavailable`, not
  as a `BackendError`.
  `kind` is a stable backend-classified enum the plugin defines for
  its own backend; `detail` is operator-facing. Maps to the
  `backend_error` label on `usage_collector.plugin.accept_errors`.
- `ContractViolation { detail }` — the call violated an SPI contract
  the plugin can detect (for example an empty batch, an
  `AggregationQuery` with an `aggregation` the plugin does not
  support, or a Method 4 invocation that contradicts the canonical
  `(timestamp, id)` keyset). Maps to the `contract_violation` label
  on `usage_collector.plugin.accept_errors`. The Plugin Host
  treats `ContractViolation` as a fail-closed gear-internal error
  rather than a backend issue. Cursor decode failure, order
  mismatch, and filter mismatch on raw queries are NOT reported as
  `ContractViolation` — they are gateway-only failures surfaced as
  canonical Problem responses (`cursor_decode`, `order_mismatch`,
  `filter_mismatch`) before any plugin dispatch.
- `MetricAlreadyExists { gts_id }` — Method 6 (`register_metric`)
  saw a row with the same `gts_id` already present, and the request
  payload differs from the stored row. Surfaced by the dispatch
  boundary as `UsageCollectorError::MetricAlreadyExists` and mapped
  to HTTP 409 on the REST surface. An identical-payload
  resubmission MUST NOT raise this variant (Method 6 is idempotent
  on `gts_id` for byte-equal payloads).
- `MetricNotFound { gts_id }` — raised by Method 9 (`delete_metric`)
  when no `metric_catalog` row has the supplied `gts_id`. Method 7
  (`read_metric`) does NOT raise this variant — a miss is surfaced
  as `Ok(None)` so the gateway can distinguish "no such metric"
  from a backend failure without pattern-matching.
- `MetricReferenced { gts_id, sample_ref_count }` — Method 9
  (`delete_metric`) was rejected by the `usage_records.gts_id`
  `ON DELETE RESTRICT` FK. `sample_ref_count` carries a bounded
  sample of how many referencing rows the plugin observed (the
  plugin MUST NOT scan the entire table to compute an exact count —
  a small sample sufficient to confirm "still referenced" is
  enough). Surfaced by the dispatch boundary as
  `UsageCollectorError::MetricReferenced` and mapped to HTTP 409 on
  the REST surface.
- `InvalidKindPrefix { gts_id }` — Method 6 (`register_metric`)
  received a `gts_id` that does not begin with one of the two
  reserved kind base type prefixes
  (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`)
  per ADR 0012. The variant is raised at the gateway boundary
  (kind-prefix validation runs before plugin dispatch) and is
  surfaced through the same dispatch translation as plugin-originated
  catalog errors so that the SDK/REST callers see a single
  catalog-error vocabulary. Plugins MUST NOT parse `gts_id` to
  re-check the prefix; they MUST NOT raise this variant themselves.
- `UnknownMetadataKey { gts_id, key }` — Methods 1 / 2
  (`persist_usage_record` / `persist_usage_records`) saw an incoming
  `UsageRecord.metadata` key that is NOT a member of the metric's
  declared `metadata_fields` set per ADR 0012. The variant is raised
  at the gateway boundary (closed-shape membership runs against the
  L1 catalog cache before plugin dispatch) and is surfaced through
  the same dispatch translation so that the SDK/REST callers see a
  single closed-shape-violation vocabulary. Plugins MUST NOT
  re-check closed-shape membership and MUST NOT raise this variant
  themselves.

Behavioural notes:

- Exact-equality duplicate ingestion submissions are reported through
  the `PersistOutcome::Deduplicated` variant on the `Ok` arm, not as
  errors — this stays the silent-absorb success dedup ack. A same-key
  submission whose canonical fields differ from the stored record is
  reported through the `PersistOutcome::Conflict` variant on the `Ok`
  arm; the Plugin Host translates `PersistOutcome::Conflict` to
  `DedupOutcome::Conflict`, which the core surfaces to the caller as a
  `UsageCollectorError` (the `idempotency_conflict` rejection,
  AlreadyExists / `409`, DESIGN §3.3) — NOT an `Ok` ack — while
  `PersistOutcome::Deduplicated` translates to the success dedup ack.
  Repeat deactivation against an already-inactive record is
  reported through the `DeactivationOutcome::AlreadyInactive`
  variant on the `Ok` arm. Catalog admin failures use error
  variants: a same-`gts_id` register-conflict is surfaced as
  `MetricAlreadyExists`, a missing target row as `MetricNotFound`,
  and an FK-rejected delete as `MetricReferenced` per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)).
  The SPI uses error variants only for failures the Plugin Host
  must classify for retry or fail-closed disposition.
- The Plugin SPI does NOT surface `Authentication`, `Authorization`,
  `Validation`, or `UnknownMetric` variants because those failure
  classes are enforced upstream by
  `cpt-cf-usage-collector-component-ingestion-gateway`,
  `cpt-cf-usage-collector-component-query-gateway`,
  `cpt-cf-usage-collector-component-deactivation-handler`, and
  `cpt-cf-usage-collector-component-metric-catalog` (each performing
  PDP enforcement inline via the `authz_scope` helper) before any SPI
  call. A plugin that observed such a failure has, by definition,
  observed a contract violation by the Plugin Host and SHOULD return
  `ContractViolation` rather than inventing a new error class.
- The five compensation-related codes
  (`gauge_compensation_rejected`, `corrects_id_not_found`,
  `corrects_id_wrong_entry_type`, `corrects_id_wrong_scope`,
  `corrects_id_inactive`) are SDK/REST-surface errors enforced on
  the ingestion path before SPI dispatch and do NOT appear in any
  SPI method outcome — see `sdk-trait.md` §Error Taxonomy and
  `usage-collector-v1.yaml` per-record 207 outcomes.
- Variant naming is canonical for this reference; the SPI crate MAY
  add per-variant context fields (such as a stable error code or
  operational trace pointer) as long as the public taxonomy
  preserves the domain classification above and the
  `usage_collector.plugin.accept_errors` label mapping.

Sources: DESIGN §3.3 Plugin SPI capability list (error
classification); §3.5 Storage Plugin Contract ("Plugins MUST
classify backend errors so the gateway can apply retry,
circuit-break, or fail-closed behaviour without backend-specific
parsing"); §3.11.7 `usage_collector.plugin.ready` and
`usage_collector.plugin.accept_errors` label vocabulary.

## Consistency profile

The Plugin SPI inherits Usage Collector's plugin-agnostic consistency
floor and obliges every active plugin to publish its actual
consistency profile. The floor is the gear-level contract
documented in DESIGN §3.10.8 (Consistency contract) and
`cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011); this
section restates the floor on the SPI side and adds the per-plugin
deployment-guide obligation.

**SPI floor (normative).** The Plugin SPI's consistency floor is
identical to DESIGN §3.10.8's gear-level floor; nothing in the SPI
relaxes or strengthens it.

- **Ingestion ack** — once `persist_usage_record` /
  `persist_usage_records` return `PersistOutcome::Persisted` (or
  `Deduplicated` for an exact-equality retry), the record is durable
  per `cpt-cf-usage-collector-adr-pluggable-storage`; the
  `(tenant_id, gts_id, idempotency_key)` dedup tuple is
  permanently visible to subsequent persistence attempts on the
  ingestion path per §"Cross-entity invariants honored by the Plugin
  SPI" (strict dedup-key preservation, refining plugin-owned
  retention); and a subsequent `deactivate_usage_event` of the same
  row commits atomically with its depth-1 compensation cascade in a
  single backend transaction per Method 5 and
  `cpt-cf-usage-collector-adr-monotonic-deactivation` /
  `cpt-cf-usage-collector-adr-usage-compensation`. These are the
  in-transaction invariants the SPI already binds; the consistency
  contract restates them so the ingestion-side guarantee is named
  alongside the read-side guarantee.
- **Query SPI** — `query_aggregated`, `query_raw_keyset`,
  `read_metric`, and `list_metrics` are **eventually consistent with
  no upper bound** relative to a
  same-tenant ingestion ack. The same record MAY be invisible to any
  of those methods for an indeterminate window after acknowledgement;
  the window is driven by the plugin's chosen replication topology
  and the workload-isolation routing it implements per
  `cpt-cf-usage-collector-nfr-workload-isolation`. **No
  monotonic-reads guarantee at the floor.** The floor is per-`(tenant_id,
gts_id)`; the SPI publishes no cross-tenant or
  cross-metric ordering claim.
- **Scope** — the floor covers BOTH the plugin's `usage_records`
  table and the plugin-owned `metric_catalog` table per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
  The gateway's non-durable L1 validator cache is not part of the
  Plugin SPI surface and is governed by the catalog cache mechanics
  in DESIGN §3.11.1, not by this floor.
- **Within-transaction atomicity is not a cross-path guarantee.**
  The deactivate cascade documented in Method 5 commits as one
  backend transaction; that atomicity is a plugin-transaction
  invariant. A subsequent query against any read pool MAY observe a
  pre-cascade state until the pool converges. Consumers that need
  the post-cascade state for an immediate decision use the
  deactivate ack, not a follow-up query.

**Per-plugin profile (deployment-guide obligation).** Each
`usage-collector-plugin-<backend>` crate's deployment guide MUST
publish the plugin's actual consistency profile so consumers that
need a stronger bound can opt in by coupling to that plugin. The
profile is an honest description of the active deployment posture;
it is NOT a promise the SPI binds and it is NOT enforced by the host
or by Usage Collector itself.

- **Required content.** Every deployment guide MUST state, at
  minimum: (a) whether ingestion and query land on the same backend
  pool (sync, single-pool) or on isolated pools (asynchronous read
  replicas, separate executor pools); (b) the expected upper bound
  on Query-SPI lag relative to ingestion ack under the documented
  deployment posture (e.g., "sync — no observable lag",
  "bounded-staleness ≤ N ms with replication-lag alerting at N/2",
  or "eventual, no bound — see workload-isolation routing"); (c)
  whether monotonic-reads-per-`(tenant_id, gts_id)` holds
  under the default deployment posture, and if so, the configuration
  knobs that preserve it (e.g., session affinity, read-replica
  delay clamps, `select_sequential_consistency`); (d) whether the
  same profile applies to the catalog reads
  (`read_metric`, `list_metrics`) or whether the catalog has its own
  stronger / weaker bound; (e) the
  procedure operators MUST follow if they deploy outside the
  documented posture (custom routing, non-default replica counts,
  cross-region read pools) and how that procedure interacts with the
  published profile.
- **Consumer-coupling rule.** Consumers that depend on a tighter
  bound than the gear floor couple themselves to a specific
  plugin's published ceiling; that coupling is intentional and MUST
  be recorded in the consumer's own design document so a plugin
  substitution surfaces as a known impact rather than a latent
  regression.
- **Drift discipline.** A change to the published profile that
  weakens any guaranteed bound is treated as a breaking change for
  every consumer coupled to the prior profile; the deployment guide
  MUST announce such changes with the same notice expected for
  ingestion or query availability under
  `cpt-cf-usage-collector-nfr-availability-boundary`. A
  strengthening change is additive and does not require notice.

**No typed `consistency_profile()` SPI method in v1.** The SPI
surface does not carry a typed accessor for the profile. The Plugin
SPI's major-version contract per
`cpt-cf-usage-collector-adr-contract-stability` (ADR-0006) treats
new optional methods as additive, so a typed accessor MAY be added
in a later Plugin SPI minor release if a real consumer needs to
branch behavior on the profile at runtime. Until then, profile
discovery is documentation-only.

Sources: DESIGN §3.10.8 (Consistency contract);
`cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011);
`cpt-cf-usage-collector-nfr-workload-isolation`; §"Cross-entity
invariants honored by the Plugin SPI" (strict dedup-key preservation
and deactivate cascade atomicity).

## Versioning/Compatibility

- The Plugin SPI is one of three independently versioned public
  surfaces (REST API, SDK trait, Plugin SPI). Each surface evolves
  under a major-version stability contract
  (`cpt-cf-usage-collector-adr-contract-stability`,
  `cpt-cf-usage-collector-principle-contract-stability`,
  `cpt-cf-usage-collector-nfr-plugin-contract-stability`,
  `cpt-cf-usage-collector-constraint-plugin-contract-stability`).
- The Plugin SPI's major version is encoded in the trait name
  suffix `V1`. A new major version (`V2`, and so on) is required for
  any breaking change.
- Within a major version only additive changes are permitted: new
  optional methods (with default implementations so existing plugin
  crates keep compiling), new optional fields on input types, new
  non-required variants on output enums, and new optional outcome
  variants. Removing methods, removing or renaming fields, narrowing
  accepted values, changing semantics, introducing a new required
  input, or removing a `default` implementation from a previously
  defaulted method is a breaking change and requires a new major
  version.
- Logical-table schema versioning is owned at the Plugin SPI surface
  per DESIGN §3.10.7: additions to the §3.7 logical record shape
  (new optional `usage_records` columns, new enum members) are
  additive within the current Plugin SPI major version; removals or
  semantic changes require a new major version.
- Deprecation flow: a Plugin SPI method, outcome variant, or field
  scheduled for removal in the next major release MUST be marked
  `deprecated` in the SPI trait rustdoc at least one minor release
  before the major bump.
- At most one prior major version is supported concurrently per
  surface, per `cpt-cf-usage-collector-nfr-plugin-contract-stability`.
  A Usage Collector gear instance MAY bind a `V1` plugin while
  another instance binds a `V2` plugin during a deprecation window;
  one Plugin Host instance binds exactly one plugin instance at a
  time per `cpt-cf-usage-collector-component-plugin-host`
  Responsibility boundaries.
- Compile-time Rust trait compatibility tests gate every PR against
  the prior major per DESIGN §3.12.3 Contract test row.
- Per-method timeouts are part of the SPI rustdoc (one per method,
  bounded by the per-operation latency budgets in DESIGN §3.11.2:
  75 ms ingestion p95, 425 ms aggregated query p95, 1 s raw-page
  p95). A change to a per-call timeout value is an additive
  observation, not a breaking change to the trait shape.
- Plugins ship on independent release schedules from the Usage
  Collector itself per `cpt-cf-usage-collector-principle-pluggable-storage`.

Sources: DESIGN §3.10.7 Schema Versioning; §3.12.8 Versioning and
Deprecation Policy; §3.12.3 Test Strategy (Contract row); §3.11.2
Latency Budgets; §1.2 NFR row for
`cpt-cf-usage-collector-nfr-plugin-contract-stability`.

## Exclusions/Non-goals

### SDK-trait-only exclusions

The Plugin SPI does not expose the operator-and-developer-facing
shape that the SDK trait surfaces; the SDK trait
(`cpt-cf-usage-collector-interface-sdk-client`, `sdk-trait.md`) owns
the consumer-facing concerns the SPI deliberately avoids:

- `UsageRecordSubmission` shape and dedup-outcome translation into
  `UsageRecordAck` / `DedupOutcome` are SDK-side concerns; the SPI
  returns the raw `PersistOutcome` so the Plugin Host can adapt to
  the SDK shape.
- `DeactivationAck` shape and the SDK-side `AlreadyInactive` /
  `NotFound` error variants are SDK-side; the SPI returns
  `DeactivationOutcome` and lets the Deactivation Handler translate
  it.
- The SDK-trait per-call timeout values, the SDK-trait `Result`
  shape, and the SDK error taxonomy are SDK-side; the SPI has its
  own taxonomy.

Sources: `sdk-trait.md` §"Plugin SPI exclusions" inverted; DESIGN
§3.6 Deactivate Usage Event.

### REST-only exclusions

The Plugin SPI does not expose REST-handling concerns:

- The OpenAPI wire contract (`usage-collector-v1.yaml`),
  endpoint paths, and request/response schemas remain REST-side.
- RFC-9457 `Problem` envelope conversion is performed by the
  REST handler in the gear crate.
- CORS, TLS termination, output encoding, and HTTP-level rate
  limiting are platform API gateway responsibilities per DESIGN
  §3.9.3.
- Platform liveness and readiness probes are handled by the ToolKit host above the gear boundary; the collector exposes no gear-local health endpoints. Operational telemetry is pushed via OTLP from ToolKit's global `SdkMeterProvider` (no in-gear `/metrics` scrape endpoint exists). The SPI contributes no `ready` or `flush` operation; the structural readiness fact (selector cached AND `ClientHub::try_get_scoped` returns `Some`) is composed by the Plugin Host and surfaced via the `usage_collector.plugin.ready` gauge.

Sources: DESIGN §3.3 REST API row; §3.9.3 Security Boundaries;
§3.11.5 Observability Architecture.

### Gear non-goals reaffirmed on the Plugin SPI

- A dedicated backfill capability (watermarks, late-data
  coordination, or a bulk-import method beyond `persist_usage_records`)
  is an explicit non-goal in v1. Old event timestamps are accepted
  without wall-clock validation, so bulk historical import uses the
  same `persist_usage_records` path with each record's true event
  timestamp (which still requires per-record idempotency keys and
  triggers the same dedup contract); see the timestamp /
  late-arrival invariant in `domain-model.md` §2.1 for the
  consequences for raw-tail consumers.
- Individual record amendment beyond deactivation is intentionally
  omitted; the SPI provides no `update_record` method. Corrections
  follow the §4 forward-looking pattern: deactivate the prior
  record, then emit a fresh idempotency-keyed record.
- Reactivation (`inactive → active`) is intentionally omitted; the
  SPI provides no transition for it per
  `cpt-cf-usage-collector-principle-monotonic-deactivation`.
- Multi-region deployment is not a v1 capability of the gear;
  cross-region durability, read locality, and conflict resolution
  remain plugin-deployment and platform-topology concerns per
  DESIGN §3.10.6.
- Gear-emitted audit events for operator-write paths are not the
  SPI's responsibility; the v1 access trail is composed at the
  gateway and PDP decision points per DESIGN §3.9.5 and §4.
- Pricing, rating, billing, invoice generation, and quota
  decisions are out of scope for every SPI operation per
  `cpt-cf-usage-collector-constraint-no-business-logic`. The plugin
  MUST NOT decide refunds, credits, credit-notes, or net-non-negative
  enforcement; per
  `cpt-cf-usage-collector-adr-usage-compensation`, recording a
  caller-supplied negative quantity (an
  `entry_type = compensation` row with `value < 0`) is **recording,
  not computing**. Per-record remaining-amount tracking, lot /
  FIFO-LIFO accounting, and negative-`SUM` detection / alerting are
  explicit non-goals.
- Gear-side caching of PDP decisions is forbidden per
  `cpt-cf-usage-collector-principle-pdp-centric-authorization`; the
  SPI sees only post-authorization queries.
- At-rest encryption, key management, masking, disposal, backup,
  point-in-time recovery, disaster recovery, replication, tiering,
  retention windows, archival, compression, encoding, partitioning,
  and acceleration structures beyond the architectural
  `(tenant_id, gts_id, timestamp)` index of DESIGN §3.7 are
  plugin-owned and operator-tuned, not part of the SPI contract. This
  plugin ownership of retention and archival is refined, not
  contradicted, by the strict dedup-key-preservation obligation in
  §"Cross-entity invariants honored by the Plugin SPI": the plugin
  still owns retention but MUST NOT free a
  `(tenant_id, gts_id, idempotency_key)` dedup key when it purges
  or archives the corresponding record bodies.
- Dead-letter queue, poison-message handling, and compensation-saga
  patterns are out of scope for the SPI; persistence is a single
  synchronous call that either succeeds (`Persisted`), deduplicates an
  exact-equality retry (`Deduplicated`), surfaces a canonical-field
  mismatch as `Conflict`, or returns a classified error per DESIGN
  §3.10.2.

Sources: DESIGN §3.10.2 DLQ and Poison-Message Applicability;
§3.10.6 Data Tiering and Archival Applicability; §3.13.3 Compliance
Mapping Applicability; §4 Additional context (v1 non-goals).

## Traceability

### Surface identifier and consumer contract

- `cpt-cf-usage-collector-interface-plugin` — the public Plugin SPI
  interface identifier carried by `UsageCollectorPluginV1`. Source:
  DESIGN §3.3 "Plugin SPI" row.
- `cpt-cf-usage-collector-contract-storage-plugin` — the consumer
  contract realized by the SPI. Source: DESIGN §3.3 Plugin SPI
  "Contracts" row; §3.5 Storage Plugin Contract.

### Capabilities exposed by the Plugin SPI

- Persist single record and persist batched records:
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-fr-record-metadata`,
  `cpt-cf-usage-collector-seq-emit-usage`. Throughput and
  batch-and-report timing:
  `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-throughput-profile`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`. Ingestion
  latency:
  `cpt-cf-usage-collector-nfr-ingestion-latency` (Plugin SPI
  allocation per §3.11.2). Sources: DESIGN §3.3, §3.6, §3.11.2.
- Aggregated query:
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-seq-query-aggregated`,
  `cpt-cf-usage-collector-nfr-query-latency`. Source: DESIGN §3.3,
  §3.6, §3.11.2.
- Raw cursor-paginated query:
  `cpt-cf-usage-collector-fr-pluggable-storage`,
  `cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-seq-query-raw`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`. Source:
  DESIGN §3.3, §3.6.
- Deactivate usage event (depth-1 atomic set flip):
  `cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-seq-deactivate-event`,
  `cpt-cf-usage-collector-adr-monotonic-deactivation`,
  `cpt-cf-usage-collector-adr-usage-compensation`,
  `cpt-cf-usage-collector-principle-monotonic-deactivation`. Source:
  DESIGN §3.3 Deactivate response shape; §3.6 Deactivate Usage Event;
  §3.10.3 Correction posture (deactivation cascade).
- Counter compensation (value-reversal; rides Method 1 / Method 2):
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-adr-usage-compensation`,
  `cpt-cf-usage-collector-entity-entry-type`. Source: DESIGN §3.3
  Unified ingestion request shape; §3.10.3 Correction posture
  (compensation primitive); ADR-0008 Decision and Consequences.
- Catalog write / read / list / delete (Methods 6–9):
  `cpt-cf-usage-collector-fr-metric-registration`,
  `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-fr-metric-deletion`,
  `cpt-cf-usage-collector-seq-register-metric`,
  `cpt-cf-usage-collector-seq-delete-metric`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
  Per ADR 0012, the Metric Catalog (managed via the Plugin SPI,
  persisted in the active storage plugin's database) is the sole
  metric catalog: its rows live alongside `usage_records` and the FK
  `usage_records.gts_id → metric_catalog(gts_id) ON DELETE RESTRICT`
  is enforced natively. The gateway owns the semantic surface (PDP,
  validation, schema authority) and dispatches catalog ops through
  these four methods. The catalog row carries `metadata_fields:
Vec<String>` (the closed, declared list of allowed metadata key
  names; stored verbatim, all values typed as String end-to-end); the
  gateway L1 catalog cache holds `Map<gts_id, {kind: derived,
metadata_fields: HashSet<String>}>` with `kind` derived once on
  cache load from the `gts_id` prefix. The in-plugin reference scheme
  (column type, index choice) is the plugin author's choice and out
  of SPI scope. Source: DESIGN §3.6 Register Metric /
  Delete Metric sequences; §3.7 Tables `metric_catalog` and
  `usage_records`;
  [`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md).
- Readiness and graceful shutdown:
  `cpt-cf-usage-collector-nfr-availability`,
  `cpt-cf-usage-collector-nfr-availability-boundary`,
  `cpt-cf-usage-collector-nfr-graceful-degradation`. Source: DESIGN
  §3.10.4 Graceful shutdown; §3.11.7 `usage_collector.plugin.ready`
  gauge (the structural readiness fact pushed via OTLP).

### Domain entities

- `cpt-cf-usage-collector-entity-usage-record`,
  `cpt-cf-usage-collector-entity-entry-type`,
  `cpt-cf-usage-collector-entity-resource-ref`,
  `cpt-cf-usage-collector-entity-subject-ref`,
  `cpt-cf-usage-collector-entity-metric`,
  `cpt-cf-usage-collector-entity-metric-kind`,
  `cpt-cf-usage-collector-entity-idempotency-key`,
  `cpt-cf-usage-collector-entity-record-metadata`,
  `cpt-cf-usage-collector-entity-deactivation-status`,
  `cpt-cf-usage-collector-entity-aggregation-query`,
  `cpt-cf-usage-collector-entity-raw-query`,
  `cpt-cf-usage-collector-entity-aggregation-result`,
  `cpt-cf-usage-collector-entity-usage-record-filter-field`,
  `cpt-cf-usage-collector-entity-keyset`,
  `cpt-cf-usage-collector-entity-plugin-binding`. Source: DESIGN
  §3.1 Domain Model; `domain-model.md` §2 Core Entities, §3 Query
  Domain, §5 Plugin Binding Domain. Raw-page output is the canonical
  `toolkit_odata::Page<UsageRecord>` shape; cursor lifecycle is
  realized by `toolkit_odata::CursorV1` plus
  `validate_cursor_against`, per
  `cpt-cf-usage-collector-principle-cursor-gateway-ownership`. The
  former gear-owned `RawRecordPage` and `CursorToken` entities
  defined in earlier drafts of `domain-model.md` are no longer
  carried by this SPI (phase-01
  `out/phase-01-domain-contracts.md` §1).

### Components allocated to the Plugin SPI

- `cpt-cf-usage-collector-component-plugin-host` — the sole
  in-process component that dispatches against the SPI per DESIGN
  §3.3 ("Allocated To") and §3.2 Plugin Host Responsibility scope.
- The SPI is also a downstream collaborator of
  `cpt-cf-usage-collector-component-ingestion-gateway`,
  `cpt-cf-usage-collector-component-query-gateway`,
  `cpt-cf-usage-collector-component-deactivation-handler`, and
  `cpt-cf-usage-collector-component-metric-catalog`, but only the
  Plugin Host calls the SPI directly per §3.2 Plugin Host
  Responsibility scope.

### Persistence anchors

- `cpt-cf-usage-collector-dbtable-usage-records` — durable rows
  emitted by Methods 1 / 2 and read by Methods 3 / 4; status
  updated by Method 5. Carries `gts_id TEXT NOT NULL REFERENCES
metric_catalog(gts_id) ON DELETE RESTRICT` per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
  The in-plugin column type and index choice are the plugin author's
  choice and out of SPI scope.
- `cpt-cf-usage-collector-dbtable-metric-catalog` — durable catalog
  rows written by Method 6, read by Methods 7 / 8, and deleted by
  Method 9. Owned by the plugin per ADR 0012 so the cross-table FK
  with `usage_records` is enforceable natively.
  Source: DESIGN §3.7 Database schemas & tables; ADR-0009; ADR-0010.

### Authorization, fail-closed, and attribution anchors (exclusions)

The SPI does NOT participate in any of the following — these anchors
are listed so reviewers can confirm the SPI's responsibility
boundary against them:

- `cpt-cf-usage-collector-contract-authz-resolver`,
  `cpt-cf-usage-collector-principle-fail-closed`,
  `cpt-cf-usage-collector-principle-pdp-centric-authorization`,
  `cpt-cf-usage-collector-adr-pdp-centric-authorization`,
  `cpt-cf-usage-collector-adr-caller-supplied-attribution`,
  `cpt-cf-usage-collector-adr-mandatory-idempotency`,
  `cpt-cf-usage-collector-constraint-pii-identity-layer`,
  `cpt-cf-usage-collector-constraint-no-business-logic`. Source:
  DESIGN §3.2 "Plugin Host" Responsibility boundaries; §3.9.6
  Authorization Architecture; §3.10.1 Fault Tolerance Defaults
  ("Retries: Caller-owned … made safe by mandatory idempotency").
- Kind-invariant enforcement
  (`cpt-cf-usage-collector-fr-counter-semantics`,
  `cpt-cf-usage-collector-fr-gauge-semantics`) is upstream in
  `cpt-cf-usage-collector-component-ingestion-gateway` /
  `cpt-cf-usage-collector-component-metric-catalog`; the SPI
  persists `value` byte-for-byte without invariant enforcement.

### Versioning, stability, and quality NFR anchors

- `cpt-cf-usage-collector-adr-contract-stability`,
  `cpt-cf-usage-collector-adr-pluggable-storage`,
  `cpt-cf-usage-collector-principle-contract-stability`,
  `cpt-cf-usage-collector-principle-pluggable-storage`,
  `cpt-cf-usage-collector-nfr-plugin-contract-stability`,
  `cpt-cf-usage-collector-nfr-ingestion-latency`,
  `cpt-cf-usage-collector-nfr-query-latency`,
  `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-throughput-profile`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`,
  `cpt-cf-usage-collector-nfr-workload-isolation`,
  `cpt-cf-usage-collector-nfr-graceful-degradation`,
  `cpt-cf-usage-collector-nfr-availability`,
  `cpt-cf-usage-collector-nfr-availability-boundary`,
  `cpt-cf-usage-collector-nfr-error-experience`,
  `cpt-cf-usage-collector-nfr-developer-operator-experience`,
  `cpt-cf-usage-collector-nfr-documentation-coverage`,
  `cpt-cf-usage-collector-constraint-plugin-contract-stability`,
  `cpt-cf-usage-collector-constraint-vendor-pluggable`. Source:
  DESIGN §3.10.7 Schema Versioning; §3.12.8 Versioning and
  Deprecation Policy; §3.11.2 Latency Budgets; §1.2 NFR rows
  enumerated by ID.
- `cpt-cf-usage-collector-adr-consistency-contract` —
  floor-and-ceiling consistency contract restated on the SPI side in
  §"Consistency profile"; obliges every active plugin's deployment
  guide to publish its actual profile; no typed
  `consistency_profile()` SPI method in v1. Source: DESIGN §3.10.8
  Consistency contract; ADR-0011 Decision and Consequences;
  §"Cross-entity invariants honored by the Plugin SPI" (the
  in-transaction invariants the floor cites).

## Open Questions

These are residual choices the `usage-collector-sdk` crate may
finalize during implementation. None block this reference; each notes
the conservative default this reference adopts.

- OQ-1 — Whether `BatchPersistOutcome` is a typed wrapper or a bare
  `Vec<Result<PersistOutcome, UsageCollectorPluginError>>`. This
  reference adopts a bare `Vec` for ergonomics with `Iterator` and
  `?` propagation in the Plugin Host; the SPI crate MAY wrap it in
  a named struct in a future minor version without changing the
  variant catalog.
- OQ-2 — **Resolved (catalog is a Plugin SPI capability; snapshot
  reads out-of-scope for v1)**: prior drafts asked whether
  `read_metric` and `list_metrics` accept an explicit `at` timestamp
  for snapshot reads. Per
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md))
  the Metric Catalog (managed via the Plugin SPI, persisted in the
  active storage plugin's database) is the sole metric catalog and
  is a Plugin SPI capability (Methods 6 / 7 / 8 / 9). The
  `at`-timestamp snapshot variant of Methods 7 / 8 is deliberately
  **omitted** in v1 — the catalog is read at "now" semantics only,
  the gateway L1 cache reconciles via synchronous invalidation on
  Methods 6 / 9, and historical / point-in-time catalog reads are a
  non-goal. A future minor version MAY add an optional
  `at: Option<Timestamp>` field on the read methods without breaking
  compatibility per §"Versioning / Compatibility".
- OQ-3 — Whether `aggregate_usage` accepts a per-call hint at
  acceleration structures (for example "prefer pre-aggregated
  rollups" or "fall back to scan"). This reference omits the hint;
  plugins choose acceleration internally to meet
  `cpt-cf-usage-collector-nfr-query-latency`. Hints MAY be added as
  optional fields on `AggregationQuery` in a future minor version
  without breaking compatibility.
- OQ-4 — Whether the SPI surfaces a separate `health` probe distinct
  from `ready`. **Resolved (no separate probe)**: this reference
  exposes **no** plugin-side readiness or health probe at all.
  Plugin availability is detected structurally by the Plugin Host
  via `GtsPluginSelector::get_or_init` (selector cached) AND
  `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns
  `Some` — these two structural facts are the only "is the plugin
  live?" signal, matching the reference-gear pattern in
  `credstore`, `authn-resolver`, and `authz-resolver`. Liveness is
  the Plugin Host's process-level health, observed by the ToolKit
  host outside the gear surface (the collector exposes no
  gear-local liveness endpoint). Plugins MAY expose
  backend-internal liveness through backend-specific metrics under
  their own `usage_collector_*` prefix per §3.11.7.
- OQ-5 — Whether `flush` accepts a deadline parameter or relies on
  the Plugin Host's operator-tuned drain timeout. **Resolved (no
  flush)**: this reference exposes no plugin-side flush hook;
  graceful shutdown is the Plugin Host's process-level lifecycle
  responsibility, not an SPI call. Plugins that buffer writes
  internally MUST drain on their own `Gear::shutdown` via the
  ToolKit gear lifecycle.
- OQ-6 — Whether trace context is passed as an explicit parameter or
  carried by the ambient task-local span. **Resolved (ambient
  context)**: this reference carries trace context via the active
  `tracing::Span` / OpenTelemetry context — no explicit `TraceContext`
  parameter appears on any SPI method, mirroring the reference plugin
  traits in `credstore`, `authn-resolver`, and `authz-resolver`. The
  Plugin Host opens the per-call span via
  `#[tracing::instrument(...)]` on its own `Service::*` methods before
  dispatching to the trait method; the SPI implementation runs inside
  that ambient span and continues it over the backend dispatch so the
  DESIGN §3.11.5 propagation invariant is satisfied without a
  syntactic parameter.
- OQ-7 — Whether DESIGN §3.11.2 should carve formal Plugin-SPI
  sub-allocations for the batched-ingestion (Method 2) and
  raw-cursor-paginated-query (Method 4) end-to-end envelopes in
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`. Today
  §3.11.2 carves sub-budgets only for ingestion (75 ms of 200 ms)
  and aggregated query (425 ms of 500 ms); this reference adopts the
  conservative "treat the SPI fraction as the dominant share with
  ≥ 25 ms reserved for gateway + PDP enforcement + core overhead" pattern
  in the meantime. A formal sub-allocation is a follow-up against
  DESIGN.md and is out of scope for this reference.

Sources: DESIGN §3.11.1 Performance Patterns / Caching; §3.11.5
Distributed tracing pattern; §3.12.8 Versioning and Deprecation
Policy.

## Document Changelog

- **2026-06-02 (amendment)** — Aligned with the ADR-0012 2026-06-02
  amendment (simplifications 5 and 6, per
  [`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)
  Amendment block). Replaced the open-but-typed schema surface and
  the trait map on the catalog row with a single
  `metadata_fields: Vec<String>` field (closed, declared list of
  allowed metadata key names; all values typed as String end-to-end);
  removed the per-metric schema validator compile and the schema
  runtime dependency from the gateway L1 description.
  The catalog row no longer carries a `kind` column — `kind ∈
{counter, gauge}` is derived from the `gts_id` prefix matching one
  of the two reserved kind base type prefixes
  `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`;
  `register_metric` now rejects identifiers that do not begin with
  one of those two prefixes with `InvalidKindPrefix { gts_id }`.
  Added the `UnknownMetadataKey { gts_id, key }` error variant for
  ingest-time closed-shape membership violation; removed the
  schema-validation error surface. Gateway L1 cache shape is now
  flat `Map<gts_id, {kind: derived, metadata_fields: HashSet<String>}>`
  with no merge core, no schema-merge cascade, and a flat keyspace
  for invalidation.
- **2026-06-02** — Aligned with ADR 0012 ([`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`](./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md)).
  Renamed catalog SPI methods to `register_metric` / `read_metric` /
  `list_metrics` / `delete_metric`; removed `read_metric_chain`
  (Method 10); reduced the pre-amendment catalog row to `gts_id` (PK) /
  the open-but-typed schema surface / the trait map / `created_at`;
  removed the prior catalog-row fields for ancestor-pointer, abstract /
  non-abstract distinction, type-uuid, type-id, and per-property
  indexable annotation from the SPI surface; usage records now reference metrics by
  `gts_id` (not `metric_type_uuid`); REST/SDK error variants renamed
  `MetricTypeNotFound` → `MetricNotFound` and `MetricTypeAlreadyExists`
  → `MetricAlreadyExists`. Stated explicitly that the in-plugin
  reference scheme (column type, index choice) is the plugin author's
  choice and out of SPI scope. Preserved unrelated SPI behaviour:
  per-metric declared-key surface, lifecycle ops, idempotency tuple
  (re-keyed to `gts_id`), consistency-contract calls, late-arrival
  semantics. (This entry was subsequently superseded in part by the
  2026-06-02 amendment entry above.)
