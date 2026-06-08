# Decomposition: Usage Collector

**Overall implementation status:**

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-status-overall`

<!-- toc -->

- [1. Overview](#1-overview)
- [2. Entries](#2-entries)
  - [2.1 Gear Foundation & Pluggable Storage ⏳ HIGH](#21-gear-foundation--pluggable-storage--high)
  - [2.2 Metric Catalog & Lifecycle ⏳ HIGH](#22-metric-catalog--lifecycle--high)
  - [2.3 Usage Emission ⏳ HIGH](#23-usage-emission--high)
  - [2.4 Usage Query ⏳ MEDIUM](#24-usage-query--medium)
  - [2.5 Event Deactivation ⏳ MEDIUM](#25-event-deactivation--medium)
  - [2.6 Compensation ⏳ MEDIUM](#26-compensation--medium)
  - [2.7 Deliberate Omissions](#27-deliberate-omissions)
- [3. Feature Dependencies](#3-feature-dependencies)
- [4. Crate Layout & Platform Dependencies](#4-crate-layout--platform-dependencies)
  - [4.1 Two-crate layout](#41-two-crate-layout)
  - [4.2 Direct platform dependencies](#42-direct-platform-dependencies)
  - [4.3 Plugin discovery and dispatch](#43-plugin-discovery-and-dispatch)
- [5. Document Changelog](#5-document-changelog)

<!-- /toc -->

## 1. Overview

The Usage Collector DESIGN is decomposed into six capability features that mirror the gear's distinct user-visible responsibilities rather than its internal layering: Foundation (Plugin SPI surface, plugin host and binding lifecycle, the shared PDP authorization helper (no centralized adapter), deployment topology, and declared tech stack), Metric Catalog & Lifecycle (operator-driven registration, deletion, and lookup of metric definitions), Usage Emission (the contract-first, kind-enforced, idempotent ingestion path that writes to the active storage backend), Usage Query (PDP-constrained aggregated and raw cursor-paginated reads through the Query Gateway), Event Deactivation (the one-way `active → inactive` status flip for previously emitted records — applies to any `entry_type` and, on a `usage` row, cascades depth-1 to active referencing compensations in the same atomic transition), and Compensation (counter value-reversal via the existing unified ingestion path: an append-only signed-negative `compensation` entry referencing a prior `usage` row via `corrects_id`, recorded under PDP attribution and mandatory idempotency, netted into `SUM` aggregations without modifying the original row). Splitting by capability — instead of by REST/SDK/Plugin layer — keeps each feature mutually exclusive, lets the foundation feature own the cross-cutting plugin plumbing and the shared PDP authorization helper once, and lines up the decomposition with the PRD's functional-requirement clusters (Ingestion, Pluggable Storage, Query & Aggregation, Event Deactivation, Compensation). Dependencies flow outward from Foundation: Metric Catalog & Lifecycle, Usage Emission, Usage Query, Event Deactivation, and Compensation all build on the foundation's Plugin SPI and shared PDP authorization helper; Usage Emission, Usage Query, and Compensation additionally depend on Metric Catalog & Lifecycle (kind/existence enforcement on the write path; mandatory single-Metric filter validation on the aggregated read path; counter-only Metric Kind for compensation); Compensation depends on Usage Emission as the writer of the rows its compensations reference (the unified ingestion path is shared); Usage Query depends on Compensation for the SUM-nets aggregation contract; and Event Deactivation depends on Usage Emission (records must exist before they can be deactivated) and is coupled to Compensation via the depth-1 cascade (deactivating a `usage` row flips active referencing compensations in the same atomic transition). This shape preserves the DESIGN's tri-surface architecture and fail-closed metering posture while keeping the read and write planes implementable and reviewable in parallel.

**Decomposition Strategy**:

- Cohesion by capability: each feature groups the DESIGN components, sequences, and data entities that collaborate to deliver one externally-observable capability (e.g., Usage Emission owns the Ingestion Gateway component, the Emit Usage Record sequence, and the `usage_records` table together).
- Loose coupling via explicit `Depends On`: every feature declares its upstream features by ID, with no implicit ordering — Foundation has no dependencies, and downstream features list only the minimum upstream features they need.
- 100% DESIGN/PRD element coverage: every `cpt-cf-usage-collector-*` ID introduced by DESIGN.md and PRD.md is assigned to at least one feature, or recorded as a deliberate omission with justification in [§2.7](#27-deliberate-omissions).
- Mutual exclusivity at the capability layer: each DESIGN component and sequence is assigned to exactly one feature, and each `dbtable` has a single writer-owner (the writing feature) with reader and status-only-update features explicitly noting shared usage; cross-cutting concerns (shared PDP authorization helper, Plugin SPI, deployment topology, contract surfaces) are owned by Foundation and referenced — not duplicated — by dependent features. Domain entities may appear under multiple features' "Domain Model Entities" lists because they cross feature boundaries by value (e.g., `SecurityContext` flows through every gateway, `UsageRecord` is written by ingestion and status-flipped by deactivation); this is reference, not duplicated ownership.
- Emission vs. query plane separation: write-side (Usage Emission) and read-side (Usage Query) capabilities are split into distinct features so the ingestion-throughput and analytical-query-latency NFRs can be sequenced and validated independently.
- Event-driven deactivation isolation: the monotonic `active → inactive` status transition is carried by its own feature so that reactivation, bulk operations, and field edits remain explicitly out of scope.

## 2. Entries

### 2.1 Gear Foundation & Pluggable Storage ⏳ HIGH

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-feature-foundation`

- **Purpose**: Establish the Usage Collector's stateless gear runtime substrate and its three public contract surfaces — the in-process SDK trait, the REST API, and the storage Plugin SPI — so that every later capability can plug into a single, identical execution shape. Every read and write entry point receives an already-resolved caller `SecurityContext` (populated upstream by the ToolKit gateway on REST via `OperationBuilder::authenticated()` or supplied directly to the SDK; the gear NEVER consumes `authn-resolver`) and is fronted by inline PDP authorization through the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver` with no anonymous bypass, no cached decisions, and no synthesized identities, so safety-critical behavior is realized once at the substrate layer rather than re-implemented per feature. The foundation also owns the Plugin SPI's contract-stability guarantee so storage vendors can ship and migrate backends independently of the core release train.

- **Depends On**: None

- **Scope**:
  - Plugin SPI surface declaration (`cpt-cf-usage-collector-interface-plugin`) and the storage-plugin contract it exposes to backend implementors.
  - Plugin host lifecycle: at `Gear::init` the host reads `[usage_collector].vendor` once via `ctx.config_or_default()?` and constructs the `Service` with an embedded `GtsPluginSelector` (no `types-registry` query yet); each `usage-collector-plugin-<backend>` `init()` independently registers its scoped `dyn UsageCollectorPluginV1` in `ClientHub` under `ClientScope::gts_id(&instance_id)`; on the first dispatch after the `types-registry` is consistent the host lazily resolves the bound instance via `GtsPluginSelector::get_or_init` (single-flight, cached for the `Service`'s lifetime) and looks the client up via `ClientHub::try_get_scoped`. There is no separate "Gear Orchestrator" component — binding is decentralised across the host gear's `Service` constructor and each plugin gear's own `init()`. Binding changes require a gear restart; there is no runtime configuration-change channel.
  - PDP authorization wiring per domain component: every ingestion-gateway, query-gateway, deactivation-handler, and metric-catalog call dispatches through `authz-resolver` via the `authz_scope` helper for a permit/deny `PdpDecision` plus any `PdpConstraint` filters, fail-closed on PDP unavailability with no cached decisions.
  - Audit-trail correlation propagation (per `cpt-cf-usage-collector-fr-audit-trail`): every domain component propagates the request-level correlation identifier carried on the inbound `SecurityContext` through `cpt-cf-usage-collector-contract-authz-resolver` on every ingestion, query, deactivation, and Metric-lifecycle operation, so the platform gateway access log and PDP decision logs can be reconciled with gear-level activity per DESIGN §5.3.
  - Tenant isolation enforcement (per `cpt-cf-usage-collector-fr-tenant-isolation`): every domain component realizes tenant isolation across read and write paths via the `authz_scope` helper (per DESIGN §3.5 component description and §5.3 traceability) by issuing PDP decisions and PDP constraints per operation; no implicit per-tenant trust and no cross-tenant access absent an explicit PDP authorization. [§2.3](#23-usage-emission-high) ingestion and [§2.4](#24-usage-query-medium) query consume this enforcement through the per-component PDP helper.
  - REST API contract surface (`cpt-cf-usage-collector-interface-rest-api`) registration behind the platform API gateway and SDK trait surface (`cpt-cf-usage-collector-interface-sdk-client`) registration in ClientHub for in-process consumers. Operational telemetry is pushed via OTLP from ToolKit's global meter provider; no gear-local Prometheus-scrape endpoint and no gear-local health endpoints are exposed (platform liveness and readiness are handled by the ToolKit host).
  - Deployment topology (`cpt-cf-usage-collector-topology-gear-runtime`): stateless, horizontally scaled instances behind the platform API gateway with durable state reached exclusively through the ClientHub-bound plugin.
  - Declared tech stack (`cpt-cf-usage-collector-tech-stack`) across the Presentation, Application, Domain, and Infrastructure layers.

- **Out of scope**:
  - Metric type registration, deletion, and catalog lookup semantics — owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle.
  - Usage record emission, idempotency dedup and conflict rejection (exact-equality retries silently absorbed; canonical-field mismatches rejected as `idempotency_conflict`), kind enforcement, and ingestion-path attribution — owned by [§2.3](#23-usage-emission-high) Usage Emission.
  - Aggregated and raw read-path query execution and PDP-constraint composition — owned by [§2.4](#24-usage-query-medium) Usage Query.
  - Event-driven `active → inactive` deactivation transitions — owned by [§2.5](#25-event-deactivation-medium) Event Deactivation.
  - Concrete backend implementations (ClickHouse, TimescaleDB, etc.), infrastructure-as-code, autoscaling thresholds, and storage-tier HA posture — owned by the active storage plugin and platform operations docs.

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-pluggable-storage`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-authn-delegation`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-audit-trail`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-tenant-isolation`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-classification`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-standards-compliance`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-non-repudiation`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-privacy-controls`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-ownership`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-availability`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-scalability`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-plugin-contract-stability`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-authentication`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-authorization`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-capacity-headroom`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-deployment-operations`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-developer-operator-experience`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-documentation-coverage`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-error-experience`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-graceful-degradation`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-operational-visibility`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-support-readiness`

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-fail-closed`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-pluggable-storage`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-contract-stability`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-pdp-centric-authorization`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-otlp-push-emission`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-gateway-http-server-instrument-reuse`

- **Design Constraints Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-plugin-contract-stability`
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-vendor-pluggable`
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-resource-platform-owned`
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-nfr-thresholds`
  - `p2` - `cpt-cf-usage-collector-adr-contract-stability`
  - `p2` - `cpt-cf-usage-collector-adr-pdp-centric-authorization`
  - `p2` - `cpt-cf-usage-collector-adr-pluggable-storage`

- **Domain Model Entities**:
  - PluginBinding (`cpt-cf-usage-collector-entity-plugin-binding`)
  - SecurityContext (`cpt-cf-usage-collector-entity-security-context`)
  - PdpDecision (`cpt-cf-usage-collector-entity-pdp-decision`)
  - PdpConstraint (`cpt-cf-usage-collector-entity-pdp-constraint`)

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-plugin-host`

- **API**:
  - Plugin SPI surface (`cpt-cf-usage-collector-interface-plugin`) — storage backend contract; reference specification in `plugin-spi.md` (sibling to DESIGN.md); the exact Rust signature lives in `usage-collector-sdk/src/plugin_api.rs`.
  - SDK trait surface (`cpt-cf-usage-collector-interface-sdk-client`) — in-process Rust trait registered in ClientHub; reference specification in `sdk-trait.md` (sibling to DESIGN.md); the exact Rust signature lives in `usage-collector-sdk/src/api.rs`.
  - REST API surface (`cpt-cf-usage-collector-interface-rest-api`) — versioned HTTP surface served behind the platform API gateway; `usage-collector-v1.yaml` is the reference contract (the production OpenAPI document is emitted at runtime by `OpenApiRegistryImpl` and CI drift-checked against the YAML).

- **Data**:
  - [ ] `p3` - `cpt-cf-usage-collector-db-gear-store`

- **Contracts**:
  - [ ] `p1` - `cpt-cf-usage-collector-contract-storage-plugin`
  - [ ] `p1` - `cpt-cf-usage-collector-contract-authz-resolver`
  - [ ] `p1` - `cpt-cf-usage-collector-contract-gts-registry`

### 2.2 Metric Catalog & Lifecycle ⏳ HIGH

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-feature-metric-lifecycle`

- **Purpose**: Provide the operator-driven lifecycle for Metric type definitions — register, list, get, and delete — so the platform-global metric catalog (keyed by `gts_id`) plus the metric's closed declared-metadata-key list exists as a single authoritative surface that the ingestion path can consult for declared-key membership validation and the query path can consult for dimension-aware filter / group-by resolution. Catalog rows are durably owned by the storage plugin alongside `usage_records` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (2026-06-02 amendment); the gateway owns the REST/SDK API surface, PDP authorization, and an in-process Level-1 (L1) catalog cache that fronts the plugin SoR. Per the ADR-0012 2026-06-02 amendment, `kind ∈ {counter, gauge}` is derived once from the `gts_id` prefix (`gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`) — there is no `kind` column on the catalog row and no `kind` trait on the metric specification — and the per-metric closed list of allowed metadata keys travels in the catalog row as `metadata_fields: Vec<String>` (all values typed as String end-to-end). Registration and deletion are gated through the per-component PDP authorization helper (against `cpt-cf-usage-collector-contract-authz-resolver`) so only authorized platform operators can mutate the catalog.

- **Depends On**: `cpt-cf-usage-collector-feature-foundation`

- **Scope**:
  - Register a Metric (type) via the SDK trait method `UsageCollectorClient::register_metric_type` or the REST endpoint `POST /usage-collector/v1/metric-types` with the metric's GTS `gts_id` (which MUST begin with one of the two reserved kind prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`) and `metadata_fields: Vec<String>` (the closed list of declared metadata keys) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02; the gateway PDP-authorizes the call, validates the kind prefix and the well-formedness of `metadata_fields` (non-empty unique key names), then dispatches the catalog write through the Plugin SPI's `register_metric_type` method per ADR 0012. Errors raised by the SPI surface as the canonical taxonomy variants `MetricAlreadyExists { gts_id }` and `InvalidKindPrefix { gts_id }` (kind-prefix rejection at the gateway). The plugin persists the row durably into the plugin-owned `metric_catalog` table alongside `usage_records`.
  - Delete a Metric (type) via the SDK trait method `UsageCollectorClient::delete_metric_type` or the REST endpoint `DELETE /usage-collector/v1/metric-types/{gts_id}`. Deletion dispatches through the Plugin SPI's `delete_metric_type` method; the plugin enforces referential integrity via the in-database `ON DELETE RESTRICT` foreign key `usage_records.gts_id → metric_catalog(gts_id)` and returns the canonical `MetricReferenced { gts_id, sample_ref_count }` error if any usage row still references the target type. A delete targeting a missing row raises `MetricNotFound { gts_id }`. The gateway surfaces these as deterministic REST/SDK errors. On success the gateway invalidates the L1 catalog cache entry for the deleted `gts_id`.
  - List the catalog (`UsageCollectorClient::list_metric_types` / `GET /usage-collector/v1/metric-types`) and get a single catalog entry (`UsageCollectorClient::read_metric_type` / `GET /usage-collector/v1/metric-types/{gts_id}`) for metric type discovery, declared-field retrieval, and dimension resolution by the Ingestion Gateway and the Query Gateway. List and get dispatch through the Plugin SPI's `list_metric_types` / `read_metric_type` methods; the gateway serves cached hits from its L1 catalog cache (see [§2.2.x Gateway L1 catalog cache](#22-metric-catalog--lifecycle--high) work-package below) and falls back to the plugin SoR on miss. A `read_metric_type` for an unknown `gts_id` raises `MetricNotFound { gts_id }`.
  - PDP-gated operator authority: every Metric register and Metric delete call receives an already-resolved caller `SecurityContext` (populated upstream by the ToolKit gateway on REST or supplied directly to the SDK) and authorizes the mutation inline through the per-component PDP authorization helper against `cpt-cf-usage-collector-contract-authz-resolver` before any Plugin SPI call is dispatched.
  - **Catalog ownership work-package (plugin-side)** — per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02: the storage plugin owns the durable `metric_catalog` table colocated with `usage_records` in the plugin's backend database; the FK `usage_records.gts_id → metric_catalog(gts_id) ON DELETE RESTRICT` enforces referential integrity natively at the storage engine, atomically inside the delete transaction, with no cross-replica protocol and no distributed coordination. Canonical row shape (aligned with DESIGN §3.7): `gts_id TEXT PRIMARY KEY` (the human-readable GTS id, MUST begin with one of the two reserved kind prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`), `metadata_fields TEXT[] NOT NULL` (the closed list of declared metadata key names; all values typed as String end-to-end; undeclared keys rejected at ingest), `created_at TIMESTAMPTZ NOT NULL`. There is NO `kind` column — `kind ∈ {counter, gauge}` is derived once from the `gts_id` prefix. No tenant scoping (Metric types are platform-global). The CRUD endpoints are surfaced through the Plugin SPI canonical methods `register_metric_type`, `read_metric_type`, `list_metric_types`, and `delete_metric_type` per ADR 0012 §3; the gateway's SDK and REST surfaces converge on a single domain service that dispatches into the SPI. Referential integrity does NOT require a separate `catalog-reference-check` SPI method: the FK `ON DELETE RESTRICT` is enforced inside the `delete_metric_type` transaction and surfaces as `MetricReferenced { gts_id, sample_ref_count }`.
  - **Gateway L1 catalog cache work-package** — per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02: the gateway maintains an in-process Level-1 read-through cache keyed by `gts_id`, whose content is `Map<gts_id, {kind, metadata_fields: HashSet<String>}>` (matching DESIGN §3.7). `kind` is derived once on cache load from the `gts_id` prefix and is NOT stored on the catalog row. On cache miss, the gateway loads the catalog row from the plugin SoR (catalog-read SPI) and inserts `{kind: derived from prefix, metadata_fields: HashSet from the row's TEXT[]}`. There is no JSON-Schema compilation, no merge core, and no per-metric validator object. On register / delete the gateway invalidates the affected `gts_id` entry only on the flat `gts_id` keyspace — no cascade-invalidation through an ancestor chain because there is no chain; the slow-path fallback for any consistency window is a direct catalog-read against the plugin. Ingest-time validation rejects undeclared metadata keys with a structured error envelope (Problem `context.reason="unknown_metadata_key"` per the locked `usage-collector-v1.yaml`); the cache is on the ingest hot path and a cache bug is an ingest-time data-quality regression, not a perf-only concern.
  - **Dimension-aware query path work-package** — per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02 (consumed by [§2.4](#24-usage-query--medium) Usage Query): `RawQuery.metric_gts_id` is **REQUIRED** (no longer optional) so the declared-key set can be resolved per request; `AggregationQuery.group_by` is **fixed-fields plus per-metric declared metadata fields** (fixed fields: `tenant`, `resource`, `subject`, `source_gear`, `timestamp`, `status`); `$filter` accepts the queried metric type's declared metadata keys on top of fixed fields. Declared keys are resolved per request from the queried metric's `metadata_fields` list in the catalog row (cached as `HashSet<String>` in the L1 catalog cache); there are no undeclared "extras" — undeclared keys are rejected at ingest. Cross-metric aggregation is out of scope (single-metric aggregation is required for declared-key resolution).

- **Out of scope**:
  - Source-gear-to-Metric emit authorization — owned by the PDP as operator-managed policy, not stored inside the Usage Collector.
  - Per-Metric business-rule validation, accounting / billing semantics, or pricing — owned by source gears and downstream consumers, never by the catalog. (Note: the catalog DOES carry typed declared-dimension validation per ADR 0012, which is a metadata shape constraint, not business logic.)
  - Usage record emission, idempotency dedup and conflict rejection (exact-equality retries silently absorbed; canonical-field mismatches rejected as `idempotency_conflict`), counter / gauge value enforcement on the ingestion path, and ingest-time metadata shape validation against the L1-cached compiled schema — owned by [§2.3](#23-usage-emission--high) Usage Emission (which consumes the L1 cache produced by this feature).
  - Aggregated and raw read-path query execution and dimension-aware filter / group-by composition — owned by [§2.4](#24-usage-query--medium) Usage Query (which resolves dimensions per request via the catalog projection produced by this feature).
  - Event-driven `active → inactive` record deactivation — owned by [§2.5](#25-event-deactivation--medium) Event Deactivation.
  - Durable Metric persistence, replication posture, and physical storage of the `metric_catalog` table — owned by the storage plugin behind the Plugin SPI per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; the gateway never holds catalog state outside the read-through L1 cache.
  - Cross-service Metric type discovery (e.g., registration of `usage-collector` metric types in a global `types-registry`) — explicitly OPTIONAL / DEFERRED per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; not in this rework.

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-metric-registration`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-metric-deletion`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-metric-existence-and-kind`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-counter-semantics`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-gauge-semantics`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-authorization`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-availability`

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-kind-enforcement`

- **Design Constraints Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-no-business-logic`
  - `p2` - `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (single plugin-DB catalog managed via SDK/REST; in-database `ON DELETE RESTRICT` FK; usage records reference metrics via `gts_id` directly; per Amendment 2026-06-02, the catalog row is flat — `gts_id` + `metadata_fields TEXT[]` + `created_at`, no `kind` column and no JSON-Schema surface; gateway L1 catalog cache fronts the plugin SoR with `Map<gts_id, {kind, metadata_fields: HashSet<String>}>`)

- **Domain Model Entities**:
  - Metric (`cpt-cf-usage-collector-entity-metric`) — a GTS Type Schema per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; durably owned by the plugin per ADR 0012.
  - MetricKind (`cpt-cf-usage-collector-entity-metric-kind`) — derived once from the `gts_id` prefix (`gts.cf.core.usage.counter.v1~` ⇒ `counter`, `gts.cf.core.usage.gauge.v1~` ⇒ `gauge`) per ADR-0012 Amendment 2026-06-02; not stored as a catalog column and not declared as a metric-spec trait.

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-metric-catalog`

- **API**:
  - POST /usage-collector/v1/metric-types (request body carries the GTS Type Schema with declared dimensions and `kind` trait per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; PDP-authorized; dispatches through the Plugin SPI catalog-write method per ADR 0012)
  - DELETE /usage-collector/v1/metric-types/{gts_id} (PDP-authorized; dispatches through the Plugin SPI catalog-delete method; the plugin's in-database `ON DELETE RESTRICT` FK rejects the delete atomically if any usage row references the target metric — surfaced as a structured "metric referenced" error per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`)
  - GET /usage-collector/v1/metric-types (served from L1 cache; falls back to Plugin SPI catalog-list on miss)
  - GET /usage-collector/v1/metric-types/{gts_id} (served from L1 cache; falls back to Plugin SPI catalog-read on miss)

- **Sequences**:
  - `p1` - `cpt-cf-usage-collector-seq-register-metric`
  - `p1` - `cpt-cf-usage-collector-seq-delete-metric`

- **Data**:
  - `p1` - `cpt-cf-usage-collector-dbtable-metric-catalog` (plugin-owned, colocated with `usage_records` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; FK `usage_records.gts_id → metric_catalog(gts_id) ON DELETE RESTRICT`; row schema carries `metadata_fields TEXT[]` per ADR 0012 Amendment 2026-06-02)

### 2.3 Usage Emission ⏳ HIGH

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-feature-usage-emission`

- **Purpose**: Provide the single, contract-first write path for at-least-once ingestion of usage records from authenticated source gears. Every emit — single or batched, REST or SDK — flows through the Ingestion Gateway, which receives an already-resolved caller `SecurityContext` (populated upstream by the ToolKit gateway on REST via `OperationBuilder::authenticated()` or supplied directly to the SDK; the gear NEVER consumes `authn-resolver`), the PDP authorizes the full attribution tuple (tenant, resource, optional subject, source gear, Metric `gts_id`) fail-closed inline through the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, the Metric Catalog L1 catalog cache confirms Metric type existence and supplies `{kind, metadata_fields: HashSet<String>}` (per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02 — `kind` derived once on cache load from the `gts_id` prefix), kind-dependent invariants are enforced (counter records reject negative deltas, gauges accept point-in-time values as-is), closed-key membership validation is applied to caller-supplied `metadata` (every caller-supplied metadata key MUST be a member of the metric's declared `metadata_fields` set; undeclared keys raise `unknown_metadata_key`; all values are typed as String end-to-end), the configurable `RecordMetadata` size cap is enforced, and the validated record is dispatched through the Plugin SPI for durable persistence under the dedup composite `(tenant_id, gts_id, idempotency_key)` — with the `gts_id` FK column enforcing referential integrity against the plugin-owned `metric_catalog` per ADR 0012. On a key collision the plugin compares the caller-supplied canonical fields (value, timestamp, resource_ref, subject_ref, source_gear, and metadata; the match key and the server-owned `id`/`status` are excluded): an exact-equality retry — every compared field equal, including metadata — is silently absorbed (no error, no double-count), whereas a same-key submission whose canonical fields differ in ANY field (including a metadata-only difference) is a deterministic `idempotency_conflict` rejection and is NEVER silently dropped. Caller-supplied idempotency keys make at-least-once delivery safe end-to-end for genuine retries, uniformly across counter and gauge kinds, so retries never inflate counter totals or poison gauge point-in-time signals. This is the only write path into `usage_records` — aggregation, query, deactivation, and audit ledger semantics are owned elsewhere.

- **Depends On**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-metric-lifecycle`

- **Scope**:
  - Ingestion Gateway endpoint — a single REST entry point (`POST /usage-collector/v1/records`) accepts 1..N records per call (batched submissions capped at 100 records per `cpt-cf-usage-collector-nfr-batch-and-report-timing`), serving both the REST API surface and the in-process SDK trait through the same gateway.
  - Per-call authentication is owned by the ToolKit gateway upstream of the collector (the gear NEVER consumes `authn-resolver`); per-call PDP authorization runs inline through the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, fail-closed on PDP unavailability with no synthesized identity and no cached PDP decision.
  - Metric type existence, derived `kind` (from the `gts_id` prefix), and declared `metadata_fields: HashSet<String>` lookup against the gateway L1 catalog cache produced by [§2.2](#22-metric-catalog--lifecycle--high) on every accepted record before plugin dispatch; the slow-path fallback on miss is a direct Plugin SPI catalog-read per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02.
  - **Closed-key metadata validation at ingest** per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02: every key in the caller-supplied `metadata` map MUST be a member of the metric's declared `metadata_fields` set served by the L1 catalog cache; declared keys are queryable by [§2.4](#24-usage-query--medium) Usage Query; there is NO free-form extras surface and NO `additionalProperties: true` escape hatch — undeclared keys are validation errors. All values are typed as String end-to-end. Undeclared keys are rejected at the gateway before plugin dispatch with a structured Problem envelope (`context.reason="unknown_metadata_key"`).
  - Four-cell `(MetricKind × EntryType)` value matrix enforcement at the gateway, before any plugin call — `counter + usage` requires `value >= 0` (rejects negative deltas); `counter + compensation` requires `value < 0` (strictly negative; signed-negative reversal recorded against a `corrects_id` pointer); `gauge + usage` accepts any signed value as a point-in-time replacement; `gauge + compensation` is REJECTED before persistence (gauges have no `SUM` semantics, so the only correction for a gauge is deactivation per `cpt-cf-usage-collector-adr-monotonic-deactivation`). The compensation cells and the L1 `corrects_id` referential checks are introduced by [§2.6](#26-compensation--medium) Compensation per `cpt-cf-usage-collector-adr-usage-compensation`; the compensation flow is inlined inside `features/usage-emission.md` per the locked `feature_doc_shape = inline-in-emission`.
  - Mandatory caller-supplied idempotency-key dedup via the storage-plugin composite `(tenant_id, gts_id, idempotency_key)`; exact-equality retries (all caller-supplied canonical fields equal — value, timestamp, resource_ref, subject_ref, source_gear, and metadata) are silently absorbed without error and without double-counting, while a same-key submission with ANY differing canonical field (including a metadata-only difference) is a deterministic `idempotency_conflict` Conflict that is rejected deterministically and is NEVER silently dropped.
  - Configurable `RecordMetadata` size-cap enforcement (default 8 KiB per record) with actionable rejection on oversize.
  - Mandatory caller-supplied tenant attribution (carried via `SecurityContext`), mandatory resource attribution (`ResourceRef`), and optional subject attribution (`SubjectRef`); source-gear identity is recorded for PDP source-gear-to-Metric authorization.
  - Persistence through the Plugin Host into `usage_records` as the sole writer of that table; the persisted row carries `gts_id` (the GTS metric id string), used as the FK column to the plugin-owned `metric_catalog` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; per-record acceptance acknowledgements are surfaced deterministically to the caller.

- **Out of scope**:
  - Aggregated or raw read-path query execution and PDP-constraint composition — owned by [§2.4](#24-usage-query-medium) Usage Query.
  - Event-driven `active → inactive` deactivation transitions — owned by [§2.5](#25-event-deactivation-medium) Event Deactivation.
  - Metric registration, deletion, and catalog mutation — owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle; the gateway only reads the catalog.
  - Plugin host lifecycle, shared PDP authorization helper definition, REST/SDK/Plugin SPI surface declaration, and deployment topology — owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Gear Foundation & Pluggable Storage.
  - Business logic / billing / pricing / quota enforcement — explicitly out of the metering substrate per `cpt-cf-usage-collector-constraint-no-business-logic`.
  - Gear-local audit-ledger emission for accepted records — authoritative audit is delegated to the platform gateway access log and PDP decision logs per `cpt-cf-usage-collector-fr-audit-trail`; a dedicated in-gear audit-emission capability is deferred per DESIGN §3.9.5 and [§4](#4-crate-layout-platform-dependencies).
  - Concrete plugin implementations, partitioning, retention, and physical layout of `usage_records` — owned by the active storage plugin; however, retention remains constrained by a strict key-preservation obligation: the plugin MUST preserve the `(tenant_id, gts_id, idempotency_key)` dedup key tuple permanently — retention may reclaim, archive, or purge record bodies, but MUST NOT free a dedup key (the unbounded idempotency window never lets a key be reused).

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-ingestion`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-idempotency`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-record-metadata`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-counter-semantics`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-gauge-semantics`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-tenant-attribution`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-resource-attribution`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-subject-attribution`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-ingestion-authorization`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-quality`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-throughput`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-throughput-profile`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-ingestion-latency`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-workload-isolation`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-batch-and-report-timing`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-availability-boundary`

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-idempotency-by-key`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-kind-enforcement`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-fail-closed`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-pluggable-storage`

- **Design Constraints Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-no-business-logic`
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-nfr-thresholds`
  - `p2` - `cpt-cf-usage-collector-adr-caller-supplied-attribution`
  - `p2` - `cpt-cf-usage-collector-adr-mandatory-idempotency`
  - `p2` - `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (per Amendment 2026-06-02: ingest-time closed-key membership validation against the L1-cached `metadata_fields: HashSet<String>` — undeclared keys raise `unknown_metadata_key`; all values typed as String end-to-end; `gts_id` FK column on `usage_records` references the plugin-owned `metric_catalog` via `ON DELETE RESTRICT`; the metric existence guarantee is enforced at the storage engine)

- **Domain Model Entities**:
  - UsageRecord (`cpt-cf-usage-collector-entity-usage-record`)
  - RecordMetadata (`cpt-cf-usage-collector-entity-record-metadata`)
  - TenantRef (caller-supplied tenant attribution carried via `SecurityContext`; tenant scope materialized on persisted records as the `tenant_id` column of `cpt-cf-usage-collector-dbtable-usage-records`)
  - ResourceRef (`cpt-cf-usage-collector-entity-resource-ref`)
  - SubjectRef (`cpt-cf-usage-collector-entity-subject-ref`)
  - IdempotencyKey (`cpt-cf-usage-collector-entity-idempotency-key`)
  - Metric (`cpt-cf-usage-collector-entity-metric`)
  - MetricKind (`cpt-cf-usage-collector-entity-metric-kind`)
  - SecurityContext (`cpt-cf-usage-collector-entity-security-context`)

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-ingestion-gateway`

- **API**:
  - POST /usage-collector/v1/records (accepts single and batched usage records per `cpt-cf-usage-collector-fr-ingestion`; batched submissions capped at 100 records per call per `cpt-cf-usage-collector-nfr-batch-and-report-timing`)

- **Sequences**:
  - `p1` - `cpt-cf-usage-collector-seq-emit-usage`

- **Data**:
  - `p2` - `cpt-cf-usage-collector-dbtable-usage-records`

### 2.4 Usage Query ⏳ MEDIUM

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-feature-usage-query`

- **Purpose**: Provide the single read path for the metering substrate — aggregated and raw — through one PDP-authorized Query Gateway that anchors every read on the resolved `SecurityContext`, composes user-supplied filters with PDP-returned constraints so the authorized scope can only narrow, and pushes server-side SUM / COUNT / MIN / MAX / AVG with grouping (aggregated path) and cursor-paginated record retrieval (raw path) into the active storage plugin. Time range and a single Metric are mandatory filters (both the aggregated and raw paths require `metric_gts_id` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, because the per-metric declared-dimension set must be resolved before `$filter` / `$apply` can be admitted); the Query Gateway validates filter structure against fixed fields plus the queried metric's declared dimensions, refuses to widen scope under any user-supplied filter, and rejects unregistered Metric references before plugin dispatch. PDP denial, empty constraints, or PDP unavailability fail closed with no cached decisions and no synthesized identity, while an empty match within the authorized scope returns an empty result set / page rather than an error.

- **Depends On**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-metric-lifecycle`, `cpt-cf-usage-collector-feature-usage-emission`, `cpt-cf-usage-collector-feature-usage-compensation`

- **Scope**:
  - Aggregated query path (`POST /usage-collector/v1/records/aggregate`) — mandatory time range and single-Metric filter, optional tenant / subject / resource / source-gear filters, server-side SUM / COUNT / MIN / MAX / AVG with grouping pushed into the bound storage plugin via the Plugin SPI. `AggregationQuery.group_by` is **fixed fields plus per-metric declared metadata keys** per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` Amendment 2026-06-02; fixed fields are `tenant`, `resource`, `subject`, `source_gear`, `timestamp`, `status`; dynamic fields are the queried metric type's declared keys from `metadata_fields` resolved via the L1 catalog cache produced by [§2.2](#22-metric-catalog--lifecycle--high). Cross-metric aggregation is out of scope — single-metric is required so the declared-key set is unambiguous.
  - Raw query path (`GET /usage-collector/v1/records`) — `RawQuery.metric_gts_id` is **REQUIRED** per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (was optional pre-rework); OData query parameters `$filter` (mandatory time range plus optional narrowing on fixed fields and the queried metric type's declared metadata keys), `$orderby` (must be a prefix of the canonical keyset `(timestamp, id)`), `$top` (page size, server-clamped to `[1, 1000]`), and `cursor` (toolkit cursor encoded by the gateway over the standardized sort keyset). The declared-key set the `$filter` AST is type-checked against is resolved per request from the queried metric's `metadata_fields` list served by the L1 catalog cache; there are no undeclared "extras" — undeclared keys are rejected at ingest, so every row only carries declared keys. Cursor decode and validate-against-current-`$filter`/`$orderby` happen at the gateway via `toolkit_odata::validate_cursor_against`; the plugin is dispatched with a structured `(filter_ast, order, page_after, limit)` tuple and returns `(rows, last_keyset)` from which the gateway re-issues the next cursor. The response envelope is `toolkit_odata::Page<UsageRecord>`.
  - PDP constraint application on every query through the per-component PDP authorization helper against `cpt-cf-usage-collector-contract-authz-resolver`, composing the returned `PdpConstraint`s with user-supplied filters so the result set can only narrow within the authorized scope.
  - Tenant isolation per `cpt-cf-usage-collector-fr-tenant-isolation`: every read is anchored on the resolved `SecurityContext` and PDP-returned constraints, with no cross-tenant read possible absent an explicit PDP decision permitting it.
  - Single-Metric filter validation and per-metric declared-dimension resolution against the gateway L1 cache on the aggregated and raw paths — unregistered Metric references are rejected with an actionable error envelope before plugin dispatch (per `cpt-cf-usage-collector-fr-metric-existence-and-kind`, requiring a direct dependency on [§2.2](#22-metric-catalog--lifecycle--high) Metric Catalog & Lifecycle); `$filter` clauses naming a property not in the declared-dimension set are rejected with Problem `context.reason="filter_field_not_declared"` per the locked `usage-collector-v1.yaml`.
  - Active-and-inactive record visibility: the Query Gateway returns both `active` and `inactive` rows from `usage_records` within the PDP-authorized scope (per `cpt-cf-usage-collector-fr-data-lifecycle`), preserving auditable history after `cpt-cf-usage-collector-seq-deactivate-event` flips the `status` column; distinguishing the two values is the caller's responsibility.
  - Fail-closed posture on AuthN, PDP, or plugin unavailability — no synthesized identity, no cached decision, no inferred result; an empty match within the authorized scope returns an empty result set / page (not an error).

- **Out of scope**:
  - Client-side aggregation, widening of the authorized scope under any user-supplied filter, and any business-rule or pricing filtering — out by `cpt-cf-usage-collector-constraint-no-business-logic` and owned by downstream consumers.
  - Cross-tenant reads without an explicit PDP decision permitting them — owned by PDP policy, not by the Query Gateway.
  - Write paths (single emit, batch emit, idempotency dedup and conflict rejection — exact-equality retries silently absorbed, canonical-field mismatches rejected as `idempotency_conflict`, counter / gauge kind enforcement, `RecordMetadata` size-cap enforcement) — owned by [§2.3](#23-usage-emission-high) Usage Emission.
  - Metric registration, deletion, and catalog mutation — owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle; the Query Gateway only reads the catalog to validate the mandatory single-Metric filter.
  - Event-driven `active → inactive` deactivation transitions — owned by [§2.5](#25-event-deactivation-medium) Event Deactivation.
  - Plugin host lifecycle, shared PDP authorization helper definition, REST / SDK / Plugin SPI surface declaration, and deployment topology — owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Gear Foundation & Pluggable Storage.
  - Concrete plugin query execution (native acceleration structures, partitioning, sort orders, retention) — owned by the active storage plugin behind the Plugin SPI.

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-query-aggregation`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-query-raw`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-tenant-isolation`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-ownership` (cross-cutting governance FR; primary owner is [§2.1](#21-gear-foundation--pluggable-storage--high) Foundation — listed here because Usage Query realizes the read-path data-sharing constraint)
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-query-latency`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-batch-and-report-timing`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-workload-isolation`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-authorization`

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-pdp-centric-authorization`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-fail-closed`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-aggregate-asymmetry`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-canonical-errors`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-canonical-page`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-cursor-gateway-ownership`

- **Design Constraints Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-no-business-logic`
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-nfr-thresholds`
  - `p2` - `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (per Amendment 2026-06-02 dimension-aware query path: `RawQuery.metric_gts_id` REQUIRED; `AggregationQuery.group_by` = fixed fields + per-metric declared metadata keys; `$filter` accepts declared keys over fixed fields; declared keys resolved per request from the queried metric's `metadata_fields` list; metric existence check on the aggregated and raw paths consumes the gateway L1 catalog cache that fronts the plugin-owned `metric_catalog`)

- **Domain Model Entities**:
  - AggregationQuery (`cpt-cf-usage-collector-entity-aggregation-query`) — `group_by` is fixed fields + per-metric declared dimensions per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
  - AggregationResult (`cpt-cf-usage-collector-entity-aggregation-result`)
  - RawQuery (`cpt-cf-usage-collector-entity-raw-query`) — `metric_gts_id` is REQUIRED per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
  - UsageRecordFilterField (`cpt-cf-usage-collector-entity-usage-record-filter-field`) — fixed-field set bounded by the Rust enum; dynamic per-metric declared dimensions admitted on top per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; implementing the `toolkit_odata::filter::FilterField` contract carries the static portion of the OData `$filter` AST accepted by the raw query path.
  - Keyset (`cpt-cf-usage-collector-entity-keyset`) — canonical `(timestamp, id)` sort tuple consumed by the toolkit cursor envelope for raw-read pagination.
  - PdpConstraint (`cpt-cf-usage-collector-entity-pdp-constraint`)
  - SecurityContext (`cpt-cf-usage-collector-entity-security-context`)
  - ResourceRef (`cpt-cf-usage-collector-entity-resource-ref`)

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-query-gateway`

- **API**:
  - POST /usage-collector/v1/records/aggregate
  - GET /usage-collector/v1/records

- **Sequences**:
  - `p1` - `cpt-cf-usage-collector-seq-query-aggregated`
  - `p2` - `cpt-cf-usage-collector-seq-query-raw`

- **Data**:
  - `p2` - `cpt-cf-usage-collector-dbtable-usage-records`

- **Contracts**:
  - [ ] `p1` - `cpt-cf-usage-collector-contract-downstream-usage-reader`

### 2.5 Event Deactivation ⏳ MEDIUM

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-feature-event-deactivation`

- **Purpose**: Provide the PDP-authorized cross-kind error-retraction path that flips a single previously emitted record's `status` column from `active` to `inactive` without mutating any other property, realizing immutability-via-deactivation rather than in-place edits or hard deletion. Deactivation applies uniformly to any `entry_type` (both `usage` and `compensation` rows), and when the target row has `entry_type = usage`, it triggers a **depth-1 cascade** that, within the same atomic storage-layer transition, flips every currently-active `entry_type = compensation` row whose `corrects_id = target_id` from `active` to `inactive` (see the cross-link to [§2.6](#26-compensation--medium) Compensation below). The Deactivation Handler receives the operator's already-resolved `SecurityContext` (populated upstream by the ToolKit gateway on REST via `OperationBuilder::authenticated()` or supplied directly to the SDK), runs the request through the per-component PDP authorization helper against `cpt-cf-usage-collector-contract-authz-resolver` fail-closed, and issues a status-only atomic transition through the Plugin SPI's `transition_active_to_inactive` capability so the plugin can enforce monotonicity at the storage layer (per `cpt-cf-usage-collector-adr-monotonic-deactivation`). Inactive records remain queryable through the Query Gateway, preserving auditable history for downstream consumers while the substrate stays free of mutable-record patterns.

- **Depends On**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-usage-emission`

- **Scope**:
  - Deactivation Handler endpoint (`POST /usage-collector/v1/records/{id}/deactivate`) dispatching a status-only transition.
  - Per-call authentication is owned by the ToolKit gateway upstream of the collector (the gear NEVER consumes `authn-resolver`); per-call PDP authorization runs inline through the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, fail-closed on PDP unavailability with no synthesized identity and no cached PDP decision.
  - One-way `active → inactive` `status` column transition on `usage_records`; no other column is mutated. The latch is uniform across `entry_type` values — neither `usage` nor `compensation` rows have a reverse transition.
  - **Cascade (depth-1 only)**: when the target row has `entry_type = usage`, the Plugin SPI's `transition_active_to_inactive` capability flips the target row AND every currently-active `entry_type = compensation` row with `corrects_id = target_id` (within the same `(tenant_id, gts_id)` scope) from `active` to `inactive` in **one atomic storage-layer transition**; partial cascades are structurally impossible. The cascade is **depth-1 only** — transitive cascade is out of scope; no row may reference a `compensation` row (per `cpt-cf-usage-collector-adr-usage-compensation`), so deactivating a `compensation` row is a single-row, no-cascade operation by construction. The success outcome carries `{ primary_id, cascaded_compensation_ids: [...] }`, where `cascaded_compensation_ids` is non-empty only when at least one active referencing compensation was cascade-flipped. See [§2.6](#26-compensation--medium) Compensation for the producer of the cascaded rows. Cross-link: `cpt-cf-usage-collector-feature-usage-compensation`.
  - **Concurrency rule**: a compensation submission referencing a row R that arrives while R is being deactivated is rejected by the ingestion-path L1 "referenced record must be active" check; no compensation can be admitted referencing a row that has already left `active`. The rule adds no new lock or coordinator — it depends only on the L1 check inlined in `features/usage-emission.md` and the atomicity of the cascade transition above.
  - Atomic monotonic transition via the Plugin SPI's `transition_active_to_inactive` capability; the plugin returns `Transitioned { primary_id, cascaded_compensation_ids }`, `already-inactive`, or `not-found`, and the handler surfaces each outcome deterministically as a 2xx confirmation or actionable error envelope.
  - Audit-trail correlation: a request-level correlation identifier (per `cpt-cf-usage-collector-fr-audit-trail`) is propagated through the deactivation flow so platform gateway and PDP decision logs can be reconciled with gear-level activity.
  - Preserves queryability of inactive records: inactive rows (of either `entry_type`) remain visible to the Query Gateway so downstream consumers can distinguish active from inactive results per `cpt-cf-usage-collector-fr-data-lifecycle`.

- **Out of scope**:
  - Reactivation (`inactive → active`) — the Usage Collector does not provide a reactivation operation, and any such request is rejected; the one-way latch is uniform across `entry_type` values (no reverse transition for cascade-flipped `compensation` rows either).
  - Bulk-by-query deactivation — every deactivation targets exactly one **primary** record by `id`; multi-record selection by filter is not offered. (The depth-1 cascade is **not** a bulk-by-filter selection — it is a structurally-bounded set-flip of active compensations whose `corrects_id` equals the primary row's `id`, performed inside the same atomic transition.)
  - Transitive cascade — the cascade is **depth-1 only**. Compensations do not have `corrects_id` references pointing back at other compensations (a `compensation → compensation` reference is structurally impossible per `cpt-cf-usage-collector-adr-usage-compensation`), so deactivating a `compensation` row produces `cascaded_compensation_ids: []`.
  - Counter value-reversal (refunds, credits, credit-notes, partial releases) — deactivation is **error retraction**, not value-reversal. Caller-driven value-reversal is owned by [§2.6](#26-compensation--medium) Compensation; computing refunds/credits/credit-notes/quota remains a downstream-consumer responsibility per the un-policed-net stance in DESIGN §3.10.3.
  - Field edits of any kind other than the `status` column — no value, timestamp, metadata, tenant, resource, subject, Metric, `entry_type`, `corrects_id`, or idempotency-key mutation is permitted after acceptance.
  - Hard deletion of `usage_records` rows — inactive records (of either `entry_type`) remain queryable; physical retention and purge are owned by the active storage plugin and operator deployment profile, subject to the strict key-preservation obligation that retention may reclaim or purge record bodies but MUST NOT free the `(tenant_id, gts_id, idempotency_key)` dedup key tuple, which the plugin preserves permanently.
  - Gear-local audit event emission for the deactivate operation — owned by the platform gateway access log and PDP decision logs per `cpt-cf-usage-collector-fr-audit-trail`; per-record audit-ledger emission inside the gear is explicitly deferred.
  - Write paths for usage record ingestion, idempotency dedup and conflict rejection (exact-equality retries silently absorbed; canonical-field mismatches rejected as `idempotency_conflict`), counter / gauge kind enforcement, the four-cell value-sign matrix, the L1 `corrects_id` referential checks, and `RecordMetadata` size-cap enforcement — owned by [§2.3](#23-usage-emission-high) Usage Emission (with the compensation flow inlined there) and [§2.6](#26-compensation--medium) Compensation.
  - Aggregated and raw read-path query execution, SUM-nets aggregation, and PDP-constraint composition — owned by [§2.4](#24-usage-query-medium) Usage Query (which continues to return inactive records of either `entry_type` as part of its scope).
  - Metric registration, deletion, and catalog mutation — owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle.
  - Plugin host lifecycle, shared PDP authorization helper definition, REST / SDK / Plugin SPI surface declaration, and deployment topology — owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Gear Foundation & Pluggable Storage.

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-event-deactivation`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-quality`
  - [ ] `p2` - `cpt-cf-usage-collector-fr-data-lifecycle`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-audit-trail`
  - [ ] `p2` - `cpt-cf-usage-collector-nfr-authorization`
  - [ ] `p1` - `cpt-cf-usage-collector-nfr-availability`

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-monotonic-deactivation`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-fail-closed`

- **Design Constraints Covered**:
  - `p2` - `cpt-cf-usage-collector-adr-monotonic-deactivation`
  - `p2` - `cpt-cf-usage-collector-adr-usage-compensation` (depth-1 cascade boundary — compensating a compensation is structurally forbidden, so deactivating a `compensation` row never cascades)
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-no-business-logic`

- **Domain Model Entities**:
  - UsageRecord (`cpt-cf-usage-collector-entity-usage-record`) — only the `status` column is transitioned; all other properties (including `entry_type` and `corrects_id`) are immutable after acceptance.
  - EntryType (`cpt-cf-usage-collector-entity-entry-type`) — discriminator that determines whether cascade evaluation runs (only when `entry_type = usage`).
  - DeactivationStatus (`cpt-cf-usage-collector-entity-deactivation-status`)
  - SecurityContext (`cpt-cf-usage-collector-entity-security-context`)

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-deactivation-handler`

- **API**:
  - POST /usage-collector/v1/records/{id}/deactivate

- **Sequences**:
  - `p1` - `cpt-cf-usage-collector-seq-deactivate-event` (depth-1 cascade surfaced as `{ primary_id, cascaded_compensation_ids: [...] }` per [§2.6](#26-compensation--medium) Compensation cross-link)

- **Data**:
  - `p2` - `cpt-cf-usage-collector-dbtable-usage-records` (shared with [§2.3](#23-usage-emission-high) Usage Emission and [§2.6](#26-compensation--medium) Compensation; this feature touches only the `status` column via a status-only update — possibly across multiple rows in one atomic transition (the primary row plus depth-1 cascaded compensations) — and never any other column)

### 2.6 Compensation ⏳ MEDIUM

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-feature-usage-compensation`

- **Purpose**: Provide the append-only **counter value-reversal** primitive that lets an authorized source gear record a real-world give-back (capacity refund, partial cancellation, dispute resolution, billing-period correction) as a signed-negative `compensation` entry that references a prior `entry_type = usage` row via `corrects_id`. The entry rides the **existing unified ingestion path** (the same REST endpoint, SDK method, and Plugin SPI `persist` capability as ordinary emission — there is NO dedicated `compensate` REST path, SDK method, or SPI call); it is recorded under PDP attribution and a mandatory caller-supplied idempotency key, and is netted into `SUM` aggregations without modifying or annotating the original `usage` row. Compensation is **recording, not computing**: the Usage Collector never decides refunds, credits, credit-notes, quotas, lots, or per-record remaining amounts, and never enforces non-negative net (per the un-policed-net stance in DESIGN §3.10.3). The compensation flow is **inlined inside `features/usage-emission.md`** (under §2 Actor Flows, flow ID `cpt-cf-usage-collector-flow-usage-emission-compensation`); there is NO standalone `features/usage-compensation.md` file.

- **Depends On**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-metric-lifecycle`, `cpt-cf-usage-collector-feature-usage-emission`

- **Scope**:
  - Unified ingestion path: every compensation emit flows through `POST /usage-collector/v1/records` and the same SDK emit method as ordinary usage, dispatched through `cpt-cf-usage-collector-component-ingestion-gateway` and persisted via the Plugin SPI's existing `persist` capability (which now carries `entry_type` ∈ {`usage`, `compensation`}, signed `value`, and optional `corrects_id`).
  - Entry-type discrimination: caller supplies `entry_type = compensation` (the new domain entity `cpt-cf-usage-collector-entity-entry-type`) and a non-empty `corrects_id`; usage emissions MUST NOT carry a `corrects_id`, compensation emissions MUST carry one.
  - Four-cell value-sign matrix (enforced at validation time by the kind-enforcement-on-ingest algorithm): `counter + usage` requires `value >= 0`; `counter + compensation` requires `value < 0` (strictly negative); `gauge + usage` accepts any signed value; `gauge + compensation` is **REJECTED** before persistence (compensation is counter-only).
  - L1 `corrects_id` referential checks (synchronous, on the ingestion path, per `cpt-cf-usage-collector-fr-usage-compensation`): the referenced row MUST exist, MUST have `entry_type = usage` (no compensating a compensation), MUST share `(tenant_id, gts_id)` with the incoming compensation, and MUST be `status = active`. There is NO L2 layer — no per-record remaining-amount tracking, no lot / FIFO-LIFO state, no negative-net detection.
  - PDP attribution and mandatory caller-supplied idempotency key: unchanged from ordinary ingestion. Mandatory idempotency makes retries safe end-to-end and prevents double-refund for free.
  - Concurrency rule: a compensation referencing usage row R that arrives while R is being deactivated is rejected by the L1 "referenced record must be active" check, surfaced on the wire as `context.reason="corrects_id_inactive"` (HTTP `409`) per the locked `usage-collector-v1.yaml` Problem.context.reason taxonomy and `sdk-trait.md` `CorrectsIdInactive`; no quarantine, no retry queue, no compensating-cascade for the rejection (the source gear retries at its own discretion, made safe by the mandatory idempotency key).
  - SUM-nets aggregation contract (consumed by [§2.4](#24-usage-query-medium) Usage Query and implemented by the Plugin SPI's aggregation capability): `SUM(value)` nets across both `entry_type = usage` and `entry_type = compensation` rows treating `value` as signed, so `SUM` is the signed net total per `(tenant_id, gts_id)` group. `COUNT`, `MIN`, `MAX`, and `AVG` operate over `entry_type = usage` rows only — **compensation entries adjust SUM; they are not events.** Status filtering is orthogonal to `entry_type` filtering — deactivated rows of any `entry_type` are excluded before aggregation.
  - Persistence through the Plugin SPI's `persist` capability writes the new columns (`entry_type`, signed `value`, nullable `corrects_id`) atomically with the existing dedup composite `(tenant_id, gts_id, idempotency_key)`; the plugin enforces structural constraints only (schema shape, idempotency-key uniqueness, atomicity, value-sign matrix) and MUST NOT re-execute the caller's L1 checks.
  - Cascade-coupling with Event Deactivation: when a `usage` row is deactivated, every currently-active referencing `compensation` row is cascade-flipped to `inactive` in the same atomic Plugin SPI transition — the producer of those rows is this capability; the depth-1 cascade itself is owned by [§2.5](#25-event-deactivation--medium) Event Deactivation.

- **Out of scope**:
  - Compensating a compensation — forbidden by `cpt-cf-usage-collector-adr-usage-compensation` non-goals; `corrects_id` MUST reference a row with `entry_type = usage`. A `compensation → compensation` reference is structurally impossible, which is what bounds the deactivation cascade to depth-1.
  - Positive or signed compensations — the value-sign matrix REQUIRES `value < 0` for `counter + compensation` and REJECTS any `compensation` against a `gauge` metric. There is no "positive compensation" code path.
  - L2 enforcement / per-record remaining-amount tracking — no remaining-amount column on `usage_records`, no per-lot ledger, no FIFO/LIFO accounting; mandatory idempotency replaces any need for "remaining amount" arithmetic.
  - Negative-net detection or alerting — the Usage Collector does NOT validate non-negative net and does NOT emit a negative-net detection signal per the un-policed-net stance in DESIGN §3.10.3; downstream consumers own any "net can't be negative" policy.
  - Lot / FIFO-LIFO tracking — out of scope for the metering substrate.
  - Computing refunds, credits, credit-notes, or quota balances — explicitly owned by downstream consumers; the Usage Collector records what the source gear decides to apply, never computes one itself, per `cpt-cf-usage-collector-constraint-no-business-logic`.
  - Gauge compensation — REJECTED at validation per the value matrix; gauges only carry point-in-time `usage` values.
  - A dedicated `compensate` REST endpoint, SDK method, or Plugin SPI call — explicitly out of scope per the locked `api_shape = single ingestion path`. Compensation rides the unified ingestion path.
  - A separate `features/usage-compensation.md` document — explicitly out of scope per the locked `feature_doc_shape = inline-in-emission`. The compensation flow is inlined inside `features/usage-emission.md` (flow ID `cpt-cf-usage-collector-flow-usage-emission-compensation`).
  - Event-driven `active → inactive` deactivation (the one-way `status` latch and its depth-1 cascade) — owned by [§2.5](#25-event-deactivation--medium) Event Deactivation. This capability is the **producer** of the rows the cascade flips, not the cascade owner.
  - Aggregated and raw read-path query execution — owned by [§2.4](#24-usage-query-medium) Usage Query; this capability defines the SUM-nets / usage-only aggregation **contract** the query path consumes, but the query execution itself lives there.
  - Metric registration, deletion, and catalog mutation — owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle; the compensation flow only reads the catalog to confirm the target Metric is `counter`.
  - Plugin host lifecycle, REST / SDK / Plugin SPI surface declaration, and deployment topology — owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Gear Foundation & Pluggable Storage.

- **Requirements Covered**:
  - [ ] `p1` - `cpt-cf-usage-collector-fr-usage-compensation`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-idempotency` (compensations carry mandatory caller-supplied idempotency keys on the same dedup composite — exact-equality retries silently absorbed, canonical-field mismatches rejected as `idempotency_conflict`)
  - [ ] `p1` - `cpt-cf-usage-collector-fr-ingestion-authorization` (compensations are PDP-authorized on the same attribution tuple as ordinary emissions)
  - [ ] `p1` - `cpt-cf-usage-collector-fr-counter-semantics` (compensation is counter-only — gauge compensation is rejected)
  - [ ] `p1` - `cpt-cf-usage-collector-fr-tenant-attribution` (compensation MUST share `(tenant_id, gts_id)` with the row it references)
  - [ ] `p1` - `cpt-cf-usage-collector-fr-resource-attribution`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-subject-attribution`
  - [ ] `p1` - `cpt-cf-usage-collector-fr-data-quality` (the L1 `corrects_id` referential checks reject malformed references at the gateway, before plugin dispatch)

- **Design Principles Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-principle-idempotency-by-key`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-kind-enforcement`
  - [ ] `p2` - `cpt-cf-usage-collector-principle-fail-closed`

- **Design Constraints Covered**:
  - [ ] `p2` - `cpt-cf-usage-collector-constraint-no-business-logic` (recording, not computing — symmetric with `+value` recording)
  - `p2` - `cpt-cf-usage-collector-adr-usage-compensation`
  - `p2` - `cpt-cf-usage-collector-adr-mandatory-idempotency`
  - `p2` - `cpt-cf-usage-collector-adr-caller-supplied-attribution`

- **Domain Model Entities**:
  - EntryType (`cpt-cf-usage-collector-entity-entry-type`) — discriminator ∈ {`usage`, `compensation`} on every emitted row; the new domain entity minted alongside this capability.
  - UsageRecord (`cpt-cf-usage-collector-entity-usage-record`) — extended with `entry_type` (NOT NULL) and `corrects_id` (nullable, FK semantic to a same-table row); writer is shared with [§2.3](#23-usage-emission-high) Usage Emission via the unified ingestion path.
  - Metric (`cpt-cf-usage-collector-entity-metric`) — only `kind = counter` accepts compensation.
  - MetricKind (`cpt-cf-usage-collector-entity-metric-kind`) — drives the four-cell value-sign matrix.
  - IdempotencyKey (`cpt-cf-usage-collector-entity-idempotency-key`) — mandatory, same dedup composite as ordinary ingestion.
  - ResourceRef (`cpt-cf-usage-collector-entity-resource-ref`)
  - SubjectRef (`cpt-cf-usage-collector-entity-subject-ref`)
  - SecurityContext (`cpt-cf-usage-collector-entity-security-context`)

- **Design Components**:
  - [ ] `p2` - `cpt-cf-usage-collector-component-ingestion-gateway` (shared with [§2.3](#23-usage-emission-high) Usage Emission via the unified ingestion path; this capability adds entry_type / corrects_id discrimination and the L1 corrects_id checks inside the same component, never as a separate gateway)
  - **Validation** (a logical sub-capacity of the ingestion gateway, NOT a new design component): entry_type semantics, the four-cell value-sign matrix, and the L1 `corrects_id` referential checks (existence ∧ `entry_type = usage` ∧ same `(tenant_id, gts_id)` ∧ `active`).

- **API**:
  - POST /usage-collector/v1/records (shared with [§2.3](#23-usage-emission-high) Usage Emission — the unified ingestion endpoint now accepts `entry_type` and optional `corrects_id` per record; there is NO dedicated `compensate` path)

- **Sequences**:
  - `p1` - `cpt-cf-usage-collector-seq-emit-usage` (shared with [§2.3](#23-usage-emission-high) Usage Emission — the same sequence carries compensation emissions; the inlined `cpt-cf-usage-collector-flow-usage-emission-compensation` flow under §2 of `features/usage-emission.md` documents the compensation-specific preconditions, validation pipeline, and error scenarios)

- **Data**:
  - `p2` - `cpt-cf-usage-collector-dbtable-usage-records` (shared with [§2.3](#23-usage-emission-high) Usage Emission as the single writer of non-status columns; this capability introduces the new `entry_type` (NOT NULL) and `corrects_id` (nullable) columns on the same table, persisted atomically with the existing dedup composite via the Plugin SPI's `persist` capability)

- **Feature flow anchor**: the **inlined "Compensation Emission" flow** inside `features/usage-emission.md` under §2 Actor Flows (`cpt-cf-usage-collector-flow-usage-emission-compensation`). There is NO separate `features/usage-compensation.md` file; the compensation flow is documented alongside the ordinary usage emission flow because both ride the same unified ingestion path.

### 2.7 Deliberate Omissions

The following `cpt-cf-usage-collector-*` IDs from the element inventory are intentionally not assigned to any [§2.1](#21-gear-foundation-pluggable-storage-high)..[§2.6](#26-compensation--medium) feature. Each omission is justified by either the kind being a non-implementation artifact (role descriptions, section anchors, PRD-side abstractions) or by an explicit scope boundary stated in DESIGN/PRD.

- `cpt-cf-usage-collector-actor-platform-developer`: PRD-side role description for the platform developer audience; gear surfaces (SDK trait, REST, Plugin SPI) are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation, no actor-specific implementation artifact required.
- `cpt-cf-usage-collector-actor-platform-operator`: PRD-side role description for the operator audience; operator authority is enforced via the shared PDP authorization helper owned by [§2.1](#21-gear-foundation-pluggable-storage-high), with concrete operator flows (metric register/delete, deactivate) already covered by [§2.2](#22-metric-catalog-lifecycle-high) and [§2.5](#25-event-deactivation-medium).
- `cpt-cf-usage-collector-actor-storage-backend`: PRD-side role description for storage-vendor implementors; the Plugin SPI contract surface they implement is owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation.
- `cpt-cf-usage-collector-actor-tenant-admin`: PRD-side role description for tenant administrators; tenant-scoped read authority is enforced via PDP constraints owned by [§2.4](#24-usage-query-medium) Usage Query and tenant attribution owned by [§2.3](#23-usage-emission-high) Usage Emission.
- `cpt-cf-usage-collector-actor-usage-consumer`: PRD-side role description for downstream consumers of usage data; the consumer-facing read surface is owned by [§2.4](#24-usage-query-medium) Usage Query via the Query Gateway and the downstream-usage-reader contract.
- `cpt-cf-usage-collector-actor-usage-source`: PRD-side role description for source gears emitting usage; the source-gear write surface is owned by [§2.3](#23-usage-emission-high) Usage Emission.
- `cpt-cf-usage-collector-design-usage-collector`: Top-level DESIGN.md section anchor; its constituent design elements (components, sequences, principles, entities) are each individually assigned to features [§2.1](#21-gear-foundation-pluggable-storage-high)..[§2.5](#25-event-deactivation-medium).
- `cpt-cf-usage-collector-design-security-architecture`: DESIGN.md section anchor for the security architecture; constituent principles (`principle-fail-closed`, `principle-pdp-centric-authorization`) and the shared PDP authorization helper definition are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation.
- `cpt-cf-usage-collector-design-reliability-data-architecture`: DESIGN.md section anchor for reliability/data architecture; constituent storage components and data entities are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) (Plugin SPI, db-gear-store) and [§2.3](#23-usage-emission-high) / [§2.4](#24-usage-query-medium) / [§2.5](#25-event-deactivation-medium) (usage_records).
- `cpt-cf-usage-collector-design-performance-operations-architecture`: DESIGN.md section anchor for performance/operations architecture; constituent NFRs (throughput, latency, workload-isolation, operational-visibility) are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation and [§2.3](#23-usage-emission-high) Usage Emission.
- `cpt-cf-usage-collector-design-maintainability-testing-ux-integration`: DESIGN.md section anchor for maintainability/testing/UX/integration architecture; constituent NFRs (developer-operator-experience, documentation-coverage, error-experience) are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation.
- `cpt-cf-usage-collector-design-capacity-cost-compliance-doc-architecture`: DESIGN.md section anchor for capacity/cost/compliance/doc architecture; constituent NFRs (capacity-headroom, scalability) are owned by [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation and the explicit compliance non-goal is captured below.
- `cpt-cf-usage-collector-usecase-register-metric`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-register-metric` owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle; no separate implementation artifact required.
- `cpt-cf-usage-collector-usecase-delete-metric`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-delete-metric` owned by [§2.2](#22-metric-catalog-lifecycle-high) Metric Catalog & Lifecycle.
- `cpt-cf-usage-collector-usecase-emit`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-emit-usage` owned by [§2.3](#23-usage-emission-high) Usage Emission.
- `cpt-cf-usage-collector-usecase-query-aggregated`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-query-aggregated` owned by [§2.4](#24-usage-query-medium) Usage Query.
- `cpt-cf-usage-collector-usecase-query-raw`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-query-raw` owned by [§2.4](#24-usage-query-medium) Usage Query.
- `cpt-cf-usage-collector-usecase-deactivate-event`: PRD-side use case realized 1:1 by `cpt-cf-usage-collector-seq-deactivate-event` owned by [§2.5](#25-event-deactivation-medium) Event Deactivation.
- `cpt-cf-usage-collector-constraint-legacy-none`: Meta-marker stating there are no legacy-system constraints applicable to the Usage Collector — no implementation artifact to assign.
- `cpt-cf-usage-collector-constraint-pii-identity-layer`: Explicit out-of-scope marker stating PII handling is owned by the platform identity layer, not the Usage Collector; no implementation in this gear.
- `cpt-cf-usage-collector-constraint-regulatory-out-of-scope`: Explicit out-of-scope marker stating regulatory (HIPAA/PCI/etc.) compliance is owned by downstream consumers and the platform, not the metering substrate; no implementation in this gear.
- `cpt-cf-usage-collector-adr-gateway-local-metric-catalog` (ADR-0007), `cpt-cf-usage-collector-adr-catalog-plugin-referential-integrity` (ADR-0009), and `cpt-cf-usage-collector-adr-gts-typed-metric-metadata` (ADR-0010): **SUPERSEDED** by `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (ADR-0012, including its 2026-06-02 amendment). The two-catalog model and the uuid5-from-type derivation are retired: there is a single plugin-DB catalog managed via SDK/REST, usage records reference metrics via `gts_id` directly, and the metric specification no longer carries `parent_type_uuid`, `x-uc-indexable`, or `abstract` complexity attributes. All ADR-0007/0009/0010 references throughout this DECOMPOSITION have been retargeted to ADR-0012 — and any reference that previously pointed at ADR-0010 as the source of the per-metric JSON-Schema validator behavior is repointed to "ADR-0012 Amendment 2026-06-02", which replaces that surface with closed `metadata_fields: Vec<String>` (all values typed as String); the gateway retains an L1 catalog cache (keyed by `gts_id`, content `Map<gts_id, {kind, metadata_fields: HashSet<String>}>`) that fronts the plugin SoR, with flat-keyspace invalidation (no ancestor chain, no descendant-cascade invalidation), and no gateway-local catalog state.
- _cpt-cf-usage-collector-seq-boot-seed-declared-metrics_ (deleted ID, no backticks): **REMOVED** from DESIGN by ADR-0012 (declared-catalog model retired in favor of a single plugin-DB catalog managed via SDK/REST). No boot-time seeding sequence exists; the dropped workstream that owned this sequence under [§2.1](#21-gear-foundation-pluggable-storage-high) Foundation has been retired in this DECOMPOSITION.

## 3. Feature Dependencies

```text
cpt-cf-usage-collector-feature-foundation
    │
    ├─→ cpt-cf-usage-collector-feature-metric-lifecycle
    │       │
    │       ├─→ cpt-cf-usage-collector-feature-usage-emission        (also ← foundation)
    │       │       │
    │       │       ├─→ cpt-cf-usage-collector-feature-usage-compensation   (also ← foundation, metric-lifecycle)
    │       │       │         │
    │       │       │         └─→ cpt-cf-usage-collector-feature-usage-query   (SUM-nets aggregation contract)
    │       │       │
    │       │       ├─→ cpt-cf-usage-collector-feature-usage-query           (also ← foundation, metric-lifecycle)
    │       │       │
    │       │       └─→ cpt-cf-usage-collector-feature-event-deactivation    (also ← foundation)
    │       │                 │
    │       │                 └─ ─ ─depth-1 cascade─ ─ ─→ cpt-cf-usage-collector-feature-usage-compensation
    │       │
    │       └─→ (also reached by usage-query and usage-compensation directly for catalog reads)
```

Direct edges captured by the diagram: foundation → {metric-lifecycle, usage-emission, usage-query, event-deactivation, usage-compensation}; metric-lifecycle → {usage-emission, usage-query, usage-compensation}; usage-emission → {usage-query, event-deactivation, usage-compensation}; usage-compensation → usage-query (SUM-nets aggregation contract); event-deactivation ⇢ usage-compensation (depth-1 cascade — operational coupling, not a feature-implementation prerequisite, because deactivation operates on rows produced by compensation but does not require the compensation capability to be implemented first; the cascade is a no-op when no compensation row exists).

**Dependency Rationale**:

- `cpt-cf-usage-collector-feature-metric-lifecycle` requires `cpt-cf-usage-collector-feature-foundation`: the catalog mutation flow needs the substrate's Plugin SPI (to persist Metric definitions), the shared PDP authorization helper (to gate operator authority against `cpt-cf-usage-collector-contract-authz-resolver`), and the GTS Registry contract (to resolve the configured plugin binding) — all owned by Foundation.
- `cpt-cf-usage-collector-feature-usage-emission` requires `cpt-cf-usage-collector-feature-foundation`: the ingestion path uses the Plugin SPI for durable persistence and the shared PDP authorization helper (against `cpt-cf-usage-collector-contract-authz-resolver`) for per-call attribution authorization, both owned by Foundation.
- `cpt-cf-usage-collector-feature-usage-emission` requires `cpt-cf-usage-collector-feature-metric-lifecycle`: every accepted emit consults the Metric Catalog for Metric existence and kind enforcement before plugin dispatch, so the catalog projection owned by Metric Lifecycle must exist first.
- `cpt-cf-usage-collector-feature-usage-query` requires `cpt-cf-usage-collector-feature-foundation`: the Query Gateway anchors every read on the resolved `SecurityContext` and composes user-supplied filters with `PdpConstraint`s returned by the shared PDP authorization helper (against `cpt-cf-usage-collector-contract-authz-resolver`), both owned by Foundation, and dispatches reads through the Plugin SPI binding.
- `cpt-cf-usage-collector-feature-usage-query` requires `cpt-cf-usage-collector-feature-metric-lifecycle`: the aggregated path validates the mandatory single-Metric filter against the Metric Catalog on every request, rejecting unregistered Metric references before plugin dispatch per `cpt-cf-usage-collector-fr-metric-existence-and-kind` — this is a direct call into [§2.2](#22-metric-catalog-lifecycle-high)'s catalog projection, not a transitive read through [§2.3](#23-usage-emission-high).
- `cpt-cf-usage-collector-feature-usage-query` requires `cpt-cf-usage-collector-feature-usage-emission`: aggregated and raw reads scan the `usage_records` table that is written exclusively by the ingestion path — there is nothing to read until Usage Emission has accepted records.
- `cpt-cf-usage-collector-feature-usage-query` requires `cpt-cf-usage-collector-feature-usage-compensation`: the SUM-nets aggregation contract (signed `SUM` nets `usage + compensation`; `COUNT/MIN/MAX/AVG` operate over `usage` rows only — "compensation entries adjust SUM; they are not events") is defined by Compensation and consumed by the Query Gateway's aggregation path. Without Compensation's value-sign semantics and `entry_type` discriminator, the Query Gateway cannot realize SUM-nets aggregation.
- `cpt-cf-usage-collector-feature-event-deactivation` requires `cpt-cf-usage-collector-feature-foundation`: the Deactivation Handler receives the operator's already-resolved `SecurityContext` (populated upstream by the ToolKit gateway via `OperationBuilder::authenticated()` on REST or supplied directly to the SDK; the gear NEVER consumes `authn-resolver`) and authorizes the transition inline through the shared PDP authorization helper against `cpt-cf-usage-collector-contract-authz-resolver`, both owned by Foundation, and dispatches the status-only transition through the Plugin SPI binding.
- `cpt-cf-usage-collector-feature-event-deactivation` requires `cpt-cf-usage-collector-feature-usage-emission`: the one-way `active → inactive` `status`-column transition targets exactly one **primary** row in `usage_records`, which is written exclusively by Usage Emission — no row can be deactivated until it has first been ingested.
- `cpt-cf-usage-collector-feature-event-deactivation` is coupled to `cpt-cf-usage-collector-feature-usage-compensation` via the **depth-1 cascade**: when the primary row has `entry_type = usage`, the Plugin SPI's `transition_active_to_inactive` capability cascade-flips every currently-active `entry_type = compensation` row with `corrects_id = target_id` (within the same `(tenant_id, gts_id)` scope) from `active` to `inactive` in the same atomic transition. This is a **runtime coupling**, not a hard implementation-prerequisite: deactivation MAY ship before Compensation's writer surface is exercised (the cascade is structurally a no-op when no compensation rows exist), but the cascade outcome shape (`{ primary_id, cascaded_compensation_ids: [...] }`) is jointly owned by both capabilities. The structural depth-1 bound comes from `cpt-cf-usage-collector-adr-usage-compensation` (compensating a compensation is forbidden), not from a runtime check.
- `cpt-cf-usage-collector-feature-usage-compensation` requires `cpt-cf-usage-collector-feature-foundation`: the compensation flow rides the unified ingestion path through the shared PDP authorization helper (against `cpt-cf-usage-collector-contract-authz-resolver`) and the Plugin SPI's existing `persist` capability, both owned by Foundation.
- `cpt-cf-usage-collector-feature-usage-compensation` requires `cpt-cf-usage-collector-feature-metric-lifecycle`: the four-cell value-sign matrix needs the target Metric's `kind` (compensation is counter-only — `gauge + compensation` is rejected before persistence), consulted via the Metric Catalog projection on every compensation emit.
- `cpt-cf-usage-collector-feature-usage-compensation` requires `cpt-cf-usage-collector-feature-usage-emission`: compensation rides the **same unified ingestion path** (the Ingestion Gateway component, the same REST endpoint, the same SDK emit method, the same Plugin SPI `persist` capability) as ordinary emission — it extends Usage Emission's writer surface with `entry_type` and `corrects_id` columns; it does not introduce a parallel writer surface. The L1 `corrects_id` referential check requires that the referenced `entry_type = usage` row already exists, so the writer side of Usage Emission must be operational before any compensation can be recorded.
- `cpt-cf-usage-collector-feature-usage-compensation` and `cpt-cf-usage-collector-feature-event-deactivation` are mutually independent **as capability implementations** but **operationally coupled** at runtime via the depth-1 cascade (described above). Neither requires the other to be implemented first; both extend the same `usage_records` table and Plugin SPI surface owned by Foundation and Usage Emission.
- `cpt-cf-usage-collector-feature-usage-query` and `cpt-cf-usage-collector-feature-event-deactivation` are independent of each other and can be developed in parallel: they share upstream dependencies on Foundation and Usage Emission but neither produces input consumed by the other (the Query Gateway reads `usage_records` for both `active` and `inactive` rows of either `entry_type`; the Deactivation Handler writes only the `status` column — possibly across multiple rows in one atomic transition via the depth-1 cascade — and does not depend on any query path).

## 4. Crate Layout & Platform Dependencies

The Usage Collector ships exactly two first-party crates following the platform-standard `<gear>` + `<gear>-sdk` two-crate layout used by every reference gear (`credstore`, `authn-resolver`, `authz-resolver`). There is no separate `-contracts` crate and no separate `-plugin-api` crate: the consumer SDK trait, the plugin trait, the GTS spec for plugin discovery, the domain models, and the public error enum all live inside the single `usage-collector-sdk` crate alongside each other.

### 4.1 Two-crate layout

- `usage-collector-sdk` (public contract crate):
  - Purpose: public contract surface consumed in-process by source gears and downstream readers AND by plugin authors. Single source of truth for the SDK trait, the Plugin trait, the GTS spec, the domain models, and the public error enum.
  - File layout under `src/`:
    - `api.rs` — `UsageCollectorClientV1` trait (consumer SDK trait; what gears call via ClientHub).
    - `plugin_api.rs` — `UsageCollectorPluginV1` trait (what plugin authors implement).
    - `gts.rs` — GTS spec for plugin discovery and binding (reserved; populated by the plugin-registration step per DESIGN §3.12.9).
    - `models.rs` — domain data types: `UsageRecord`, `ResourceRef`, `SubjectRef`, `Metric`, `MetricKind`, `IdempotencyKey`, `RecordMetadata`, `DeactivationStatus`, `PdpDecision`, `PdpConstraint`, `PluginBinding`, `AggregationQuery`, `AggregationResult`, `RawQuery`, `UsageRecordFilterField`, `Keyset`, and related plain Rust types. MUST NOT derive `utoipa::ToSchema`.
    - `error.rs` — public error enum surfaced through the SDK trait and the Plugin trait.
    - `lib.rs` — re-exports.

- `usage-collector` (host gear crate):
  - Purpose: REST machinery, gear wiring, plugin resolution, and the in-process implementation of the SDK trait.
  - File layout under `src/`:
    - `gear.rs` — `#[toolkit::gear]` entrypoint, ClientHub wiring, REST route registration.
    - `config.rs` — gear config.
    - `domain/service.rs` — business logic, plugin dispatch.
    - `domain/local_client.rs` — `UsageCollectorLocalClient` implementing `UsageCollectorClientV1`, registered un-scoped into ClientHub for in-process callers via `ctx.client_hub().register::<dyn UsageCollectorClientV1>(...)`.
    - `domain/error.rs` — internal `DomainError` with `From` bridges to the SDK error enum.
    - `api/rest/routes.rs` — `OperationBuilder` registrations.
    - `api/rest/handlers.rs` — thin pass-throughs that call the local client.
    - `api/rest/dto.rs` — wire DTOs with a `Dto` suffix (e.g. `UsageRecordDto`, `AggregationRequestDto`, `AggregationResultDto`), each deriving `serde::Serialize` / `serde::Deserialize` and `utoipa::ToSchema`.
    - `api/rest/mappers.rs` — explicit `From` / `TryFrom` (or named) functions that convert between domain entities (from `usage-collector-sdk`) and DTOs. Mapping is one-way per direction and never embedded inside handlers.
    - `infra/` — implementation glue (e.g. `sdk_error_mapping.rs` for translating internal `DomainError` to the public SDK error enum).
  - OData parsing, gateway-cursor handling (decode / validate / re-issue `CursorV1` over the standardized `(timestamp, id)` keyset), and canonical error mapping live in this crate, behind `OperationBuilder` route registrations that produce the runtime-emitted OpenAPI document via `OpenApiRegistryImpl`.

- Concrete-plugin crates (out of scope to implement here; the spec only describes how they plug in): one per backend under `gears/system/usage-collector/plugins/<backend>/` (e.g. `usage-collector-plugin-clickhouse`, `usage-collector-plugin-timescaledb`), depend on `usage-collector-sdk` only — never on the host crate — and are compiled in at the workspace level.

### 4.2 Direct platform dependencies

The crates depend directly on the following ToolKit platform crates (existing edges to storage plugins, the `authz-resolver` consumer SDK (per `cpt-cf-usage-collector-contract-authz-resolver`), the GTS Registry contract, and the runtime are preserved unchanged):

- `usage-collector-sdk` (public contract crate):
  - `toolkit` — ToolKit core building blocks consumed by the SDK and Plugin traits.
  - `toolkit-gts` and `gts`, `gts-macros` — GTS spec macros and runtime used to declare the plugin discovery type system.
  - `toolkit-security` — `SecurityContext` and related security primitives surfaced through the trait signatures.
  - `async-trait` — used by the SDK and Plugin trait definitions.
  - `thiserror` — error enum derivation in `error.rs`.
  - `serde`, `schemars` — domain-model derives (plain serialization only; no `utoipa::ToSchema`).
  - `toolkit-odata` (optional) — for `Page<T>` re-export and the `FilterField` derive on `UsageRecordFilterField` consumed by the Plugin trait.

  The SDK crate does **NOT** depend on `toolkit-canonical-errors`. Consumers pattern-match `UsageCollectorError` variants directly; the lift to `toolkit_canonical_errors::CanonicalError` lives in the host crate at `usage-collector/src/infra/sdk_error_mapping.rs`. This mirrors the platform standard set by `account-management-sdk`, `credstore-sdk`, `authn-resolver-sdk`, and `authz-resolver-sdk`: SDK crates publish a flat gear-specific error enum (via `thiserror::Error`) and never take a dependency on the canonical-errors envelope crate.

- `usage-collector` (host gear crate):
  - `usage-collector-sdk` — the public contract crate (path dependency).
  - `toolkit` — ToolKit core building blocks for gear wiring and REST registration.
  - `toolkit-canonical-errors` — provides the canonical `Problem` error envelope. The host crate's `infra/sdk_error_mapping.rs` lifts `UsageCollectorError` (from the SDK) into `toolkit_canonical_errors::CanonicalError`, whose built-in `IntoResponse` produces the RFC-9457 `Problem` response. The gear-specific `context.reason` discriminator names (`cursor_decode`, `order_mismatch`, `filter_mismatch`, `unknown_metric`, `kind_invariant`, …) are produced by the host's lift from `UsageCollectorError` variants (multiple discriminator names may originate from a single variant — e.g. `cursor_decode` / `order_mismatch` / `filter_mismatch` all lift from `UsageCollectorError::Validation`), not from the platform crate.
  - `toolkit-odata` — `Page<T>` re-export and OData query parsing (`$filter`, `$orderby`, `$top`, `cursor`), plus `toolkit_odata::validate_cursor_against` for cursor binding checks.
  - `types-registry-sdk` — GTS instance and type registry lookups (`TypesRegistryClient::list_instances`) consumed by the host's `GtsPluginSelector` lazily on the first dispatch after the `types-registry` is consistent (single-flight `get_or_init`, cached for the `Service`'s lifetime); binding changes require a gear restart.
  - `toolkit-security` — `SecurityContext` propagation across REST handlers and the local client.
  - `axum`, `tokio`, and other standard runtime / HTTP dependencies.

### 4.3 Plugin discovery and dispatch

Plugin discovery follows the platform-standard `PluginV1<P>` + `types-registry` + `ClientHub` pattern shared with `credstore`, `authn-resolver`, and `authz-resolver`, per `cpt-cf-usage-collector-principle-plugin-resolution-via-client-hub` and DESIGN §3.5 "Plugin Resolution and Dispatch". The lifecycle has four steps; the host crate has no compile-time dependency on any concrete plugin crate, so binding is purely a runtime concern resolved through `types-registry` + `ClientHub`.

1. **SDK declares the GTS spec**: `usage-collector-sdk/src/gts.rs` declares the unit-struct `UsageCollectorPluginSpecV1` via `#[gts_type_schema(base = PluginV1, schema_id = "gts.cf.toolkit.plugins.plugin.v1~cf.core.usage_collector.plugin.v1~", description = "Usage Collector plugin specification", properties = "")]`. The empty `properties = ""` is intentional — instance metadata (`vendor`, `priority`) is carried by the `PluginV1<P>` base type and is not duplicated in the usage-collector-specific spec.
2. **Plugin `init()` publishes the instance**: each `usage-collector-plugin-<backend>` crate's `#[toolkit::gear]` `init(...)` calls `PluginV1::<UsageCollectorPluginSpecV1>::build_registration("<vendor>.<package>.usage_collector_plugin.v1", cfg.vendor, cfg.priority)?` to assemble the `(instance_id, payload)` pair, registers the payload through `ctx.client_hub().get::<dyn TypesRegistryClient>()?.register(vec![payload]).await?` (gated by `RegisterResult::ensure_all_ok`), and registers the trait object via `ctx.client_hub().register_scoped::<dyn UsageCollectorPluginV1>(ClientScope::gts_id(&instance_id), api)`.
3. **Host resolves and caches the bound instance**: the host's `cpt-cf-usage-collector-component-plugin-host` (in `usage-collector/src/domain/service.rs`) holds a `GtsPluginSelector` that lazily resolves the bound plugin instance — it queries `types-registry` with `UsageCollectorPluginSpecV1::gts_schema_id()` for the pattern `gts.cf.toolkit.plugins.plugin.v1~cf.core.usage_collector.plugin.v1~*`, then calls `choose_plugin_instance::<UsageCollectorPluginSpecV1>(&self.vendor, instances)` to pick the lowest-priority match for the configured `[usage_collector].vendor`. The resolved `GtsInstanceId` is cached in the selector for the `Service`'s lifetime.
4. **Per-request dispatch is an in-memory scoped lookup**: each ingestion / query / deactivation / Metric-lifecycle call resolves the bound plugin by `self.hub.try_get_scoped::<dyn UsageCollectorPluginV1>(&ClientScope::gts_id(instance_id.as_ref()))` and dispatches against the returned `Arc<dyn UsageCollectorPluginV1>`. There is no `types-registry` round-trip on the warm path; a cold-path lookup (first call after bootstrap, or after a binding refresh) executes the selector's resolution + caches the result.

Compile-time linkage is static at the workspace level: plugin crates are built as part of the same Cargo workspace and registered with ToolKit via `#[toolkit::gear]` at startup, but the host `usage-collector` crate depends only on `usage-collector-sdk` and `types-registry-sdk` — never on a concrete `usage-collector-plugin-<backend>` crate. Adding or swapping a plugin is a workspace-build + config-vendor change, not a host-crate change.

## 5. Document Changelog

| Version | Date       | Author          | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| ------- | ---------- | --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.3.0   | 2026-06-02 | Cypilot Phase 7 | Cascaded the `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` 2026-06-02 amendment (simplifications 5 + 6) into the decomposition. Dropped every `jsonschema` runtime-dependency mention and every `metadata_schema JSONB` / `traits JSONB` / `Draft-07` reference; updated the catalog row to flat `gts_id` + `metadata_fields TEXT[]` + `created_at` (no `kind` column — derived from the `gts_id` prefix); updated the gateway L1 cache description from "compiled JSON-Schema validators" to `Map<gts_id, {kind, metadata_fields: HashSet<String>}>` (cache component preserved, content changed); rewrote ingest-time validation prose from "metadata-shape against the compiled validator" to "closed-key membership against the declared `metadata_fields` set" with the Problem `context.reason` now `unknown_metadata_key`; updated the §2.7 ADR-0007/0009/0010 supersession block to call out the amendment explicitly; no ADR-0010 references survive outside that supersession block.          |
| 0.2.0   | 2026-06-02 | Cypilot Phase 4 | Cascaded `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` into the decomposition. Dropped the boot-seed-declared-metrics sequence (deleted ID, no backticks) from [§2.1](#21-gear-foundation--pluggable-storage--high) Foundation; renamed all "Metrics Catalog" prose to the canonical "Metric Catalog"; retargeted `metric_type_uuid`/`type_uuid` → `gts_id` in dedup composites, FK columns, and `corrects_id` scope tuples; retired the `read_metric_chain` / `effective_schema()` / descendant-cascade L1-invalidation work-packages and the `parent_type_uuid` / `x-uc-indexable` / `abstract` complexity attributes from [§2.2](#22-metric-catalog--lifecycle--high) Metric Catalog & Lifecycle; renamed error variants `MetricTypeAlreadyExists` → `MetricAlreadyExists` and `MetricTypeNotFound` → `MetricNotFound`; rewrote the [§2.7](#27-deliberate-omissions) ADR-0007/0009/0010 supersession block to point at ADR-0012 and added a new omission entry for the removed boot-seed sequence. |
