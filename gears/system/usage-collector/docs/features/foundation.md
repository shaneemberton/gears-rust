<!--
cpt:
  version: 0.2.1
  updated: 2026-06-02
-->

# Feature: Gear Foundation & Pluggable Storage

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Plugin Host Binding (Lazy Resolve)](#plugin-host-binding-lazy-resolve)
  - [PDP Authorize and Constraint Return](#pdp-authorize-and-constraint-return)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Plugin Host Binding (Lazy Resolution)](#plugin-host-binding-lazy-resolution)
  - [Database binding](#database-binding)
  - [PDP Authorize](#pdp-authorize)
  - [Audit-Correlation Propagation](#audit-correlation-propagation)
  - [Tenant Isolation Enforcement](#tenant-isolation-enforcement)
- [4. Definitions of Done](#4-definitions-of-done)
  - [FR: Pluggable Storage](#fr-pluggable-storage)
  - [FR: AuthN Delegation](#fr-authn-delegation)
  - [FR: Audit Trail](#fr-audit-trail)
  - [FR: Tenant Isolation](#fr-tenant-isolation)
  - [FR: Data Classification](#fr-data-classification)
  - [FR: Standards Compliance](#fr-standards-compliance)
  - [FR: Non-Repudiation](#fr-non-repudiation)
  - [FR: Privacy Controls](#fr-privacy-controls)
  - [FR: Data Ownership](#fr-data-ownership)
  - [NFR: Availability](#nfr-availability)
  - [NFR: Scalability](#nfr-scalability)
  - [NFR: Plugin Contract Stability](#nfr-plugin-contract-stability)
  - [NFR: Authentication](#nfr-authentication)
  - [NFR: Authorization](#nfr-authorization)
  - [NFR: Capacity Headroom](#nfr-capacity-headroom)
  - [NFR: Deployment Operations](#nfr-deployment-operations)
  - [NFR: Developer & Operator Experience](#nfr-developer--operator-experience)
  - [NFR: Documentation Coverage](#nfr-documentation-coverage)
  - [NFR: Error Experience](#nfr-error-experience)
  - [NFR: Graceful Degradation](#nfr-graceful-degradation)
  - [NFR: Operational Visibility](#nfr-operational-visibility)
  - [NFR: Support Readiness](#nfr-support-readiness)
  - [Principle: Fail Closed](#principle-fail-closed)
  - [Principle: Pluggable Storage](#principle-pluggable-storage)
  - [Principle: Contract Stability](#principle-contract-stability)
  - [Principle: PDP-Centric Authorization](#principle-pdp-centric-authorization)
  - [Constraint: Plugin Contract Stability](#constraint-plugin-contract-stability)
  - [Constraint: Vendor Pluggable](#constraint-vendor-pluggable)
  - [Constraint: Resource Platform-Owned](#constraint-resource-platform-owned)
  - [Constraint: NFR Thresholds](#constraint-nfr-thresholds)
  - [ADR: Contract Stability](#adr-contract-stability)
  - [ADR: PDP-Centric Authorization](#adr-pdp-centric-authorization)
  - [ADR: Pluggable Storage](#adr-pluggable-storage)
  - [Contract: Storage Plugin](#contract-storage-plugin)
  - [Contract: AuthZ Resolver](#contract-authz-resolver)
  - [Contract: GTS Registry](#contract-gts-registry)
  - [Entity: PluginBinding](#entity-pluginbinding)
  - [Entity: SecurityContext](#entity-securitycontext)
  - [Entity: PdpDecision](#entity-pdpdecision)
  - [Entity: PdpConstraint](#entity-pdpconstraint)
  - [Component: Plugin Host](#component-plugin-host)
  - [§2.1-item → DoD-ID Coverage Matrix](#21-item--dod-id-coverage-matrix)
- [5. Acceptance Criteria](#5-acceptance-criteria)
- [6. Changelog](#6-changelog)

<!-- /toc -->

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-foundation`

## 1. Feature Context

- [ ] `p1` - `cpt-cf-usage-collector-feature-foundation`

### 1.1 Overview

Establishes the Usage Collector's stateless gear runtime substrate and its three public contract surfaces — the in-process SDK trait, the REST API, and the storage Plugin SPI — so that every later capability (Metric Catalog, Usage Emission, Usage Query, Event Deactivation) plugs into a single, identical execution shape with foundation-owned plugin host binding, `SecurityContext` acceptance plus per-domain-component PDP dispatch, audit-correlation propagation, tenant isolation, and deployment topology; operational metrics reach the platform exclusively through OTLP push via ToolKit's `SdkMeterProvider`, and platform liveness/readiness probes are handled by the ToolKit host above the gear boundary (no gear-local health endpoints are exposed). Authentication is owned by the ToolKit gateway upstream of the collector; every request arrives carrying a resolved `SecurityContext`, and PDP enforcement is dispatched by each domain component (ingestion-gateway, query-gateway, deactivation-handler, metric-catalog) through a shared `authz_scope` helper per `cpt-cf-usage-collector-contract-authz-resolver`.

### 1.2 Purpose

This feature exists so safety-critical behavior — fail-closed authentication, PDP-mediated authorization, audit-correlation propagation, and tenant isolation — is realized once at the substrate layer rather than re-implemented per feature, and so storage vendors can ship and migrate backends independently of the core release train through a contract-stable Plugin SPI bound through the GTS Registry and ClientHub.

**Requirements**: `cpt-cf-usage-collector-fr-pluggable-storage`, `cpt-cf-usage-collector-fr-authn-delegation`, `cpt-cf-usage-collector-fr-audit-trail`, `cpt-cf-usage-collector-fr-tenant-isolation`, `cpt-cf-usage-collector-fr-data-classification`, `cpt-cf-usage-collector-fr-standards-compliance`, `cpt-cf-usage-collector-fr-non-repudiation`, `cpt-cf-usage-collector-fr-privacy-controls`, `cpt-cf-usage-collector-fr-data-ownership`, `cpt-cf-usage-collector-nfr-availability`, `cpt-cf-usage-collector-nfr-scalability`, `cpt-cf-usage-collector-nfr-plugin-contract-stability`, `cpt-cf-usage-collector-nfr-authentication`, `cpt-cf-usage-collector-nfr-authorization`, `cpt-cf-usage-collector-nfr-capacity-headroom`, `cpt-cf-usage-collector-nfr-deployment-operations`, `cpt-cf-usage-collector-nfr-developer-operator-experience`, `cpt-cf-usage-collector-nfr-documentation-coverage`, `cpt-cf-usage-collector-nfr-error-experience`, `cpt-cf-usage-collector-nfr-graceful-degradation`, `cpt-cf-usage-collector-nfr-operational-visibility`, `cpt-cf-usage-collector-nfr-support-readiness`

**Principles**: `cpt-cf-usage-collector-principle-fail-closed`, `cpt-cf-usage-collector-principle-pluggable-storage`, `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`, `cpt-cf-usage-collector-principle-contract-stability`, `cpt-cf-usage-collector-principle-pdp-centric-authorization`

**Platform dependencies (foundation-level)**: `toolkit` (gear wiring, `#[toolkit::gear]`, `ClientHub`), `toolkit-gts` (`PluginV1<P>` GTS base type and the `gts_type_schema` derive consumed by `usage-collector-sdk/src/gts.rs` to declare `UsageCollectorPluginSpecV1` per `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`), `types-registry-sdk` (`TypesRegistryClient::list_instances` consumed by `GtsPluginSelector` lazily on the first dispatch call after the `types-registry` is consistent — there is no runtime config-change channel that would re-trigger this query), `toolkit-security` (`SecurityContext` propagation), and `toolkit-canonical-errors` (canonical `Problem` envelope on the REST surface; taken by the host crate `usage-collector` only — the SDK crate `usage-collector-sdk` does NOT depend on it, and the host's `From<UsageCollectorError> for CanonicalError` lift in `usage-collector/src/infra/sdk_error_mapping.rs` produces the canonical Problem envelope from the flat SDK error per DESIGN §3.3 Error Envelopes).

### 1.3 Actors

| Actor                                             | Role in Feature                                                                                                                                                                                   |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-actor-platform-operator`  | Selects and configures the active storage plugin via `[usage_collector].vendor` (read once at `Gear::init`); observes operational endpoints; changes to the binding require a gear restart    |
| `cpt-cf-usage-collector-actor-platform-developer` | Consumes the in-process SDK trait through ClientHub; implements the Plugin SPI when delivering a storage backend                                                                                  |
| `cpt-cf-usage-collector-actor-storage-backend`    | Implements the Plugin SPI surface bound by the Plugin Host; receives dispatched persistence/query/deactivate calls per the `cpt-cf-usage-collector-contract-storage-plugin`                       |
| `cpt-cf-usage-collector-actor-usage-source`       | Arrives at the foundation carrying a gateway-resolved `SecurityContext`; the per-domain-component PDP helper authorizes emission (substrate-only role here; emission semantics owned by §2.3)     |
| `cpt-cf-usage-collector-actor-usage-consumer`     | Arrives at the foundation carrying a gateway-resolved `SecurityContext`; the per-domain-component PDP helper authorizes reads (substrate-only role here; query semantics owned by §2.4)           |
| `cpt-cf-usage-collector-actor-tenant-admin`       | Arrives at the foundation carrying a gateway-resolved `SecurityContext` scoped to their own tenant; tenant isolation is enforced uniformly by every domain component via the `authz_scope` helper |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md) -- Actors §2, Pluggable Storage §5.4, Security & Data Governance §5.8, NFR catalog §6
- **Design**: [DESIGN.md](../DESIGN.md) -- Plugin Host (§3.2), Metric Catalog (§3.2), Contract Surfaces (§3.3), Deployment Topology (§3.8), PRD→DESIGN Realization (§5.3)
- **Decomposition**: [DECOMPOSITION.md](../DECOMPOSITION.md) -- §2.1 Gear Foundation & Pluggable Storage; §4.3 Plugin discovery and dispatch
- **ADR**: [ADR-0001](../ADR/0001-pdp-centric-authorization.md) -- PDP-Centric Authorization; [ADR-0002](../ADR/0002-pluggable-storage.md) -- Pluggable Storage; [ADR-0006](../ADR/0006-contract-stability.md) -- Contract Stability; [ADR-0012](../ADR/0012-unified-plugin-catalog-and-gts-id-reference.md) -- Unified Plugin-DB Metric Catalog & `gts_id` Reference (supersedes ADR-0007 / ADR-0009 / ADR-0010)
- **Plugin SPI reference**: [plugin-spi.md](../plugin-spi.md)
- **SDK trait reference**: [sdk-trait.md](../sdk-trait.md)
- **REST contract**: [usage-collector-v1.yaml](../usage-collector-v1.yaml)
- **Dependencies**: None

## 2. Actor Flows (CDSL)

### Plugin Host Binding (Lazy Resolve)

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:

- At gear bootstrap the host `cpt-cf-usage-collector-component-plugin-host` reads `[usage_collector].vendor` once via `ctx.config_or_default()?`, constructs the `Service` with an embedded `GtsPluginSelector` (no instance resolution happens yet), and registers the consumer-facing `dyn UsageCollectorClientV1` in `ClientHub`. Independently, each `usage-collector-plugin-<backend>` crate's `init()` calls `PluginV1::<UsageCollectorPluginSpecV1>::build_registration(...)`, publishes a `PluginV1<UsageCollectorPluginSpecV1>` instance through `TypesRegistryClient`, then registers its scoped `dyn UsageCollectorPluginV1` client in `ClientHub` under `ClientScope::gts_id(&instance_id)`.
- On the first dispatch call after the `types-registry` is consistent, the host's `GtsPluginSelector::get_or_init` queries `TypesRegistryClient::list_instances` with `UsageCollectorPluginSpecV1::gts_schema_id()`, runs `choose_plugin_instance` against the configured vendor (lowest priority wins), and caches the resolved `GtsInstanceId` as `Arc<str>` for the `Service`'s lifetime. Subsequent dispatches reuse the cached id via `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` per `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`.

**Error Scenarios**:

- `types-registry` unreachable on the first dispatch — the per-call `get_or_init` surfaces the deterministic resolution error to that caller; the selector remains uncached and the next dispatch retries the lazy resolve. The host's `usage_collector.plugin.ready` gauge reports `0` until the structural readiness fact ("selector cached AND `try_get_scoped` returns `Some`") holds.
- No plugin instance is registered under the resolved `ClientScope::gts_id(instance_id)` (for example a plugin gear's `init()` failed before the `register_scoped` step, or the dispatch arrived before that step ran) — `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `None`, which the host lifts to a per-call `plugin-unavailable` error mirroring the credstore Service's `try_get_scoped` → `DomainError::PluginUnavailable` pattern at `gears/credstore/credstore/src/domain/service.rs:53-75`. The `usage_collector.plugin.ready` gauge reflects the same structural fact.
- Binding selection is monotonic for the `Service`'s lifetime: once `get_or_init` has cached an instance id, that selection is reused until the gear restarts. There is no runtime configuration-change channel that would re-trigger resolution (`GtsPluginSelector::reset` is exercised only in framework unit tests).

**Steps**:

1. [ ] - `p1` - At `Gear::init`, read `[usage_collector].vendor` once via `ctx.config_or_default()?` and construct the `Service` with an embedded `GtsPluginSelector`; no `types-registry` query is performed here (mirrors `gears/credstore/credstore/src/gear.rs:44-51`) - `inst-binding-config-read-once`
2. [ ] - `p1` - Each `usage-collector-plugin-<backend>` `init()` builds and publishes its `PluginV1<UsageCollectorPluginSpecV1>` instance through `TypesRegistryClient`, then registers its trait object via `ctx.client_hub().register_scoped::<dyn UsageCollectorPluginV1>(ClientScope::gts_id(&instance_id), api)` — a plain `HashMap::insert` under a `parking_lot::RwLock` - `inst-binding-clienthub-register`
3. [ ] - `p1` - On the first dispatch call after the `types-registry` is consistent, the host enters `Service::get_plugin` and invokes `self.selector.get_or_init(|| self.resolve_plugin())` which queries `TypesRegistryClient::list_instances` with `format!("{plugin_type_id}*", plugin_type_id = UsageCollectorPluginSpecV1::gts_schema_id())`, runs `choose_plugin_instance::<UsageCollectorPluginSpecV1>(&self.vendor, instances.iter().map(|e| (e.id.as_ref(), &e.object)))`, and caches the resolved `GtsInstanceId` exactly once for the `Service`'s lifetime (mirrors `gears/credstore/credstore/src/domain/service.rs:53-75`) - `inst-binding-lazy-resolve`
4. [ ] - `p1` - Look up the scoped trait object via `self.hub.try_get_scoped::<dyn UsageCollectorPluginV1>(&ClientScope::gts_id(instance_id.as_ref()))` and lift `None` to the host's `plugin-unavailable` error on the per-call path (mirrors the credstore Service `try_get_scoped` → `DomainError::PluginUnavailable` pattern at `gears/credstore/credstore/src/domain/service.rs:57-74`) - `inst-binding-try-get-scoped`
5. [ ] - `p1` - Compute the structural readiness fact per `cpt-cf-usage-collector-contract-storage-plugin` ("selector has cached an instance id AND `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `Some` under `ClientScope::gts_id(&instance_id)`") and reflect it in the `usage_collector.plugin.ready` gauge (`1` when both facts hold, `0` otherwise); the SPI exposes no plugin-side `ready()` probe - `inst-binding-readiness-fact`
6. [ ] - `p1` - **RETURN** the resolved scoped `Arc<dyn UsageCollectorPluginV1>` to the calling pipeline so the dispatch can complete; warm-path calls reuse the cached id and the cached scoped Arc with no further `types-registry` round-trip - `inst-binding-return-handle`

### PDP Authorize and Constraint Return

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Actor**: `cpt-cf-usage-collector-actor-platform-developer`

**Success Scenarios**:

- With the inbound `cpt-cf-usage-collector-entity-security-context` (resolved upstream by the ToolKit gateway on REST or supplied by the caller on the in-process SDK) and the operation's attribution tuple, the domain component handling the call (ingestion-gateway, query-gateway, deactivation-handler, or metric-catalog) invokes the shared `authz_scope` helper which calls `cpt-cf-usage-collector-contract-authz-resolver` via `PolicyEnforcer::access_scope_with(ctx, ...)`, receives a permit `cpt-cf-usage-collector-entity-pdp-decision` plus zero-or-more `cpt-cf-usage-collector-entity-pdp-constraint` filters, and surfaces both to its own dispatch path without caching.
- For read paths the returned constraints scope the request so the subsequent dispatch cannot widen the authorized view — user filters can only narrow within the PDP-permitted scope.

**Error Scenarios**:

- `authz-resolver` is unreachable or times out — fail closed with a deterministic platform-authorization error, increment `usage_collector.pdp.failures`, and never serve a cached or permissive decision per `cpt-cf-usage-collector-principle-pdp-centric-authorization`.
- The resolver returns deny — the operation is rejected immediately with an actionable error envelope and no plugin dispatch is performed.
- The resolver returns permit with an empty constraint set on a tenant-scoped read — fail closed per `cpt-cf-usage-collector-fr-tenant-isolation`, because the foundation never derives implicit tenant trust.

**Steps**:

1. [ ] - `p1` - Receive (`cpt-cf-usage-collector-entity-security-context`, operation, attribution tuple) from the surface boundary (REST handler `Extension<SecurityContext>` or in-process SDK caller) — the `SecurityContext` is already resolved upstream of the collector - `inst-pdp-input`
2. [ ] - `p1` - Compose the attribution tuple required by `cpt-cf-usage-collector-contract-authz-resolver` (tenant, resource, optional subject, source gear, optional Metric `gts_id`) - `inst-pdp-compose-tuple`
3. [ ] - `p1` - **TRY** call `cpt-cf-usage-collector-contract-authz-resolver` with the composed tuple - `inst-pdp-resolver-call`
4. [ ] - `p1` - **CATCH** unreachable-or-timeout - `inst-pdp-resolver-catch`
   1. [ ] - `p1` - Increment `usage_collector.pdp.failures` with the deterministic cause label - `inst-pdp-failure-counter`
   2. [ ] - `p1` - **RETURN** platform-authorization error envelope (no cached decision, no permissive fallback) - `inst-pdp-fail-closed`
5. [ ] - `p1` - **IF** the returned `cpt-cf-usage-collector-entity-pdp-decision` is deny **RETURN** the actionable platform-authorization error envelope - `inst-pdp-deny`
6. [ ] - `p1` - **IF** the returned decision is permit but the `cpt-cf-usage-collector-entity-pdp-constraint` set is empty on a tenant-scoped read **RETURN** the same fail-closed envelope per `cpt-cf-usage-collector-fr-tenant-isolation` - `inst-pdp-empty-constraints`
7. [ ] - `p1` - **ELSE** propagate the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint`) pair to the bound domain component for constraint-aware dispatch - `inst-pdp-propagate`
8. [ ] - `p1` - **RETURN** the permit decision plus constraint set to the calling pipeline without caching the result - `inst-pdp-return`

## 3. Processes / Business Logic (CDSL)

### Plugin Host Binding (Lazy Resolution)

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Input**: the `[usage_collector].vendor` string already cached on the `Service` (read once at `Gear::init` via `ctx.config_or_default()?`), the `GtsPluginSelector`'s current cache state (`Some(Arc<str>)` after the first successful resolve or `None` before it), and the `TypesRegistryClient` + `ClientHub` handles obtained from the gear context per `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`. There is no runtime configuration-change input — binding changes require a gear restart.

**Output**: an `Arc<dyn UsageCollectorPluginV1>` for the lazily resolved `cpt-cf-usage-collector-entity-plugin-binding` (cached `GtsInstanceId` + scoped trait object reachable via `ClientHub::try_get_scoped` under `ClientScope::gts_id(&instance_id)`), or a per-call deterministic binding error (`PluginNotFound` when `choose_plugin_instance` finds no match, `PluginUnavailable` when the scoped slot is empty, `TypesRegistryUnavailable` when the registry call fails); the `usage_collector.plugin.ready` gauge reflects the resulting structural readiness fact.

**Steps**:

1. [ ] - `p1` - On the first dispatch call after the `types-registry` is consistent (and on every subsequent dispatch), enter `Service::get_plugin` and invoke `self.selector.get_or_init(|| self.resolve_plugin()).await` — fast path returns the cached `Arc<str>` instance id; slow path takes the resolve lock, re-checks, and runs `resolve_plugin()` exactly once (mirrors `libs/toolkit/src/plugins/mod.rs:56-90` and `gears/credstore/credstore/src/domain/service.rs:53-75`) - `inst-algo-binding-get-or-init`
2. [ ] - `p1` - Inside `resolve_plugin`, query `TypesRegistryClient::list_instances` with the pattern `format!("{plugin_type_id}*", plugin_type_id = UsageCollectorPluginSpecV1::gts_schema_id())` and run `choose_plugin_instance::<UsageCollectorPluginSpecV1>(&self.vendor, instances.iter().map(|e| (e.id.as_ref(), &e.object)))` to pick the lowest-priority match - `inst-algo-binding-resolve-plugin`
3. [ ] - `p1` - **CATCH** registry-or-selector failure - `inst-algo-binding-catch`
   1. [ ] - `p1` - **IF** `TypesRegistryClient::list_instances` is unavailable **RETURN** `TypesRegistryUnavailable` on the per-call path; the selector cache remains empty so the next dispatch retries `get_or_init` - `inst-algo-binding-registry-unavailable`
   2. [ ] - `p1` - **IF** `choose_plugin_instance` matches no instance **RETURN** `PluginNotFound`; the selector cache remains empty and the next dispatch retries (no prior binding exists to retain: `register_scoped` is a plain `HashMap::insert`, with no parallel cache) - `inst-algo-binding-plugin-not-found`
4. [ ] - `p1` - Derive the scope via `ClientScope::gts_id(instance_id.as_ref())` and call `self.hub.try_get_scoped::<dyn UsageCollectorPluginV1>(&scope)`; if it returns `None`, lift to `PluginUnavailable { gts_id, reason: "client not registered yet".into() }` on the per-call path (mirrors `gears/credstore/credstore/src/domain/service.rs:57-74`) - `inst-algo-binding-try-get-scoped`
5. [ ] - `p1` - Compute the structural readiness fact per `cpt-cf-usage-collector-contract-storage-plugin` ("selector has cached an instance id AND `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `Some` under `ClientScope::gts_id(&instance_id)`") and set `usage_collector.plugin.ready` to `1` when both facts hold or `0` otherwise; the SPI exposes no plugin-side `ready()` probe - `inst-algo-binding-readiness-fact`
6. [ ] - `p1` - **RETURN** the resolved scoped `Arc<dyn UsageCollectorPluginV1>` to the calling pipeline so the dispatch completes; warm-path subsequent calls hit the selector fast path and the `ClientHub` `RwLock::read` and reuse both caches with no further `types-registry` round-trip - `inst-algo-binding-return`

### Database binding

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-foundation-database-binding`

Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, the **metric catalog is owned by the storage plugin** (managed via the Plugin SPI, persisted in the active storage plugin's database) alongside `usage_records`, and the plugin's backend database enforces an `ON DELETE RESTRICT` foreign key from `usage_records.gts_id` to `metric_catalog(gts_id)`. The gateway hosts no `metric_catalog` table and no gateway-owned `MetricCatalogRepo`; the foundation database binding instead provisions any auxiliary gateway-resident state (audit-correlation scratch, PDP failure counters, operational-bookkeeping tables) that does NOT belong to the catalog. The gateway keeps a Level-1 read-through cache of the catalog rows for the typed metadata-validation hot path, but the System of Record is the plugin, reached through `cpt-cf-usage-collector-contract-storage-plugin`. The in-plugin reference scheme (column type, index choice) is a plugin-author choice per DESIGN §3.2 / §3.7 and out of FEATURE scope.

**Input**: the gear context handed to `Gear::init` (carrying the platform DB binding handle and the declared `DatabaseCapability::migrations()` set for any gateway-resident auxiliary tables), and the embedded SeaORM migration set shipped inside the gear crate.

**Output**: a registered platform DB binding for gateway-resident auxiliary state, or a deterministic startup error that aborts gear readiness when any step fails. The catalog surface itself is bound separately through the Plugin Host binding algorithm above; it is NOT served from a gateway-local repo.

**Steps**:

1. [ ] - `p1` - At `Gear::init`, resolve the platform DB binding via `ctx.db_required()?` for any gateway-resident auxiliary state; surface the platform error verbatim on failure so the startup pipeline can distinguish a missing/misconfigured platform DB binding from a downstream construction or migration failure - `inst-algo-db-binding-resolve`
2. [ ] - `p1` - Apply the embedded SeaORM migrations declared by `DatabaseCapability::migrations()` (gateway-resident auxiliary tables only — the durable `metric_catalog` table lives in the plugin's backend database per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) exactly once during `Gear::init`; migration application is idempotent across restarts, so a re-run on an already-migrated database completes as a no-op without side effects - `inst-algo-db-binding-apply-migrations`
3. [ ] - `p1` - Register the resolved gateway-resident DB handle with the gear's domain layer so non-catalog domain services can resolve it; catalog services are bound through the Plugin Host (`cpt-cf-usage-collector-flow-foundation-plugin-host-binding`), not through this DB binding, per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-algo-db-binding-register-repo`
4. [ ] - `p1` - **IF** any of steps 1-3 fail **RETURN** a startup error envelope and abort gear readiness; do not proceed to register service routes or expose any gear surface, and record the failure cause on the corresponding `usage_collector.*.failures` counter per `cpt-cf-usage-collector-principle-fail-closed` - `inst-algo-db-binding-fail-closed`

**Config-side note**: the gear declares no `[gears.usage_collector.database.connection_string]` field. The gateway participates in the platform DB binding via `ctx.db_required()?` only for any auxiliary gateway-resident state; the durable `metric_catalog` table and the `usage_records` table both live in the storage plugin's backend database, configured by the plugin gear, per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. Changing the platform DB binding or the plugin binding requires a gear restart.

### PDP Authorize

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Input**: `&cpt-cf-usage-collector-entity-security-context` (resolved upstream by the ToolKit gateway on REST or supplied by the in-process SDK caller), the operation descriptor, and the operation's attribution tuple (tenant, resource, optional subject, source gear, optional Metric `gts_id`).

**Output**: A (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair surfaced to the bound domain component, or a deterministic platform-authorization error envelope with `usage_collector.pdp.failures` incremented.

**Steps**:

1. [ ] - `p1` - Compose the attribution tuple from the caller-supplied operation context and the resolved `cpt-cf-usage-collector-entity-security-context` - `inst-algo-pdp-compose`
2. [ ] - `p1` - **TRY** call `cpt-cf-usage-collector-contract-authz-resolver` with the attribution tuple - `inst-algo-pdp-call`
3. [ ] - `p1` - **CATCH** unreachable-or-timeout - `inst-algo-pdp-catch`
   1. [ ] - `p1` - Increment `usage_collector.pdp.failures` with the deterministic cause label (`unreachable` or `timeout`) - `inst-algo-pdp-counter`
   2. [ ] - `p1` - **RETURN** platform-authorization error envelope; never serve a cached or permissive decision per `cpt-cf-usage-collector-principle-pdp-centric-authorization` - `inst-algo-pdp-fail-closed`
4. [ ] - `p1` - **IF** the returned `cpt-cf-usage-collector-entity-pdp-decision` is deny **RETURN** the actionable platform-authorization error envelope - `inst-algo-pdp-deny`
5. [ ] - `p1` - **IF** the returned decision is permit but the `cpt-cf-usage-collector-entity-pdp-constraint` set is empty on a tenant-scoped read **RETURN** the same fail-closed envelope per `cpt-cf-usage-collector-fr-tenant-isolation` - `inst-algo-pdp-empty`
6. [ ] - `p1` - **ELSE** **RETURN** the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair to the bound domain component without caching - `inst-algo-pdp-return`

### Audit-Correlation Propagation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Input**: inbound request headers (W3C `traceparent` and optional `tracestate` from `cpt-cf-usage-collector-interface-rest-api`, or the in-process trace context attached by callers of `cpt-cf-usage-collector-interface-sdk-client`), the inbound `cpt-cf-usage-collector-entity-security-context` and the PDP decision context emitted by the per-domain-component helper, and the operation descriptor.

**Output**: A propagated W3C Trace Context plus `request-id` correlation pair that is attached to every downstream call (PDP via the per-component helper, Plugin SPI) and recorded on every structured operational event emitted by the gear.

**Steps**:

1. [ ] - `p1` - Read the W3C `traceparent` and optional `tracestate` from the inbound request frame at the surface boundary - `inst-algo-audit-read-headers`
2. [ ] - `p1` - **IF** the request is in-process and has no active trace context **RETURN** the deterministic context-missing error so the caller starts a span before invocation - `inst-algo-audit-missing-context`
3. [ ] - `p1` - Open a server span scoped to the operation, preserving the upstream `trace-id` and starting a new child `span-id` - `inst-algo-audit-open-span`
4. [ ] - `p1` - Capture the `request-id` correlation pair satisfying `cpt-cf-usage-collector-nfr-operational-visibility` - `inst-algo-audit-request-id`
5. [ ] - `p1` - **FOR EACH** downstream invocation (PDP through `cpt-cf-usage-collector-contract-authz-resolver` via the per-domain-component `authz_scope` helper, Plugin SPI through `cpt-cf-usage-collector-contract-storage-plugin`) - `inst-algo-audit-foreach`
   1. [ ] - `p1` - Carry the active `traceparent` (and optional `tracestate`) via the ambient `tracing::Span` / OpenTelemetry context that scopes the downstream invocation; the Plugin SPI declares no explicit `TraceContext` parameter — PDP / Plugin SPI calls inherit the active span - `inst-algo-audit-attach`
   2. [ ] - `p1` - Record the active `trace-id` and `request-id` on every structured operational event emitted during the call - `inst-algo-audit-emit-event`
6. [ ] - `p1` - Reflect the resulting `traceparent` on the outbound response (REST) or on the SDK return value (in-process) so end-to-end traces span gateway → core → plugin → backend - `inst-algo-audit-reflect`
7. [ ] - `p1` - **RETURN** the propagated correlation context to the calling pipeline so subsequent foundation algorithms (lazy binding resolution, PDP authorize, tenant isolation) can re-use it - `inst-algo-audit-return`

### Tenant Isolation Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Input**: `cpt-cf-usage-collector-entity-security-context`, operation descriptor (read or write), caller-supplied attribution (tenant, resource, optional subject, source gear), and any user-supplied query filters.

**Output**: A tenant-scoped operation context whose effective filter set is the intersection of the PDP-returned `cpt-cf-usage-collector-entity-pdp-constraint` set and the user-supplied filters, or a deterministic platform-authorization rejection per `cpt-cf-usage-collector-fr-tenant-isolation`.

**Steps**:

1. [ ] - `p1` - Read the caller-supplied tenant attribution from the operation descriptor without inferring it from the `cpt-cf-usage-collector-entity-security-context` - `inst-algo-tenant-read-attribution`
2. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-foundation-pdp-authorize` to obtain the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair for the (caller, tenant, operation) triple - `inst-algo-tenant-invoke-pdp`
3. [ ] - `p1` - **IF** PDP returned deny or fail-closed **RETURN** the propagated platform-authorization error envelope - `inst-algo-tenant-pdp-deny`
4. [ ] - `p1` - **IF** the operation descriptor identifies a tenant-scoped read and the `cpt-cf-usage-collector-entity-pdp-constraint` set is empty **RETURN** the deterministic platform-authorization error envelope (no implicit tenant trust per `cpt-cf-usage-collector-principle-fail-closed`) - `inst-algo-tenant-empty-constraint`
5. [ ] - `p1` - **FOR EACH** user-supplied filter on a read path - `inst-algo-tenant-foreach`
   1. [ ] - `p1` - **IF** the filter would widen scope beyond the PDP-permitted `cpt-cf-usage-collector-entity-pdp-constraint` set **RETURN** the deterministic platform-authorization error envelope - `inst-algo-tenant-widen-reject`
   2. [ ] - `p1` - **ELSE** intersect the filter with the PDP-permitted constraint set so the effective scope can only narrow - `inst-algo-tenant-intersect`
6. [ ] - `p1` - **RETURN** the tenant-scoped operation context (composed filter set plus the original `cpt-cf-usage-collector-entity-security-context`) to the bound domain component for `cpt-cf-usage-collector-contract-storage-plugin` dispatch - `inst-algo-tenant-return`

## 4. Definitions of Done

### FR: Pluggable Storage

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-pluggable-storage`

The system **MUST** materialize the active storage backend exclusively through `cpt-cf-usage-collector-contract-storage-plugin`, resolved through the `PluginV1<UsageCollectorPluginSpecV1>` GTS base + `types-registry` + `ClientHub` scoped registration pattern per `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub` (SDK declares `UsageCollectorPluginSpecV1` in `usage-collector-sdk/src/gts.rs`; plugins publish through `TypesRegistryClient` and register a scoped `dyn UsageCollectorPluginV1` in `ClientHub` under `ClientScope::gts_id(&instance_id)`; the host's `GtsPluginSelector` lazily resolves the instance on the first dispatch call via `get_or_init` and caches the `GtsInstanceId` for the `Service`'s lifetime; subsequent dispatches reuse the cache via `ClientHub::try_get_scoped`). `[usage_collector].vendor` is read once at `Gear::init`; changing the binding requires a gear restart. There is no in-core fallback path and no parallel cache. Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, the bound plugin owns the durable metric catalog (managed via the Plugin SPI, persisted in the active storage plugin's database) alongside `usage_records`; the catalog row carries `gts_id` (PK; MUST begin with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`) and `metadata_fields` (closed `array<string>` of declared metadata key names; all values are typed as `String` end-to-end). `MetricKind` (`counter` / `gauge`) is **derived** from the `gts_id` prefix; it is not a separate column or trait. The plugin's backend database enforces an `ON DELETE RESTRICT` foreign key from `usage_records.gts_id` to `metric_catalog(gts_id)` so the SoR-level referential invariant holds without any cross-replica coordination. The in-plugin reference scheme (column type, index choice) is plugin-author choice per DESIGN §3.2 / §3.7.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-fr-pluggable-storage`, `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`

**Touches**:

- Telemetry: `usage_collector.plugin.ready` gauge (set to `0` when no `GtsInstanceId` is cached OR no scoped client exists under `ClientScope::gts_id(instance_id)`; the SPI exposes no plugin-side `ready()` probe)
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### FR: AuthN Delegation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-authn-delegation`

The system **MUST** accept only `cpt-cf-usage-collector-entity-security-context` values resolved by the ToolKit gateway (REST surface) or supplied by the in-process caller (SDK surface) and **MUST** reject any operation arriving without a `SecurityContext`; the collector never synthesizes identity, never holds credentials, and never resolves authentication itself.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-fr-authn-delegation`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### FR: Audit Trail

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-audit-trail`

The system **MUST** propagate the W3C Trace Context plus `request-id` correlation pair across every PDP (via the per-domain-component `authz_scope` helper) and Plugin SPI dispatch and record both identifiers on every structured operational event emitted by the gear. Correlation_id originates from the inbound `cpt-cf-usage-collector-entity-security-context` populated by the ToolKit gateway upstream.

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Constraints**: `cpt-cf-usage-collector-fr-audit-trail`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### FR: Tenant Isolation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-tenant-isolation`

The system **MUST** enforce tenant isolation by intersecting every read-path user filter with the PDP-returned `cpt-cf-usage-collector-entity-pdp-constraint` set and rejecting any operation when the constraint set is empty or would be widened, with no implicit per-tenant trust.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-fr-tenant-isolation`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`, `SecurityContext`

### FR: Data Classification

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-data-classification`

The system **MUST** carry the platform-resolved data-classification attributes on the inbound `cpt-cf-usage-collector-entity-security-context` (populated by the ToolKit gateway upstream) and propagate them through every domain component into the bound storage plugin so that no data-classification decision is taken locally.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-fr-data-classification`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### FR: Standards Compliance

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-standards-compliance`

The system **MUST** publish the three contract surfaces (`cpt-cf-usage-collector-interface-plugin`, `cpt-cf-usage-collector-interface-sdk-client`, `cpt-cf-usage-collector-interface-rest-api`), accept gateway-resolved `cpt-cf-usage-collector-entity-security-context` values at the boundary, and operate them through `cpt-cf-usage-collector-contract-authz-resolver` (per-domain-component `authz_scope` helper) so that platform-standard AuthZ and trace-context propagation are realized once at the substrate layer; AuthN is owned by the ToolKit gateway upstream of the collector.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Constraints**: `cpt-cf-usage-collector-fr-standards-compliance`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### FR: Non-Repudiation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-non-repudiation`

The system **MUST** persist the inbound (gateway-resolved) `cpt-cf-usage-collector-entity-security-context` attribution plus the propagated trace identifiers on every operational event so that downstream audit reconciliation can attribute every read, write, and Metric-lifecycle action to a verifiable caller identity. The collector never synthesizes identity; attribution is anchored on the `SecurityContext` accepted at the boundary.

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-audit-correlation`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-fr-non-repudiation`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### FR: Privacy Controls

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-privacy-controls`

The system **MUST** delegate every privacy-relevant access decision to `cpt-cf-usage-collector-contract-authz-resolver`, surface the returned `cpt-cf-usage-collector-entity-pdp-constraint` set to the bound domain component, and refuse to cache or invent any privacy decision locally.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-fr-privacy-controls`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`

### FR: Data Ownership

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-fr-data-ownership`

The system **MUST** enforce the PRD §5.8 ownership and stewardship model across the entire gear: usage records are owned by the tenant administrator (`cpt-cf-usage-collector-actor-tenant-admin`) for their tenant, the Metric catalog and storage-plugin selection are stewarded by the platform operator (`cpt-cf-usage-collector-actor-platform-operator`), and the gear acts as data custodian without asserting ownership of tenant usage data. The gear **MUST NOT** authorize cross-tenant access without an explicit PDP decision (cross-reference `cpt-cf-usage-collector-fr-tenant-isolation`), **MUST** share usage data with downstream consumers only through the public read surfaces (`cpt-cf-usage-collector-interface-rest-api`, `cpt-cf-usage-collector-interface-sdk-client`) within the PDP-authorized scope, and **MUST** require third-party systems to access data as authenticated `cpt-cf-usage-collector-actor-usage-consumer` callers authorized by the platform PDP with no out-of-band export path. This is a cross-cutting governance FR; downstream features (Usage Query, Usage Emission, Event Deactivation) realize specific facets through the foundation-owned PDP authorization helper and tenant-isolation enforcement.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-fr-data-ownership`, `cpt-cf-usage-collector-fr-tenant-isolation`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`, `cpt-cf-usage-collector-interface-sdk-client`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`, `PdpDecision`, `PdpConstraint`

### NFR: Availability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-availability`

The system **MUST** keep the foundation's stateless runtime instances reachable through the platform API gateway so that PDP and Plugin SPI dispatch remain available whenever the bound plugin's structural readiness fact (selector cached AND `ClientHub::try_get_scoped` returns `Some`) holds and is surfaced by the `usage_collector.plugin.ready` gauge. AuthN availability is owned by the ToolKit gateway upstream and is not part of the collector's readiness surface; gear-local liveness and readiness HTTP probes are likewise owned by the ToolKit host above the gear boundary.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-availability`

**Touches**:

- Telemetry: `usage_collector.plugin.ready` gauge
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Scalability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-scalability`

The system **MUST** scale foundation runtime instances horizontally without sharing in-process state across instances; every instance MUST resolve its own `cpt-cf-usage-collector-entity-plugin-binding` through the GTS registry and reach durable state exclusively through the ClientHub-bound plugin.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-scalability`

**Touches**:

- Telemetry: `usage_collector.plugin.ready` gauge
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Plugin Contract Stability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-plugin-contract-stability`

The system **MUST** treat `cpt-cf-usage-collector-contract-storage-plugin` as a stable surface: any breaking change MUST be carried on a versioned suffix so vendors can ship and migrate backends independently of the core release train.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-plugin-contract-stability`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Authentication

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-authentication`

The system **MUST** reject any operation arriving without a `cpt-cf-usage-collector-entity-security-context` resolved by the ToolKit gateway (REST surface) or supplied by the in-process caller (SDK surface); the collector never synthesizes identity, never holds credentials, and the gateway-enforced authentication boundary is the sole AuthN edge for the gear.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-nfr-authentication`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### NFR: Authorization

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-authorization`

The system **MUST** require a permit `cpt-cf-usage-collector-entity-pdp-decision` from `cpt-cf-usage-collector-contract-authz-resolver` and a non-empty `cpt-cf-usage-collector-entity-pdp-constraint` set for tenant-scoped reads before dispatching to the bound domain component, with `usage_collector.pdp.failures` incremented on every failure.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-nfr-authorization`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`

### NFR: Capacity Headroom

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-capacity-headroom`

The system **MUST** keep the foundation runtime stateless behind the platform API gateway so that capacity headroom is realized by horizontally scaling additional instances against the same `cpt-cf-usage-collector-entity-plugin-binding` resolution path.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-capacity-headroom`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Deployment Operations

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-deployment-operations`

The system **MUST** emit the foundation-owned structural-readiness telemetry (`usage_collector.plugin.ready` and `usage_collector.pdp.ready` gauges) via OTLP push so the platform deployment pipeline can drive rollout, drain, and rollback decisions without inspecting gear internals. Platform liveness and readiness HTTP probes are handled by the ToolKit host above the gear boundary and are not gear-owned endpoints.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-deployment-operations`

**Touches**:

- Telemetry: `usage_collector.plugin.ready`, `usage_collector.pdp.ready` gauges
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Developer & Operator Experience

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-developer-operator-experience`

The system **MUST** publish the SDK trait surface (`cpt-cf-usage-collector-interface-sdk-client`) and the REST API surface (`cpt-cf-usage-collector-interface-rest-api`) with stable error envelopes and stable correlation propagation so platform developers and operators can integrate without consulting gear internals.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Constraints**: `cpt-cf-usage-collector-nfr-developer-operator-experience`

**Touches**:

- API: `cpt-cf-usage-collector-interface-sdk-client`, `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### NFR: Documentation Coverage

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-documentation-coverage`

The system **MUST** keep every published surface — `cpt-cf-usage-collector-interface-plugin`, `cpt-cf-usage-collector-interface-sdk-client`, `cpt-cf-usage-collector-interface-rest-api` — referenced by the foundation feature with sibling specifications (`plugin-spi.md`, `sdk-trait.md`, `usage-collector-v1.yaml`) so the contract surfaces are documented before any consumer integrates.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-nfr-documentation-coverage`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`, `cpt-cf-usage-collector-interface-sdk-client`, `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### NFR: Error Experience

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-error-experience`

The system **MUST** return deterministic, actionable error envelopes for missing/invalid `SecurityContext`, PDP failure, and plugin binding failure, with the failure cause recorded in the corresponding `usage_collector.*.failures` counter and the trace identifier preserved across the boundary. AuthN failures themselves are surfaced by the ToolKit gateway upstream and never reach the collector boundary.

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-error-experience`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`, `PdpDecision`, `PluginBinding`

### NFR: Graceful Degradation

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-graceful-degradation`

The system **MUST** retain the previously-bound `cpt-cf-usage-collector-entity-plugin-binding` whenever a downstream registry or PDP resolver becomes unreachable at runtime, surfacing the cause on the foundation-owned readiness gauges rather than synthesizing a permissive decision. AuthN unavailability is owned by the ToolKit gateway upstream and is not a runtime degradation mode of the collector.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-nfr-graceful-degradation`

**Touches**:

- Telemetry: `usage_collector.plugin.ready`, `usage_collector.pdp.ready` gauges
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`, `SecurityContext`, `PdpDecision`

### NFR: Operational Visibility

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-operational-visibility`

The usage-collector gear **MUST** construct all foundation-owned instruments on `opentelemetry::global::meter_with_scope(MODULE_NAME)` at bootstrap, so that they appear in the OTLP stream emitted by ToolKit's `SdkMeterProvider`. The gear **MUST** propagate `trace-id` and `request-id` headers per W3C TraceContext (already enabled by ToolKit's `init_tracing`), so every emitted log, metric exemplar, and span shares the same correlation identifiers. The gear **MUST NOT** expose any in-gear HTTP metrics endpoint; metrics reach the collector exclusively through the OTLP push path established by ToolKit's `SdkMeterProvider`. Platform liveness and readiness HTTP probes are owned by the ToolKit host above the gear boundary; the collector contributes only the structural-readiness gauges (`usage_collector.plugin.ready`, `usage_collector.pdp.ready`).

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-audit-correlation`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-nfr-operational-visibility`

**Touches**:

- Telemetry: `usage_collector.plugin.ready`, `usage_collector.pdp.ready` gauges
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`, `SecurityContext`

### NFR: Support Readiness

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-nfr-support-readiness`

The system **MUST** preserve the trace identifier across every PDP (via the per-domain-component `authz_scope` helper) and Plugin SPI boundary so the platform gateway access log, PDP decision logs, and gear-level operational events can be reconciled by support per the audit-trail propagation algorithm.

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Constraints**: `cpt-cf-usage-collector-nfr-support-readiness`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### Principle: Fail Closed

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-principle-fail-closed`

The system **MUST** fail closed whenever the inbound `SecurityContext` is missing or invalid, the PDP resolver is unreachable, or the storage plugin binding is unreachable or returns an unexpected outcome — never synthesize identity, never serve a cached decision, never invent a binding.

**Implements**:

- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-principle-fail-closed`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`, `PdpDecision`, `PluginBinding`

### Principle: Pluggable Storage

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-principle-pluggable-storage`

The system **MUST** keep the storage backend pluggable behind `cpt-cf-usage-collector-contract-storage-plugin` and reach durable state exclusively through the ClientHub-bound plugin handle.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-principle-pluggable-storage`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Principle: Contract Stability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-principle-contract-stability`

The system **MUST** evolve every published contract surface — `cpt-cf-usage-collector-contract-storage-plugin`, `cpt-cf-usage-collector-contract-authz-resolver`, `cpt-cf-usage-collector-contract-gts-registry` — through versioned, additive changes so existing consumers and backend implementors continue to bind without code change.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-principle-contract-stability`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`, `cpt-cf-usage-collector-interface-sdk-client`, `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`, `SecurityContext`

### Principle: PDP-Centric Authorization

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-principle-pdp-centric-authorization`

The system **MUST** dispatch every read and write operation through `cpt-cf-usage-collector-contract-authz-resolver` for a permit/deny `cpt-cf-usage-collector-entity-pdp-decision` plus the `cpt-cf-usage-collector-entity-pdp-constraint` set, never serving a cached decision and never deriving authorization locally.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-principle-pdp-centric-authorization`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`, `SecurityContext`

### Constraint: Plugin Contract Stability

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-constraint-plugin-contract-stability`

The system **MUST** treat `cpt-cf-usage-collector-contract-storage-plugin` as the only durable-state interface and refuse to introduce parallel storage paths that bypass the binding.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-constraint-plugin-contract-stability`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Constraint: Vendor Pluggable

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-constraint-vendor-pluggable`

The system **MUST** keep concrete vendor backends out of the foundation feature so any compliant `cpt-cf-usage-collector-contract-storage-plugin` implementation can be bound through the GTS instance selector without core changes.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-constraint-vendor-pluggable`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Constraint: Resource Platform-Owned

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-constraint-resource-platform-owned`

The system **MUST** treat compute, storage, and identity resources as platform-owned by reaching them only through `cpt-cf-usage-collector-contract-gts-registry`, `cpt-cf-usage-collector-contract-authz-resolver`, and `cpt-cf-usage-collector-contract-storage-plugin`; caller identity arrives in the inbound `cpt-cf-usage-collector-entity-security-context` resolved by the ToolKit gateway upstream.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-constraint-resource-platform-owned`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`, `SecurityContext`

### Constraint: NFR Thresholds

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-constraint-nfr-thresholds`

The system **MUST** preserve the foundation's stateless, horizontally-scaled topology so that downstream availability, scalability, and capacity-headroom NFR thresholds remain valid as feature surfaces are added.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-constraint-nfr-thresholds`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### ADR: Contract Stability

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-adr-contract-stability`

The system **MUST** carry every breaking change to a published surface on a versioned suffix per the contract-stability ADR, so existing implementors continue to bind through `cpt-cf-usage-collector-contract-storage-plugin` and `cpt-cf-usage-collector-interface-sdk-client` without recompilation.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-adr-contract-stability`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`, `cpt-cf-usage-collector-interface-sdk-client`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### ADR: PDP-Centric Authorization

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-adr-pdp-centric-authorization`

The system **MUST** route every authorization decision through `cpt-cf-usage-collector-contract-authz-resolver` per the PDP-centric authorization ADR; no local policy table, no cached decision, no derived bypass.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-adr-pdp-centric-authorization`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`

### ADR: Pluggable Storage

- [ ] `p3` - **ID**: `cpt-cf-usage-collector-dod-foundation-adr-pluggable-storage`

The system **MUST** retain pluggable storage as the only durable-state path per the pluggable-storage ADR (`cpt-cf-usage-collector-adr-pluggable-storage`), binding the active backend exclusively through `cpt-cf-usage-collector-contract-storage-plugin` resolved against the GTS instance selector. Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` the metric catalog is the sole catalog and lives on the pluggable-storage substrate alongside `usage_records`; no gateway-local `metric_catalog` table is provisioned.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-adr-pluggable-storage`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Contract: Storage Plugin

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-contract-storage-plugin`

The system **MUST** publish `cpt-cf-usage-collector-contract-storage-plugin` as the sole durable-state contract and register the bound plugin in ClientHub with GTS instance scope so the host's structural readiness fact (selector cached AND `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `Some`) drives the `usage_collector.plugin.ready` gauge; the contract exposes no plugin-side `ready()` probe. Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, the contract carries the catalog write/read/list/delete/reference-check surface alongside the usage-records surface; the catalog write surface carries the GTS-typed metric `gts_id` plus the closed `metadata_fields: array<string>` payload (declared metadata key names; `MetricKind` is derived from the `gts_id` prefix and is NOT a separate payload field), and the catalog row shape on the SoR side is `gts_id` (PK), `metadata_fields` (array of strings), and `created_at`. The contract guarantees that `usage_records.gts_id` is constrained by an in-database `ON DELETE RESTRICT` foreign key to `metric_catalog(gts_id)`, so unsafe catalog deletes surface as a structured `MetricReferenced` SPI error to the gateway. The in-plugin reference scheme (column type, index choice) is plugin-author choice per DESIGN §3.2 / §3.7.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-contract-storage-plugin`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Contract: AuthZ Resolver

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-contract-authz-resolver`

The system **MUST** dispatch every operation through `cpt-cf-usage-collector-contract-authz-resolver`, propagate the audit-correlation context on the call, and emit a deterministic platform-authorization error envelope on resolver failure, deny, or empty-constraint outcomes for tenant-scoped reads.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-contract-authz-resolver`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`, `PdpConstraint`

### Contract: GTS Registry

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-contract-gts-registry`

The system **MUST** resolve the active storage plugin identity through `cpt-cf-usage-collector-contract-gts-registry` from the `[usage_collector].vendor` value cached at `Gear::init`, lazily on the first dispatch call after the `types-registry` is consistent (single-flight `GtsPluginSelector::get_or_init`), and cache the resolved `GtsInstanceId` for the `Service`'s lifetime; subsequent binding changes require a gear restart.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-contract-gts-registry`

**Touches**:

- API: `cpt-cf-usage-collector-interface-plugin`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Entity: PluginBinding

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-entity-plugin-binding`

The system **MUST** materialize `cpt-cf-usage-collector-entity-plugin-binding` exclusively through the Plugin Host (the host gear's own Service) using the GTS-resolved plugin identity. Binding state is the two structural facts recomputed per call by the `cpt-cf-usage-collector-flow-foundation-plugin-host-binding` flow (selector-cached `GtsInstanceId` AND `ClientHub::try_get_scoped` returns `Some`); the prior finite-state-machine model (`Unbound`/`Resolving`/`Bound`/`Refreshing`/`Failed`) was removed because it is not present in the reference gears.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-entity-plugin-binding`

**Touches**:

- Telemetry: `usage_collector.plugin.ready` gauge
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### Entity: SecurityContext

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-entity-security-context`

The system **MUST** carry the inbound `cpt-cf-usage-collector-entity-security-context` (resolved by the ToolKit gateway upstream of the collector on the REST surface, or supplied by the in-process caller on the SDK surface) through every PDP, plugin, and operational-event boundary without local mutation.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-audit-correlation`

**Constraints**: `cpt-cf-usage-collector-entity-security-context`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `SecurityContext`

### Entity: PdpDecision

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-entity-pdp-decision`

The system **MUST** surface the `cpt-cf-usage-collector-entity-pdp-decision` returned by `cpt-cf-usage-collector-contract-authz-resolver` to the bound domain component without caching and reject every deny outcome through the deterministic platform-authorization error envelope.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-entity-pdp-decision`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpDecision`

### Entity: PdpConstraint

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-foundation-entity-pdp-constraint`

The system **MUST** surface the `cpt-cf-usage-collector-entity-pdp-constraint` set returned by `cpt-cf-usage-collector-contract-authz-resolver` to the bound domain component, intersect every user-supplied read filter with that set, and reject any operation that would widen the constraint scope.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-foundation-tenant-isolation`

**Constraints**: `cpt-cf-usage-collector-entity-pdp-constraint`

**Touches**:

- API: `cpt-cf-usage-collector-interface-rest-api`
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PdpConstraint`

### Component: Plugin Host

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-foundation-component-plugin-host`

The system **MUST** realize `cpt-cf-usage-collector-component-plugin-host` as the sole owner of lazy binding resolution (`GtsPluginSelector::get_or_init` on the first dispatch after the `types-registry` is consistent, cached for the `Service`'s lifetime) and the `usage_collector.plugin.ready` structural readiness gauge. Scoped `dyn UsageCollectorPluginV1` registration in `ClientHub` is owned by each `usage-collector-plugin-<backend>` crate's own `init()`, not by the host.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-plugin-host-binding`
- `cpt-cf-usage-collector-algo-foundation-plugin-host-binding`

**Constraints**: `cpt-cf-usage-collector-component-plugin-host`

**Touches**:

- Telemetry: `usage_collector.plugin.ready` gauge
- DB: `cpt-cf-usage-collector-db-gear-store`
- Entities: `PluginBinding`

### §2.1-item → DoD-ID Coverage Matrix

Coverage of every DECOMPOSITION §2.1 catalog item:

| §2.1 source ID                                                | §2.1 kind         | DoD ID                                                                       |
| ------------------------------------------------------------- | ----------------- | ---------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-fr-pluggable-storage`                 | FR                | `cpt-cf-usage-collector-dod-foundation-fr-pluggable-storage`                 |
| `cpt-cf-usage-collector-fr-authn-delegation`                  | FR                | `cpt-cf-usage-collector-dod-foundation-fr-authn-delegation`                  |
| `cpt-cf-usage-collector-fr-audit-trail`                       | FR                | `cpt-cf-usage-collector-dod-foundation-fr-audit-trail`                       |
| `cpt-cf-usage-collector-fr-tenant-isolation`                  | FR                | `cpt-cf-usage-collector-dod-foundation-fr-tenant-isolation`                  |
| `cpt-cf-usage-collector-fr-data-classification`               | FR                | `cpt-cf-usage-collector-dod-foundation-fr-data-classification`               |
| `cpt-cf-usage-collector-fr-standards-compliance`              | FR                | `cpt-cf-usage-collector-dod-foundation-fr-standards-compliance`              |
| `cpt-cf-usage-collector-fr-non-repudiation`                   | FR                | `cpt-cf-usage-collector-dod-foundation-fr-non-repudiation`                   |
| `cpt-cf-usage-collector-fr-privacy-controls`                  | FR                | `cpt-cf-usage-collector-dod-foundation-fr-privacy-controls`                  |
| `cpt-cf-usage-collector-fr-data-ownership`                    | FR                | `cpt-cf-usage-collector-dod-foundation-fr-data-ownership`                    |
| `cpt-cf-usage-collector-nfr-availability`                     | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-availability`                     |
| `cpt-cf-usage-collector-nfr-scalability`                      | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-scalability`                      |
| `cpt-cf-usage-collector-nfr-plugin-contract-stability`        | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-plugin-contract-stability`        |
| `cpt-cf-usage-collector-nfr-authentication`                   | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-authentication`                   |
| `cpt-cf-usage-collector-nfr-authorization`                    | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-authorization`                    |
| `cpt-cf-usage-collector-nfr-capacity-headroom`                | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-capacity-headroom`                |
| `cpt-cf-usage-collector-nfr-deployment-operations`            | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-deployment-operations`            |
| `cpt-cf-usage-collector-nfr-developer-operator-experience`    | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-developer-operator-experience`    |
| `cpt-cf-usage-collector-nfr-documentation-coverage`           | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-documentation-coverage`           |
| `cpt-cf-usage-collector-nfr-error-experience`                 | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-error-experience`                 |
| `cpt-cf-usage-collector-nfr-graceful-degradation`             | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-graceful-degradation`             |
| `cpt-cf-usage-collector-nfr-operational-visibility`           | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-operational-visibility`           |
| `cpt-cf-usage-collector-nfr-support-readiness`                | NFR               | `cpt-cf-usage-collector-dod-foundation-nfr-support-readiness`                |
| `cpt-cf-usage-collector-principle-fail-closed`                | Principle         | `cpt-cf-usage-collector-dod-foundation-principle-fail-closed`                |
| `cpt-cf-usage-collector-principle-pluggable-storage`          | Principle         | `cpt-cf-usage-collector-dod-foundation-principle-pluggable-storage`          |
| `cpt-cf-usage-collector-principle-contract-stability`         | Principle         | `cpt-cf-usage-collector-dod-foundation-principle-contract-stability`         |
| `cpt-cf-usage-collector-principle-pdp-centric-authorization`  | Principle         | `cpt-cf-usage-collector-dod-foundation-principle-pdp-centric-authorization`  |
| `cpt-cf-usage-collector-constraint-plugin-contract-stability` | Design constraint | `cpt-cf-usage-collector-dod-foundation-constraint-plugin-contract-stability` |
| `cpt-cf-usage-collector-constraint-vendor-pluggable`          | Design constraint | `cpt-cf-usage-collector-dod-foundation-constraint-vendor-pluggable`          |
| `cpt-cf-usage-collector-constraint-resource-platform-owned`   | Design constraint | `cpt-cf-usage-collector-dod-foundation-constraint-resource-platform-owned`   |
| `cpt-cf-usage-collector-constraint-nfr-thresholds`            | Design constraint | `cpt-cf-usage-collector-dod-foundation-constraint-nfr-thresholds`            |
| `cpt-cf-usage-collector-adr-contract-stability`               | ADR-derived       | `cpt-cf-usage-collector-dod-foundation-adr-contract-stability`               |
| `cpt-cf-usage-collector-adr-pdp-centric-authorization`        | ADR-derived       | `cpt-cf-usage-collector-dod-foundation-adr-pdp-centric-authorization`        |
| `cpt-cf-usage-collector-adr-pluggable-storage`                | ADR-derived       | `cpt-cf-usage-collector-dod-foundation-adr-pluggable-storage`                |
| `cpt-cf-usage-collector-contract-storage-plugin`              | Contract          | `cpt-cf-usage-collector-dod-foundation-contract-storage-plugin`              |
| `cpt-cf-usage-collector-contract-authz-resolver`              | Contract          | `cpt-cf-usage-collector-dod-foundation-contract-authz-resolver`              |
| `cpt-cf-usage-collector-contract-gts-registry`                | Contract          | `cpt-cf-usage-collector-dod-foundation-contract-gts-registry`                |
| `cpt-cf-usage-collector-entity-plugin-binding`                | Domain entity     | `cpt-cf-usage-collector-dod-foundation-entity-plugin-binding`                |
| `cpt-cf-usage-collector-entity-security-context`              | Domain entity     | `cpt-cf-usage-collector-dod-foundation-entity-security-context`              |
| `cpt-cf-usage-collector-entity-pdp-decision`                  | Domain entity     | `cpt-cf-usage-collector-dod-foundation-entity-pdp-decision`                  |
| `cpt-cf-usage-collector-entity-pdp-constraint`                | Domain entity     | `cpt-cf-usage-collector-dod-foundation-entity-pdp-constraint`                |
| `cpt-cf-usage-collector-component-plugin-host`                | Design component  | `cpt-cf-usage-collector-dod-foundation-component-plugin-host`                |

Coverage totals: FR=9, NFR=13, Principle=4, Design constraint=4, ADR-derived=3, Contract=3, Domain entity=4, Design component=1 — total 41 DoD entries, zero duplicates, zero §2.1 gaps.

## 5. Acceptance Criteria

- [ ] `p1` - At gear bootstrap with a valid `[usage_collector].vendor` configuration, the foundation constructs the `Service` with an embedded `GtsPluginSelector` (no `types-registry` query is issued at bootstrap); each `usage-collector-plugin-<backend>` `init()` independently registers its scoped `dyn UsageCollectorPluginV1` in `ClientHub` under `ClientScope::gts_id(&instance_id)`. On the first dispatch call after the `types-registry` is consistent, the host lazily resolves the binding via `GtsPluginSelector::get_or_init` and publishes `usage_collector.plugin.ready=1` through the OTLP stream emitted by ToolKit's `SdkMeterProvider` once the structural readiness fact holds; while resolution has not yet succeeded (or the `types-registry` is unreachable on the per-call path), the dispatch returns the deterministic `plugin-unavailable` error envelope and the gauge reads `0`.
- [ ] `p1` - The host's `GtsPluginSelector` performs lazy single-flight resolution on the first dispatch call after the `types-registry` is consistent and caches the resolved `GtsInstanceId` for the `Service`'s lifetime; a per-call dispatch whose scoped slot in `ClientHub` is empty returns the deterministic `plugin-unavailable` error envelope (mirroring `gears/credstore/credstore/src/domain/service.rs:57-74`) without inventing a binding or substituting a prior one. Binding changes require a gear restart.
- [ ] `p1` - Every REST and SDK operation that arrives without a resolved `cpt-cf-usage-collector-entity-security-context` is rejected at the boundary with a deterministic error envelope and no operation is dispatched to the bound plugin; the collector never synthesizes identity and never holds credentials, because AuthN is owned by the ToolKit gateway upstream.
- [ ] `p1` - When `cpt-cf-usage-collector-contract-authz-resolver` is unreachable, returns deny, or returns permit with an empty `cpt-cf-usage-collector-entity-pdp-constraint` set on a tenant-scoped read, every call returns the deterministic platform-authorization error envelope, `usage_collector.pdp.failures` increments, and no cached or permissive decision is ever served.
- [ ] `p1` - User-supplied read filters can only narrow the PDP-permitted `cpt-cf-usage-collector-entity-pdp-constraint` set; any operation that would widen the constraint scope, or that arrives with an empty PDP constraint set on a tenant-scoped read, is rejected with the platform-authorization error envelope.
- [ ] `p1` - Every inbound request that arrives with a W3C `traceparent` causes that `trace-id` plus the captured `request-id` correlation pair to appear on every downstream PDP (per-domain-component `authz_scope` helper) and Plugin SPI call and on every structured operational event emitted by the gear; the outbound REST response and SDK return value reflect the resulting `traceparent`.
- [ ] `p1` - Platform liveness and readiness probes are handled by the ToolKit host above the gear boundary; the collector exposes no gear-local health endpoints. The foundation-owned instruments `usage_collector.plugin.ready` and `usage_collector.pdp.failures` are visible in the OTLP stream emitted by ToolKit's `SdkMeterProvider`, and `usage_collector.plugin.ready` flips to `0` whenever the bound plugin's structural readiness fact stops holding (selector cache missing OR `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `None`; the SPI exposes no plugin-side `ready()` probe).
- [ ] `p1` - The Plugin SPI (`cpt-cf-usage-collector-interface-plugin`), SDK trait (`cpt-cf-usage-collector-interface-sdk-client`), and REST API (`cpt-cf-usage-collector-interface-rest-api`) are published with the sibling specifications (`plugin-spi.md`, `sdk-trait.md`, `usage-collector-v1.yaml`), accept gateway-resolved `SecurityContext` values at the boundary, operate through `cpt-cf-usage-collector-contract-authz-resolver`, and expose no data path that bypasses `cpt-cf-usage-collector-contract-storage-plugin`.
- [ ] `p2` - Any breaking change to a published contract surface is carried on a versioned suffix per `cpt-cf-usage-collector-adr-contract-stability`, so existing in-process SDK consumers and storage backend implementors continue to bind without recompilation across foundation revisions.
- [ ] `p1` - **Given** a storage plugin bound through `cpt-cf-usage-collector-contract-storage-plugin` whose `metric_catalog` table is empty for the candidate `gts_id`, **when** any caller attempts to insert a `usage_records` row carrying that `gts_id`, **then** the plugin's in-database `ON DELETE RESTRICT` foreign key on `usage_records.gts_id` → `metric_catalog(gts_id)` rejects the insert at the storage boundary and the SPI surfaces a structured `MetricNotFound` error to the gateway with the offending `gts_id` cited in the error context (referential-integrity invariant per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`).
- [ ] `p1` - **Given** a storage plugin bound through `cpt-cf-usage-collector-contract-storage-plugin` whose `metric_catalog` table holds a row for `gts_id = G` and whose `usage_records` table holds at least one row whose `gts_id = G`, **when** any caller invokes the catalog delete SPI for `gts_id = G`, **then** the plugin's `ON DELETE RESTRICT` foreign key rejects the delete inside the same transaction and the SPI surfaces a structured `MetricReferenced` error to the gateway that carries the `gts_id` and a sample reference count, no `metric_catalog` row is removed, and no `usage_records` row is mutated — preserving the `cpt-cf-usage-collector-adr-caller-supplied-attribution` invariant by construction (referential-delete semantics per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`).

## 6. Changelog

- **0.2.1 — 2026-06-02** — Cascaded the ADR-0012 2026-06-02 amendment (simplifications 5 and 6) into the FR: Pluggable Storage and Contract: Storage Plugin DoDs: replaced the prior open-shape metadata-schema + GTS-traits-map prose with the closed `metadata_fields: array<string>` shape (declared keys only; all values typed as `String`) and the kind-from-prefix derivation against the two reserved kind base type ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`). The catalog row shape on the SoR side is `gts_id` (PK), `metadata_fields` (array of strings), and `created_at`; `MetricKind` is derived, not stored. Cites ADR 0012 §Amendment, PRD §5.1/§5.2/§5.7, and domain-model §2.5/§2.6/§2.8.
- **0.2.0 — 2026-06-02** — Aligned the FEATURE with the unified plugin-DB metric catalog model per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (supersedes ADRs 0007 / 0009 / 0010) and the updated DESIGN (Phase 3): renamed catalog references to the canonical "Metric Catalog" (managed via the Plugin SPI, persisted in the active storage plugin's database); removed the entire `Declared Metrics Configuration` subsection from §3 and its `Boot Seed Declared Metrics` sequence reference; reframed Database Binding so no gateway-local `metric_catalog` table is provisioned; renamed FK column `usage_records.metric_type_uuid` → `usage_records.gts_id` and target PK `metric_catalog(type_uuid)` → `metric_catalog(gts_id)` in the Storage Plugin contract DoD, the Pluggable Storage DoD, and the §5 referential-integrity / referential-delete acceptance criteria; renamed SPI error variant `MetricTypeNotFound` → `MetricNotFound` on the referential-integrity acceptance criterion; dropped residual mentions of `uuid5` / `parent_type_uuid` / indexable-trait gate / `abstract` from all normative content. Cites DESIGN §3.2 Metric Catalog, §3.7 `metric_catalog` row shape, and ADR 0012.
