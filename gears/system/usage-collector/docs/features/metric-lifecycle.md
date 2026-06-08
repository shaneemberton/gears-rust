<!--
cpt:
  version: 0.2.1
  updated: 2026-06-02
-->

# Feature: Metric Catalog & Lifecycle

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
  - [1.5 Explicit Non-Applicability](#15-explicit-non-applicability)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Register Metric](#register-metric)
  - [Delete Metric](#delete-metric)
  - [List Metrics](#list-metrics)
  - [Read Metric](#read-metric)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Metric Shape Validation](#metric-shape-validation)
  - [Catalog Kind Lookup](#catalog-kind-lookup)
  - [Gateway L1 Metadata Validation](#gateway-l1-metadata-validation)
- [4. States (CDSL)](#4-states-cdsl)
  - [Metric Registration Lifecycle State Machine](#metric-registration-lifecycle-state-machine)
- [5. Definitions of Done](#5-definitions-of-done)
  - [FR: Metric Registration](#fr-metric-registration)
  - [FR: Metric Deletion](#fr-metric-deletion)
  - [FR: Metric Existence and Kind](#fr-metric-existence-and-kind)
  - [FR: Counter Semantics](#fr-counter-semantics)
  - [FR: Gauge Semantics](#fr-gauge-semantics)
  - [NFR: Authorization](#nfr-authorization)
  - [NFR: Availability](#nfr-availability)
  - [Principle: Kind Enforcement](#principle-kind-enforcement)
  - [Constraint: No Business Logic](#constraint-no-business-logic)
  - [Component: Metric Catalog](#component-metric-catalog)
  - [Sequence: Register Metric](#sequence-register-metric)
  - [Sequence: Delete Metric](#sequence-delete-metric)
  - [Data: Metric Catalog Table](#data-metric-catalog-table)
  - [Entity: Metric](#entity-metric)
  - [Entity: Metric Kind](#entity-metric-kind)
  - [API: POST /usage-collector/v1/metric-types](#api-post-usage-collectorv1metric-types)
  - [API: DELETE /usage-collector/v1/metric-types/{gts_id}](#api-delete-usage-collectorv1metric-typesgts_id)
  - [API: GET /usage-collector/v1/metric-types](#api-get-usage-collectorv1metric-types)
  - [API: GET /usage-collector/v1/metric-types/{gts_id}](#api-get-usage-collectorv1metric-typesgts_id)
  - [Error Mapping: SPI → REST / SDK](#error-mapping-spi--rest--sdk)
  - [§2.2-item → DoD-ID Coverage Matrix](#22-item--dod-id-coverage-matrix)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Changelog](#7-changelog)

<!-- /toc -->

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-metric-lifecycle`

<!-- reference to DECOMPOSITION entry -->

- [ ] `p2` - `cpt-cf-usage-collector-feature-metric-lifecycle`

## 1. Feature Context

### 1.1 Overview

Provides the operator-driven lifecycle for Metric definitions — register, list, get, and delete — so the platform-global metric catalog, persisted in the active storage plugin's database and managed via the Plugin SPI, exists as a single authoritative surface that the ingestion path consults for kind-and-existence enforcement and the query path consults for Metric validation, with PDP-gated mutations. The metric catalog is the sole catalog per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; no boot-seeded declared catalog and no gateway-resident config-loaded catalog exist.

**Consistency posture.** The public catalog read surfaces (`list_metric_types`, `read_metric_type` on the SDK trait and the equivalent REST endpoints) are governed by the gear-level consistency floor of `cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011) and DESIGN [§3.10.8](../DESIGN.md#3108-consistency-contract): eventually consistent with no upper bound at the gear floor relative to the prior `register_metric_type` / `delete_metric_type` ack. The **gateway L1 validator cache** consulted by the ingestion hot path is internal to ingestion (not a public read surface) and is governed by the cache mechanics in DESIGN §3.11.1 (synchronous invalidation on register / delete keyed by `gts_id`, per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), NOT by the consistency floor; the floor and the cache mechanics are independent. Operators that need post-register / post-delete read-back for an immediate decision MUST use the synchronous register / delete ack returned by this feature, not a follow-up `list` or `read`.

### 1.2 Purpose

This feature exists so the operator-controlled metric catalog is the single authoritative surface for Metric existence, Metric Kind (derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and the closed `metadata_fields` declaration across the gear: registration and deletion are gated by per-component PDP enforcement (the gateway Metric Catalog service invokes the `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver` directly; the same domain service is also reachable in-process via the SDK trait methods `UsageCollectorClient::register_metric_type` / `delete_metric_type` / `list_metric_types` / `read_metric_type` and dispatches catalog reads and writes through `cpt-cf-usage-collector-contract-storage-plugin` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) so only authorized platform operators can mutate the catalog; and an in-memory Level-1 cache populated from the plugin SoR serves Metric existence and per-metric `metadata_fields` lookups to the latency-critical ingestion path per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (`MetricKind` is derived from the `gts_id` prefix on every lookup, not stored in the cache; gateway L1 validation is on the hot path; cache invalidation on register/delete is load-bearing).

**Requirements**: `cpt-cf-usage-collector-fr-metric-registration`, `cpt-cf-usage-collector-fr-metric-deletion`, `cpt-cf-usage-collector-fr-metric-existence-and-kind`, `cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-gauge-semantics`, `cpt-cf-usage-collector-nfr-authorization`, `cpt-cf-usage-collector-nfr-availability`

**Principles**: `cpt-cf-usage-collector-principle-kind-enforcement`

### 1.3 Actors

| Actor                                             | Role in Feature                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-actor-platform-operator`  | Registers and deletes Metric type definitions via either the REST surface (`POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`) or the SDK trait methods `UsageCollectorClient::register_metric_type` / `delete_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; sole mutator of the platform-global metric catalog, gated by PDP authorization                                                                                                                                                                                                                                                                                                      |
| `cpt-cf-usage-collector-actor-platform-developer` | Consumes the catalog read surface via either REST (`GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`) or the SDK trait methods `UsageCollectorClient::list_metric_types` / `read_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` for Metric existence, Metric Kind (derived from the `gts_id` prefix), and `metadata_fields` discovery during source-gear integration; in-process consumption on the ingestion hot path goes through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` against the gateway L1 cache populated from the plugin SoR per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md) -- Metric Existence and Kind Enforcement §5.7, Metric Registration §5.7, Metric Deletion §5.7, Counter §5.2, Gauge §5.2, Authorization Enforcement §6.1, High Availability §6.1
- **Design**: [DESIGN.md](../DESIGN.md) -- Metric Catalog component (§3.2), Register / Delete Metric sequences (§3.6), `metric_catalog` row shape (§3.7) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, PRD→DESIGN realization for fr-metric-registration / fr-metric-deletion / fr-metric-existence-and-kind / fr-counter-semantics / fr-gauge-semantics (§5.3)
- **Decomposition**: [DECOMPOSITION.md](../DECOMPOSITION.md) -- §2.2 Metric Catalog & Lifecycle
- **Foundation feature**: [foundation.md](./foundation.md) -- SecurityContext acceptance at the REST surface (`Extension<SecurityContext>` from ToolKit gateway middleware via `OperationBuilder::authenticated()`) and at the SDK trait surface (`&SecurityContext` argument), PDP enforcement via the per-component `authz_scope` helper, plugin host, gateway-resident auxiliary DB binding (`DBProvider<UsageCollectorError>`), audit-correlation, tenant isolation (reused, not re-defined); the durable `metric_catalog` table itself lives in the plugin's backend database per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
- **Plugin SPI reference**: [plugin-spi.md](../plugin-spi.md) -- the catalog SoR lives in the plugin; catalog write/read/list/delete SPI methods carry the `gts_id` + `metadata_fields` payload (`MetricKind` is derived from the `gts_id` prefix; not a separate SPI payload field) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.
- **SDK trait reference**: [sdk-trait.md](../sdk-trait.md) -- `UsageCollectorClient::register_metric_type` / `delete_metric_type` / `list_metric_types` / `read_metric_type` and the flat `UsageCollectorError` variants `MetricAlreadyExists`, `MetricNotFound`, `MetricReferenced`, `InvalidKindPrefix`, `UnknownMetadataKey`
- **REST contract**: [usage-collector-v1.yaml](../usage-collector-v1.yaml) -- `/usage-collector/v1/metric-types` paths keyed by `{gts_id}`
- **ADR references**: [ADR/0012-unified-plugin-catalog-and-gts-id-reference.md](../ADR/0012-unified-plugin-catalog-and-gts-id-reference.md) -- `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (supersedes ADR 0007 / 0009 / 0010; see §7 Changelog)
- **Dependencies**: `cpt-cf-usage-collector-feature-foundation`

### 1.5 Explicit Non-Applicability

- **UX** (`UX-FDESIGN-001` user journey, `UX-FDESIGN-002` accessibility): Not applicable because the metric-lifecycle feature is a backend surface reachable via both the public REST contract (`POST/DELETE/GET /usage-collector/v1/metric-types`) and the in-process SDK trait `UsageCollectorClient::register_metric_type` / `delete_metric_type` / `list_metric_types` / `read_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, and is consumed by source gears in-process via the gateway L1 cache through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`; there is no human-facing UI in this gear, and any UI consumption of Metric definitions is delivered by upstream products outside this scope. User-friendliness on the operator surface is encoded through the deterministic `Problem` error envelopes published by `usage-collector-v1.yaml` (REST) and the flat `UsageCollectorError` enum (SDK).

## 2. Actor Flows (CDSL)

### Register Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:

- A platform operator submits a Metric **type** definition (`gts_id` ending `~` and beginning with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`, plus a closed `metadata_fields: array<string>` declaring the allowed metadata key names) either via `POST /usage-collector/v1/metric-types` or via the SDK trait method `UsageCollectorClient::register_metric_type` (both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`); the REST handler receives `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) (or, on the SDK path, the in-process trait impl receives `&SecurityContext` directly) and delegates to the gateway Metric Catalog service, the gateway service invokes the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) to authorize the register operation, the gateway validates the type shape (including the kind-prefix check against the two reserved prefixes), the gateway dispatches the catalog write through `cpt-cf-usage-collector-contract-storage-plugin` (the plugin persists the new row in `metric_catalog` inside a transaction with the unique constraint on `gts_id` rejecting duplicates, and the row carries `metadata_fields` as a typed array of strings), the gateway L1 cache is synchronously refreshed for the new `gts_id` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, and the canonical `MetricTypeRecord` resource is returned with a `Location` header per `usage-collector-v1.yaml`.

**Error Scenarios**:

- Request arrives without a resolved `SecurityContext` (the ToolKit gateway middleware rejected the call upstream, so the REST handler is never invoked) — the canonical `Unauthenticated` `Problem` envelope is returned by the gateway; the collector never synthesizes identity and no catalog mutation occurs.
- PDP denies the register operation — propagated platform-authorization error envelope from `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`); no catalog mutation occurs.
- Type shape is invalid in one of two distinct ways: (a) the supplied `gts_id` does not begin with one of the two reserved kind base type id prefixes (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) — actionable `InvalidKindPrefix` envelope (HTTP `400`, `context.reason="invalid_kind_prefix"`) is returned before any plugin dispatch; (b) `metadata_fields` is malformed (missing, not an array of strings, contains empty strings, or contains duplicates) — actionable fields-validation envelope (HTTP `400`) is returned before any plugin dispatch.
- Duplicate `gts_id` already present in the plugin's `metric_catalog` table — the unique constraint surfaces a `MetricAlreadyExists` SPI error which the gateway returns as an actionable conflict error envelope (HTTP `409`) without mutating the L1 cache.
- Plugin transport or persistence failure — propagated as a deterministic platform error envelope; no synthesized Metric handle and no L1 cache mutation.

**Steps**:

1. [ ] - `p1` - Operator submits the register request via either `POST /usage-collector/v1/metric-types` or `UsageCollectorClient::register_metric_type` (SDK trait); both surfaces accept a resolved `cpt-cf-usage-collector-entity-security-context` (REST: `Extension<SecurityContext>` populated by ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK: `&SecurityContext` argument) and W3C audit-correlation context, and both converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-register-metric-submit`
2. [ ] - `p1` - Delegate to the gateway Metric Catalog service (the REST handler and the SDK trait impl share the same domain service entry point), passing the inbound `cpt-cf-usage-collector-entity-security-context` and the register-operation payload (`gts_id`, `metadata_fields`) - `inst-register-metric-service-call`
3. [ ] - `p1` - Inside the gateway service, invoke `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the register-operation attribution tuple and receive (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) - `inst-register-metric-pdp`
4. [ ] - `p1` - **IF** the PDP decision is deny **RETURN** the propagated platform-authorization error envelope (HTTP `403` per the yaml `Problem` response shape) - `inst-register-metric-pdp-deny`
5. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation` against the submitted payload (covering `gts_id` form, `gts_id` kind-prefix membership in the two reserved kind base type ids, and `metadata_fields` shape — array of unique non-empty strings) - `inst-register-metric-validate-shape`
6. [ ] - `p1` - **IF** the shape-validation algorithm reports invalid **RETURN** the actionable validation envelope (HTTP `400`) — `InvalidKindPrefix` (`context.reason="invalid_kind_prefix"`) when the kind-prefix check fails, or the fields-validation envelope when `metadata_fields` is malformed — per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` before any plugin dispatch - `inst-register-metric-invalid-shape`
7. [ ] - `p1` - **TRY** dispatch the catalog write through `cpt-cf-usage-collector-contract-storage-plugin` carrying (`gts_id`, `metadata_fields`); the plugin persists the new row in `metric_catalog` (PK `gts_id`) inside a transaction with `metadata_fields` stored as a typed array of strings per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-register-metric-spi-insert`
8. [ ] - `p1` - **CATCH** plugin SPI error - `inst-register-metric-spi-catch`
   1. [ ] - `p1` - **IF** the error is `MetricAlreadyExists` (the plugin's unique constraint on `gts_id` fired) **RETURN** the actionable conflict error envelope (HTTP `409`, `context.reason="metric_already_exists"`) without mutating the L1 cache - `inst-register-metric-duplicate`
   2. [ ] - `p1` - **ELSE** (transport / availability / persistence error from the plugin — the gear is unready whenever the bound plugin is unavailable, per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`) **RETURN** the propagated platform-error envelope (the L1 cache remains unchanged) - `inst-register-metric-spi-fail`
9. [ ] - `p1` - Synchronously refresh the gateway L1 cache for the newly persisted `gts_id` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (cache invalidation is load-bearing because the L1 cache sits on the ingest hot path) - `inst-register-metric-update-index`
10. [ ] - `p1` - **RETURN** HTTP `201` with the canonical `MetricTypeRecord` resource body and `Location: /usage-collector/v1/metric-types/{gts_id}` per `usage-collector-v1.yaml` (or, on the SDK path, return `Ok(MetricTypeRecord)` to the trait caller), propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` - `inst-register-metric-return`

**Acceptance Scenarios (Given-When-Then)**:

- **Given** the catalog has no row for the proposed `gts_id`, **when** an operator calls `POST /usage-collector/v1/metric-types` with `gts_id` ending `~` and beginning with one of the two reserved kind base type id prefixes, plus a valid closed `metadata_fields: array<string>`, **then** the gateway runs PDP and shape-validation (both pass), the plugin persists the new row in `metric_catalog` with the `metadata_fields` array column, the gateway L1 cache is refreshed for the new `gts_id`, and HTTP `201` is returned with the canonical `MetricTypeRecord` resource and a `Location` header.
- **Given** the catalog already contains a row keyed by the submitted `gts_id`, **when** an operator calls `POST /usage-collector/v1/metric-types` with the same `gts_id`, **then** the plugin's unique constraint on `gts_id` fires, the gateway returns HTTP `409` `MetricAlreadyExists` with `context.gts_id` carrying the offending identifier, no row mutation occurs, and the L1 cache is unchanged.

### Delete Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:

- A platform operator submits the delete request via either `DELETE /usage-collector/v1/metric-types/{gts_id}` or the SDK trait method `UsageCollectorClient::delete_metric_type` (both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`); the gateway Metric Catalog service authorizes the delete via the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, dispatches the catalog delete through `cpt-cf-usage-collector-contract-storage-plugin`, the plugin's in-database `ON DELETE RESTRICT` foreign key from `usage_records.gts_id` to `metric_catalog(gts_id)` confirms zero references and the plugin removes the row inside the same transaction, the gateway L1 cache entry is synchronously evicted, and `204 No Content` (REST) or `Ok(())` (SDK) is returned.

**Error Scenarios**:

- Request arrives without a resolved `SecurityContext` (gateway middleware rejected upstream) — the canonical `Unauthenticated` `Problem` envelope is returned by the gateway; no catalog mutation occurs.
- PDP denies the delete operation — propagated platform-authorization error envelope from `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`).
- Metric `gts_id` not present in the plugin's `metric_catalog` table — the plugin surfaces `MetricNotFound` and the gateway returns the actionable not-found error envelope (HTTP `404`).
- Plugin's FK rejects the delete because at least one `usage_records` row references the target `gts_id` — the plugin surfaces `MetricReferenced` carrying the `gts_id` and a sample reference count, and the gateway returns HTTP `409` with the same payload (referential-delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`).
- Plugin transport or persistence failure — propagated as a deterministic platform error envelope; no L1 cache mutation.

**Steps** (the referential-delete protocol per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`):

1. [ ] - `p1` - Operator submits the delete request via either `DELETE /usage-collector/v1/metric-types/{gts_id}` or `UsageCollectorClient::delete_metric_type` (SDK trait); both surfaces accept a resolved `cpt-cf-usage-collector-entity-security-context` and W3C audit-correlation context, both converge on the same gateway domain service, and the gateway service invokes `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the delete-operation attribution tuple (including the target Metric's `gts_id`); **IF** the PDP decision is deny **RETURN** the propagated platform-authorization error envelope (HTTP `403`) - `inst-delete-metric-pdp-authorize`
2. [ ] - `p1` - Dispatch the catalog delete through `cpt-cf-usage-collector-contract-storage-plugin` carrying the target `gts_id`; the plugin enters a transaction, the `ON DELETE RESTRICT` foreign key on `usage_records.gts_id` → `metric_catalog(gts_id)` either allows the row removal (zero references) or rejects the delete with a structured `MetricReferenced` error inside the same transaction per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-delete-metric-spi-dispatch`
3. [ ] - `p1` - **CATCH** plugin SPI error - `inst-delete-metric-spi-catch`
   1. [ ] - `p1` - **IF** the error is `MetricNotFound` (no `metric_catalog` row exists for the target `gts_id`) **RETURN** the actionable not-found error envelope (HTTP `404`) without any L1 cache mutation - `inst-delete-metric-not-found`
   2. [ ] - `p1` - **IF** the error is `MetricReferenced` (the plugin's FK rejected the delete because at least one `usage_records` row references the `gts_id`) **RETURN** HTTP `409` carrying the `MetricReferenced` payload (`context.gts_id` and `context.sample_ref_count` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) without any L1 cache mutation - `inst-delete-metric-referenced`
   3. [ ] - `p1` - **ELSE** (transport / availability / persistence error from the plugin) **RETURN** the propagated platform-error envelope (the L1 cache remains unchanged) - `inst-delete-metric-spi-fail`
4. [ ] - `p1` - On a successful delete, synchronously evict the corresponding entry from the gateway L1 cache (cache invalidation is load-bearing per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and **RETURN** HTTP `204 No Content` (REST) or `Ok(())` (SDK trait) per `usage-collector-v1.yaml`, propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` - `inst-delete-metric-spi-delete-return`

**Acceptance Scenarios (Given-When-Then)**:

- **Given** the plugin's `metric_catalog` table holds a row for `gts_id = G` and the plugin's `usage_records` table holds zero rows whose `gts_id = G`, **when** an operator calls `DELETE /usage-collector/v1/metric-types/{gts_id}` (or `UsageCollectorClient::delete_metric_type`) for `G`, **then** the gateway runs PDP (passes), the plugin's FK confirms zero references and removes the row inside the same transaction, the gateway L1 cache evicts the entry for `G`, HTTP `204 No Content` (REST) or `Ok(())` (SDK) is returned, and no other row in either table is mutated.
- **Given** the plugin's `metric_catalog` table holds a row for `gts_id = G` and the plugin's `usage_records` table holds at least one row whose `gts_id = G`, **when** an operator calls `DELETE /usage-collector/v1/metric-types/{gts_id}` (or `UsageCollectorClient::delete_metric_type`) for `G`, **then** the plugin's `ON DELETE RESTRICT` foreign key rejects the delete inside the same transaction and surfaces `MetricReferenced` to the gateway, the gateway returns HTTP `409` with a `Problem` envelope whose `context` carries `gts_id = G` and `sample_ref_count`, no `metric_catalog` row is removed, no `usage_records` row is mutated, and the gateway L1 cache is unchanged.

### List Metrics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-metric-lifecycle-list-metrics`

**Actor**: `cpt-cf-usage-collector-actor-platform-developer`

**Success Scenarios**:

- A platform developer (any authorized REST caller, or any in-process SDK consumer) submits the list request via either `GET /usage-collector/v1/metric-types` with optional `limit` and `cursor` paging parameters or `UsageCollectorClient::list_metric_types` (SDK trait) with a `PageParams { cursor, limit }` argument; both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, the gateway service authorizes the read via the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, the page is composed from a paginated catalog read dispatched through `cpt-cf-usage-collector-contract-storage-plugin` against the plugin's `metric_catalog` table, and a `MetricPage` is returned per `usage-collector-v1.yaml`.

**Error Scenarios**:

- Request arrives without a resolved `SecurityContext` (gateway middleware rejected upstream) — the canonical `Unauthenticated` `Problem` envelope is returned by the gateway.
- PDP denies the read operation — propagated platform-authorization error envelope from `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`).
- Plugin SPI transport or persistence failure on the paginated catalog read — propagated as a deterministic platform error envelope; the gear is unready whenever the bound plugin is unavailable per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`.

**Steps**:

1. [ ] - `p1` - Caller submits the list request via either `GET /usage-collector/v1/metric-types` with optional `limit` and `cursor` paging parameters per the yaml schema or `UsageCollectorClient::list_metric_types(ctx, PageParams { cursor, limit })` (SDK trait); both surfaces accept a resolved `cpt-cf-usage-collector-entity-security-context` and W3C audit-correlation context - `inst-list-metrics-submit`
2. [ ] - `p1` - Delegate to the gateway Metric Catalog service (the REST handler and the SDK trait impl share the same domain service entry point), passing the inbound `cpt-cf-usage-collector-entity-security-context` and the paging parameters - `inst-list-metrics-service-call`
3. [ ] - `p1` - Inside the gateway service, invoke `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the read-operation attribution tuple and receive (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) - `inst-list-metrics-pdp`
4. [ ] - `p1` - **IF** the PDP decision is deny **RETURN** the propagated platform-authorization error envelope (HTTP `403`) - `inst-list-metrics-pdp-deny`
5. [ ] - `p1` - Dispatch the paginated catalog read through `cpt-cf-usage-collector-contract-storage-plugin` against the plugin's `metric_catalog` table and compose the requested page from the returned rows per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-list-metrics-plugin-read`
6. [ ] - `p1` - **RETURN** HTTP `200` with the populated `MetricPage` (next-cursor included per `usage-collector-v1.yaml`) on the REST path, or `Ok(Page<Metric>)` on the SDK path, propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` - `inst-list-metrics-return`

### Read Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-metric-lifecycle-get-metric`

**Actor**: `cpt-cf-usage-collector-actor-platform-developer`

**Success Scenarios**:

- A platform developer (any authorized REST caller, or any in-process SDK consumer) submits the get request via either `GET /usage-collector/v1/metric-types/{gts_id}` or `UsageCollectorClient::read_metric_type(ctx, gts_id)` (SDK trait); both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, the gateway service authorizes the read via the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`, a catalog get is dispatched through `cpt-cf-usage-collector-contract-storage-plugin` (served from the gateway L1 cache when warm and refreshed from the plugin SoR on miss per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and the canonical `MetricTypeRecord` resource is returned per `usage-collector-v1.yaml`.

**Error Scenarios**:

- Request arrives without a resolved `SecurityContext` (gateway middleware rejected upstream) — the canonical `Unauthenticated` `Problem` envelope is returned by the gateway.
- PDP denies the read operation — propagated platform-authorization error envelope from `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`).
- The requested `gts_id` is absent from the `metric_catalog` table — actionable not-found error envelope (HTTP `404`) is returned.
- Plugin SPI transport or persistence failure on the catalog get dispatch — propagated as a deterministic platform error envelope; the gear is unready whenever the bound plugin is unavailable per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`.

**Steps**:

1. [ ] - `p1` - Caller submits the get request via either `GET /usage-collector/v1/metric-types/{gts_id}` or `UsageCollectorClient::read_metric_type(ctx, gts_id)` (SDK trait); both surfaces accept a resolved `cpt-cf-usage-collector-entity-security-context` and W3C audit-correlation context - `inst-get-metric-submit`
2. [ ] - `p1` - Delegate to the gateway Metric Catalog service (the REST handler and the SDK trait impl share the same domain service entry point), passing the inbound `cpt-cf-usage-collector-entity-security-context` and the target `gts_id` - `inst-get-metric-service-call`
3. [ ] - `p1` - Inside the gateway service, invoke `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the read-operation attribution tuple (including the target `gts_id`) and receive (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) - `inst-get-metric-pdp`
4. [ ] - `p1` - **IF** the PDP decision is deny **RETURN** the propagated platform-authorization error envelope (HTTP `403`) - `inst-get-metric-pdp-deny`
5. [ ] - `p1` - Dispatch the catalog get through `cpt-cf-usage-collector-contract-storage-plugin` for the supplied `gts_id`; the gateway L1 cache serves a warm hit when present and falls through to the plugin SoR on miss per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-get-metric-repo-find-by-id`
6. [ ] - `p1` - **IF** the plugin surfaces `MetricNotFound { gts_id }` **RETURN** the actionable not-found error envelope (HTTP `404`) - `inst-get-metric-not-found`
7. [ ] - `p1` - **RETURN** HTTP `200` with the canonical `MetricTypeRecord` resource (`gts_id`, `metadata_fields`, `created_at`; `MetricKind` is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` and is NOT a separate response field) per `usage-collector-v1.yaml` (or `Ok(MetricTypeRecord)` on the SDK path), propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` - `inst-get-metric-return`

## 3. Processes / Business Logic (CDSL)

### Metric Shape Validation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`

**Input**: the proposed Metric definition payload from `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric` (`gts_id`, `metadata_fields` per the `MetricTypeRegisterRequest` schema in `usage-collector-v1.yaml`).

**Output**: `valid` (the payload is structurally and semantically well-formed for plugin dispatch), or a structured validation envelope — `InvalidKindPrefix` (`context.reason="invalid_kind_prefix"`) when the `gts_id` prefix check fails, or a fields-validation envelope citing the offending `/metadata_fields` index when the array is malformed — per the `Problem` shape in `usage-collector-v1.yaml` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. This algorithm performs structural validation only — duplicate-`gts_id` detection is owned by the plugin's unique constraint on `metric_catalog(gts_id)` (surfaced as `MetricAlreadyExists`) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, NOT by this algorithm. The validated fields are `gts_id` (must end `~` and begin with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`) and `metadata_fields` (must be an array of unique non-empty strings).

**Steps**:

1. [ ] - `p1` - Parse and normalize the proposed Metric type definition payload into (`gts_id`, `metadata_fields`) without invoking any plugin SPI capability - `inst-algo-shape-parse`
2. [ ] - `p1` - **IF** the `gts_id` field is missing or empty **RETURN** the structured validation envelope with `context.reason="missing_gts_id"` and `instance_path="/gts_id"` - `inst-algo-shape-missing-gts-id`
3. [ ] - `p1` - **IF** the `gts_id` field does not parse as a GTS type id (must end `~` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) **RETURN** the structured validation envelope with `context.reason="invalid_gts_id"` and `instance_path="/gts_id"` - `inst-algo-shape-invalid-gts-id`
4. [ ] - `p1` - **IF** the `gts_id` prefix is not one of the two reserved kind base type ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` **RETURN** the structured `InvalidKindPrefix` envelope with `context.reason="invalid_kind_prefix"` and `instance_path="/gts_id"` so `MetricKind` (counter / gauge) can be deterministically derived from the prefix - `inst-algo-shape-invalid-kind-prefix`
5. [ ] - `p1` - **IF** the `metadata_fields` field is missing, is not an array of strings, contains an empty string, or contains a duplicate, **RETURN** the structured fields-validation envelope with `context.reason="invalid_metadata_fields"` and `instance_path="/metadata_fields/{index}"` of the offending entry - `inst-algo-shape-invalid-metadata-fields`
6. [ ] - `p1` - **RETURN** `valid`; the payload is ready for plugin dispatch by `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric` - `inst-algo-shape-return-valid`

### Catalog Kind Lookup

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Input**: target Metric `gts_id` (supplied by the latency-critical ingestion path or the aggregation-query validation path).

**Output**: a shape descriptor carrying `kind` (derived from the `gts_id` prefix at lookup time, not stored in the cache) and `metadata_fields` (the closed `array<string>` of declared metadata key names) when the `gts_id` is present in the gateway L1 cache populated from the plugin SoR, or `not-found` when it is absent. This algorithm performs in-process lookups only against the gateway L1 cache — it MUST NOT dispatch the plugin SPI on the latency-critical ingestion path per `cpt-cf-usage-collector-component-metric-catalog`; the L1 cache is the load-bearing hot-path surface per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. The cache is populated at boot from a plugin catalog-list dispatch and synchronously refreshed on every successful register/delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; there is no asynchronous projection refresh.

**Consumers (forward reference only — NOT implemented in this feature)**: this process is consumed by §2.3 Usage Emission (the ingestion path that enforces `cpt-cf-usage-collector-fr-counter-semantics` and `cpt-cf-usage-collector-fr-gauge-semantics` kind invariants on every record) and §2.4 Usage Query (the aggregation-query validation path that enforces `cpt-cf-usage-collector-fr-metric-existence-and-kind` on every aggregated read). Those features own their own flows; this algorithm only publishes the read-side kind/shape descriptor.

**Steps**:

1. [ ] - `p1` - Read the target Metric `gts_id` from the calling pipeline without any DB or SPI dispatch - `inst-algo-kind-lookup-read-input`
2. [ ] - `p1` - Resolve the active gateway L1 cache populated from the plugin SoR (the write-through cache populated at boot from a plugin catalog-list dispatch and synchronously refreshed on every successful register/delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) - `inst-algo-kind-lookup-resolve-index`
3. [ ] - `p1` - **TRY** look the entry up by `gts_id` in the in-memory kind/shape index backed by the gateway L1 cache - `inst-algo-kind-lookup-index-lookup`
4. [ ] - `p1` - **IF** the in-memory kind/shape index has an entry for the supplied `gts_id` **RETURN** the (`kind`, `metadata_fields`) shape descriptor to the calling pipeline (`kind` is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` — counter ⇐ `gts.cf.core.usage.counter.v1~`, gauge ⇐ `gts.cf.core.usage.gauge.v1~` — and is computed at lookup time, not stored in the cache entry) - `inst-algo-kind-lookup-hit`
5. [ ] - `p1` - **ELSE** **RETURN** `not-found`; the calling pipeline surfaces the actionable error envelope owned by §2.3 or §2.4 (e.g. `UnknownMetric` per `cpt-cf-usage-collector-fr-metric-existence-and-kind`) - `inst-algo-kind-lookup-miss`

### Gateway L1 Metadata Validation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-metric-lifecycle-l1-metadata-validation`

**Input**: target `gts_id` (resolved from the incoming usage row) and the candidate `metadata` JSON object as supplied by the ingest call.

**Output**: `valid` (every key in the candidate `metadata` is a declared member of the metric's `metadata_fields`; all values are conveyed as `String`), or a structured `UnknownMetadataKey` envelope citing the offending key in `context.key` and `instance_path` (the offending node in the candidate metadata) per the `Problem` shape in `usage-collector-v1.yaml`. The lookup of `metadata_fields` MUST come from the gateway L1 cache — the cache is load-bearing on the ingest hot path per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (a cache bug becomes an ingest-time data-quality regression, not a perf-only issue). This validation is **closed-shape**: there is no free-form remainder and undeclared keys are never silently preserved.

**Steps**:

1. [ ] - `p1` - Resolve the `metadata_fields` set for the target `gts_id` via `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`; **IF** the lookup returns `not-found` **RETURN** the `MetricNotFound` envelope (HTTP `404`) so the ingest path can reject the row before any plugin write dispatch - `inst-algo-l1-validate-resolve-fields`
2. [ ] - `p1` - Validate the candidate `metadata` object against the resolved `metadata_fields` by checking that every key in the candidate is a member of the closed `metadata_fields` array per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; values are accepted as `String` end-to-end - `inst-algo-l1-validate-closed-shape`
3. [ ] - `p1` - **IF** the candidate carries any key that is not a member of `metadata_fields` **RETURN** the `UnknownMetadataKey` envelope (HTTP `400`, `context.reason="unknown_metadata_key"`) citing `context.key` (the first offending key) and `instance_path` (e.g. `/metadata/extra_tag`) so the caller can pinpoint the offending key - `inst-algo-l1-validate-reject`
4. [ ] - `p1` - **RETURN** `valid`; the candidate metadata may now flow to the plugin's usage-record insert path - `inst-algo-l1-validate-return-valid`

**Acceptance Scenarios (Given-When-Then)**:

- **Given** a registered metric `T` whose `metadata_fields = ["region"]`, **when** any caller submits a usage row carrying `gts_id = T` and `metadata = { "region": "eu-west-1" }`, **then** the gateway resolves the declared keys from the L1 cache, verifies that every candidate key is a member of `metadata_fields` (passes), and accepts the row for plugin dispatch (closed-shape validation per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`).
- **Given** a registered metric `T` whose `metadata_fields = ["region"]`, **when** any caller submits a usage row carrying `gts_id = T` and `metadata = { "region": "eu-west-1", "extra_tag": "x" }`, **then** the gateway returns HTTP `400` with an `UnknownMetadataKey` payload citing `context.key = "extra_tag"` and `instance_path = "/metadata/extra_tag"`, no plugin write dispatch occurs, and no `usage_records` row is mutated.

## 4. States (CDSL)

### Metric Registration Lifecycle State Machine

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-state-metric-lifecycle-metric-registration-lifecycle`

**States**: `not-registered`, `registered`

**Initial State**: `not-registered`

**Scope note**: This state machine models the lifecycle of a `gts_id` in the unified metric catalog (`cpt-cf-usage-collector-dbtable-metric-catalog`) persisted in the plugin's database per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. Registration and deletion go through the REST surface or the SDK trait surface; both surfaces converge on the same gateway domain service which dispatches catalog writes through `cpt-cf-usage-collector-contract-storage-plugin`.

**Transitions**:

1. [ ] - `p1` - **FROM** `not-registered` **TO** `registered` **WHEN** `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric` completes successfully — the plugin SPI catalog-insert persisted the new row in the plugin's `metric_catalog` table inside a transaction (mirrors `inst-register-metric-spi-insert`) and the gateway L1 cache was synchronously refreshed for the new `gts_id` (mirrors `inst-register-metric-update-index`) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-state-metric-lifecycle-registered`
2. [ ] - `p1` - **FROM** `registered` **TO** `not-registered` **WHEN** `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric` completes successfully — the plugin SPI catalog-delete removed the row from the plugin's `metric_catalog` table after the `ON DELETE RESTRICT` foreign key confirmed zero `usage_records` references, and the gateway L1 cache was synchronously evicted (mirrors `inst-delete-metric-spi-delete-return`) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; the transition is REJECTED while the plugin's FK reports at least one `usage_records` row references the `gts_id` (the plugin returns `MetricReferenced` and the gateway returns HTTP `409` per `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-deletion`), and the state remains `registered` - `inst-state-metric-lifecycle-not-registered`

## 5. Definitions of Done

### FR: Metric Registration

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-registration`

The system **MUST** accept a Metric type definition (`gts_id`, `metadata_fields`) on `POST /usage-collector/v1/metric-types` or via the SDK trait method `UsageCollectorClient::register_metric_type` (both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), validate its shape via `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation` before any plugin dispatch (the validation rejects any `gts_id` that does not begin with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~` with error name `invalid_kind_prefix`, and any malformed `metadata_fields` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), persist the new row in `cpt-cf-usage-collector-dbtable-metric-catalog` (PK `gts_id` plus the typed `metadata_fields` array of strings; `MetricKind` is derived from the `gts_id` prefix and is not a separate column) by dispatching the catalog write through `cpt-cf-usage-collector-contract-storage-plugin` inside a transaction per `cpt-cf-usage-collector-seq-register-metric` and `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, synchronously refresh the gateway L1 cache for the new `gts_id` on success per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, and surface a deterministic `MetricAlreadyExists` (HTTP `409`) envelope when the plugin's unique constraint on `gts_id` fires so retried submissions of the same `gts_id` never produce silent duplication or partial catalog mutation.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`
- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`

**Constraints**: `cpt-cf-usage-collector-fr-metric-registration`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### FR: Metric Deletion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-deletion`

The system **MUST** implement `DELETE /usage-collector/v1/metric-types/{gts_id}` and the SDK trait method `UsageCollectorClient::delete_metric_type` (both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) per the referential-delete protocol of §"Delete Metric" — PDP authorize, dispatch the catalog delete through `cpt-cf-usage-collector-contract-storage-plugin` so that: a `MetricNotFound` SPI error ⇒ HTTP `404`; a `MetricReferenced` SPI error (the plugin's `ON DELETE RESTRICT` FK on `usage_records.gts_id` rejected the delete because at least one row references the `gts_id`) ⇒ HTTP `409` with the `MetricReferenced` payload (`gts_id` + sample reference count); a successful row removal ⇒ a synchronous eviction of the gateway L1 cache entry per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` followed by `204 No Content` per `cpt-cf-usage-collector-seq-delete-metric`. The implementation MUST NOT introduce a tombstone, a state column, or a second transaction.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Constraints**: `cpt-cf-usage-collector-fr-metric-deletion`

**Touches**:

- API: `DELETE /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`

### FR: Metric Existence and Kind

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-existence-and-kind`

The system **MUST** publish a read-side lookup, `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`, that resolves a target `gts_id` against the gateway L1 cache populated from the plugin SoR — returning the shape descriptor (`kind`, `metadata_fields`) when present (where `kind` is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` and `metadata_fields` is the closed `array<string>` of declared key names) and `not-found` otherwise — so the §2.3 Usage Emission ingestion path and the §2.4 Usage Query aggregation-query validation path can enforce Metric existence, Metric Kind, and the closed `metadata_fields` invariant without round-tripping the plugin SPI on the latency-critical path per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. The gateway L1 cache is a write-through cache populated at boot from a plugin catalog-list dispatch and synchronously refreshed on every successful register/delete; there is no asynchronous projection refresh and no cold-state fallback. The cache is load-bearing because gateway L1 metadata validation runs on the ingest hot path per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-fr-metric-existence-and-kind`

**Touches**:

- API: `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### FR: Counter Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-fr-counter-semantics`

The system **MUST** classify a Metric whose registered `gts_id` begins with the reserved counter prefix `gts.cf.core.usage.counter.v1~` as a non-negative delta accumulation Metric per `cpt-cf-usage-collector-entity-metric-kind` (MetricKind derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) and publish the `counter` classification through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` so the §2.3 Usage Emission ingestion path can enforce non-negative-delta invariants on every record without re-deriving the Metric Kind locally.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-fr-counter-semantics`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### FR: Gauge Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-fr-gauge-semantics`

The system **MUST** classify a Metric whose registered `gts_id` begins with the reserved gauge prefix `gts.cf.core.usage.gauge.v1~` as a point-in-time Metric stored as-is per `cpt-cf-usage-collector-entity-metric-kind` (MetricKind derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) and publish the `gauge` classification through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` so the §2.3 Usage Emission ingestion path can accept gauge values without delta-accumulation rewriting.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-fr-gauge-semantics`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### NFR: Authorization

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-authorization`

The system **MUST** authorize every metric-lifecycle call — `POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`, and their SDK trait counterparts `UsageCollectorClient::register_metric_type` / `delete_metric_type` / `list_metric_types` / `read_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` — by accepting a resolved `cpt-cf-usage-collector-entity-security-context` (REST: `Extension<SecurityContext>` populated by ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK: the `&SecurityContext` argument supplied by the in-process caller) at the surface boundary, delegating to the shared gateway Metric Catalog service, and invoking the PDP permit/deny decision through the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) inside that gateway service, never deriving authorization locally and never serving a cached PDP decision; absence of a resolved `SecurityContext` on the REST path surfaces the canonical `Unauthenticated` `Problem` envelope (the gateway rejected the call upstream — the collector never synthesizes identity), the SDK trait impl returns the corresponding `UsageCollectorError` on the in-process path, and PDP deny outcomes or PDP-resolver-unavailable conditions MUST be propagated as platform-authorization error envelopes per `usage-collector-v1.yaml` with no catalog mutation.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`

**Constraints**: `cpt-cf-usage-collector-nfr-authorization`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- Component: `cpt-cf-usage-collector-component-metric-catalog`
- Entities: `SecurityContext`, `PdpDecision`, `PdpConstraint`

### NFR: Availability

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`

The system **MUST** keep the read endpoints `GET /usage-collector/v1/metric-types` and `GET /usage-collector/v1/metric-types/{gts_id}` (and their SDK trait counterparts `UsageCollectorClient::list_metric_types` / `read_metric_type`) available whenever the bound storage plugin is available — catalog reads dispatch through `cpt-cf-usage-collector-contract-storage-plugin` against the plugin's `metric_catalog` table — and MUST treat the bound plugin as a hard gear-readiness dependency on parity with every other gateway-resident system gear per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`: when the plugin is unavailable the gear declines readiness, the read endpoints surface deterministic platform-error envelopes per `usage-collector-v1.yaml`, and there is no retain-prior-projection fallback. The gateway L1 cache serves warm reads but is refreshed from the plugin SoR on cache miss and on every register/delete; the SoR remains the plugin per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-list-metrics`
- `cpt-cf-usage-collector-flow-metric-lifecycle-get-metric`

**Constraints**: `cpt-cf-usage-collector-nfr-availability`

**Touches**:

- API: `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`

### Principle: Kind Enforcement

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-principle-kind-enforcement`

The system **MUST** enforce Metric Kind invariants at the catalog boundary: every Metric registration validates that the supplied `gts_id` begins with one of the two reserved kind base type id prefixes (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) before any repo dispatch and rejects bad prefixes with the `invalid_kind_prefix` error name, and every read-side consumer obtains the authoritative `kind` for a `gts_id` exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` (which derives `kind` from the prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) so that ingestion (`cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-gauge-semantics`) and aggregation-query validation never re-derive the Metric Kind locally.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-principle-kind-enforcement`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Constraint: No Business Logic

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-constraint-no-business-logic`

The system **MUST** keep the Metric Catalog free of **per-Metric business-rule columns**: the durable row shape in `cpt-cf-usage-collector-dbtable-metric-catalog` carries `gts_id` (PK), `metadata_fields` (closed `array<string>` of declared metadata key names), and `created_at` with no tenant scoping, and **MUST NOT** introduce accounting / billing / per-Metric value-rule columns. `MetricKind` (`counter` / `gauge`) is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` and is NOT a stored column. Carrying a closed `metadata_fields` declaration is **metadata-shape typing**, not business logic — it constrains payload shape per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. Every per-Metric business rule (counter/gauge value enforcement on the ingestion path, accounting interpretation, billing transforms) remains owned by source gears and downstream consumers — never by the catalog.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`
- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`

**Constraints**: `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Component: Metric Catalog

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-component-metric-catalog`

The system **MUST** realize `cpt-cf-usage-collector-component-metric-catalog` as the sole owner of the unified Metric catalog — a single catalog persisted in the plugin's `metric_catalog` table reached through `cpt-cf-usage-collector-contract-storage-plugin` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` — plus the Metric lifecycle entry points (register, list, get, delete) reachable via REST and the SDK trait `UsageCollectorClient`. The component maintains a gateway L1 cache as a write-through cache for the hot ingestion path: the cache is populated from the plugin SoR at boot via a catalog-list SPI dispatch and synchronously refreshed on every successful register/delete — there is no TTL, no reconciliation tick, and no asynchronous projection-refresh algorithm. Catalog reads and writes flow through the Plugin SPI per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; the gateway L1 cache is the load-bearing hot-path surface (gateway L1 metadata validation runs on the ingest hot path). On every catalog delete it MUST surface the plugin's `MetricReferenced` SPI error as HTTP `409` with the structured payload (`gts_id` + sample reference count) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`
- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`
- `cpt-cf-usage-collector-flow-metric-lifecycle-list-metrics`
- `cpt-cf-usage-collector-flow-metric-lifecycle-get-metric`

**Constraints**: `cpt-cf-usage-collector-component-metric-catalog`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Sequence: Register Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-seq-register-metric`

The system **MUST** implement the `cpt-cf-usage-collector-seq-register-metric` sequence end-to-end on both the REST and the SDK trait surfaces (the REST handler accepts `Extension<SecurityContext>` from ToolKit gateway middleware and the SDK trait method `UsageCollectorClient::register_metric_type` accepts `&SecurityContext` directly; both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) → gateway Metric Catalog service (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver` + shape validation including the kind-prefix check on `gts_id` and the closed `metadata_fields` array validation per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) → catalog-insert dispatched through `cpt-cf-usage-collector-contract-storage-plugin` against the plugin's `metric_catalog` table inside a transaction (the unique constraint on `gts_id` enforces deduplication, surfaced as `MetricAlreadyExists`), with PDP denial, invalid kind prefix (`InvalidKindPrefix`), malformed `metadata_fields`, and unique-constraint duplicate outcomes rejecting the call before or at the plugin boundary and the successful path returning the canonical `MetricTypeRecord` resource with a `Location` header on the REST path or `Ok(MetricTypeRecord)` on the SDK path per `usage-collector-v1.yaml`.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`
- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`

**Constraints**: `cpt-cf-usage-collector-seq-register-metric`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Sequence: Delete Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-seq-delete-metric`

The system **MUST** implement the `cpt-cf-usage-collector-seq-delete-metric` sequence end-to-end on both the REST and the SDK trait surfaces (`DELETE /usage-collector/v1/metric-types/{gts_id}` and `UsageCollectorClient::delete_metric_type`; both surfaces converge on the same gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) per the referential-delete protocol: PDP authorize → dispatch the catalog delete through `cpt-cf-usage-collector-contract-storage-plugin` → `MetricNotFound` SPI error ⇒ HTTP `404` → `MetricReferenced` SPI error (the plugin's `ON DELETE RESTRICT` FK on `usage_records.gts_id` rejected the delete) ⇒ HTTP `409` with the `MetricReferenced` payload (`gts_id` + sample reference count) → successful plugin row removal + synchronous eviction of the gateway L1 cache entry → `204` (REST) / `Ok(())` (SDK). The implementation MUST NOT introduce a tombstone, a state column, or a second transaction.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Constraints**: `cpt-cf-usage-collector-seq-delete-metric`

**Touches**:

- API: `DELETE /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`

### Data: Metric Catalog Table

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-dbtable-metric-catalog`

The system **MUST** persist Metric records in `cpt-cf-usage-collector-dbtable-metric-catalog` — the `metric_catalog` table owned by the bound storage plugin per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` — with the DESIGN-mandated row shape: `gts_id` (PK, NOT NULL, ends `~`, MUST begin with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`), `metadata_fields` (typed array of strings — the closed declared metadata key set per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and `created_at` — with no tenant scoping (Metrics are platform-global) and the referential rule that every `usage_records.gts_id` MUST resolve to a row here via the in-database `ON DELETE RESTRICT` foreign key (so unsafe deletes surface as `MetricReferenced` SPI errors per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`). `MetricKind` is derived from the `gts_id` prefix and is NOT stored as a separate column. The plugin author chooses the concrete in-plugin column type and index strategy for `gts_id` (DESIGN §3.7 explicitly leaves this to the plugin author). Catalog mutations dispatch through `cpt-cf-usage-collector-contract-storage-plugin`.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`
- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Constraints**: `cpt-cf-usage-collector-dbtable-metric-catalog`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Entity: Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-entity-metric`

The system **MUST** treat `cpt-cf-usage-collector-entity-metric` as the platform-global, identity-bearing Metric definition keyed by `gts_id` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (the human-readable `gts_id` is a unique `MetricGtsId` newtype whose `Deserialize` impl rejects any string that does not begin with one of the two reserved kind base type id prefixes `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`). The entity is described by its closed `metadata_fields: array<string>` (declared metadata key names; all values typed as `String` end-to-end). `MetricKind` is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` and is NOT a separate registration field, trait, or catalog column. The entity's identity (`gts_id`) MUST be unique deployment-wide, MUST NOT carry tenant scoping, MUST be validated through the `MetricGtsId` newtype boundary on the REST ingress path, and MUST be re-registrable after a successful clean delete; idempotency-key collisions are not a concern because idempotency-keyed dedup is per (tenant_id, gts_id, idempotency_key) and any surviving `usage_records` rows are tolerated by the catalog deletion.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`
- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Constraints**: `cpt-cf-usage-collector-entity-metric`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`, `DELETE /usage-collector/v1/metric-types/{gts_id}`, `GET /usage-collector/v1/metric-types`, `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`

### Entity: Metric Kind

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-entity-metric-kind`

The system **MUST** treat `cpt-cf-usage-collector-entity-metric-kind` as a closed enumeration of accumulation-semantics classifiers — `counter` (non-negative delta accumulation) and `gauge` (point-in-time, stored as-is) per DESIGN §3.1 (Core Entities table, MetricKind row). Per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, `MetricKind` is NOT a standalone first-class column on the catalog row and is NOT carried as a separate trait; it is **derived** from the leftmost `~`-separated segment of the metric's `gts_id` against the two reserved kind base type ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`). The system **MUST** reject any Metric registration whose `gts_id` does not begin with one of those two reserved prefixes through `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation` (error name `invalid_kind_prefix`) before any repo dispatch.

**Implements**:

- `cpt-cf-usage-collector-algo-metric-lifecycle-metric-shape-validation`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-entity-metric-kind`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `MetricKind`

### API: POST /usage-collector/v1/metric-types

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-api-post-metrics`

The system **MUST** expose `POST /usage-collector/v1/metric-types` as the REST Metric-registration entry point per `usage-collector-v1.yaml`, accepting the `MetricTypeRegisterRequest` payload (`gts_id`, `metadata_fields`) and returning the canonical `MetricTypeRecord` resource on success with a `Location: /usage-collector/v1/metric-types/{gts_id}` header, surfacing deterministic `Problem` envelopes for `403` PDP denial, `400 InvalidKindPrefix` (`context.reason="invalid_kind_prefix"`) when `gts_id` does not begin with one of the two reserved kind base type id prefixes, `400` fields-validation envelopes for malformed `metadata_fields`, `409 MetricAlreadyExists` conflicts when the plugin's unique constraint on `gts_id` fires, and propagated platform-error envelopes for upstream resolver or plugin SPI failures. The same operation is reachable via the SDK trait method `UsageCollectorClient::register_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; both surfaces converge on the same gateway domain service.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-register-metric`

**Constraints**: `cpt-cf-usage-collector-fr-metric-registration`

**Touches**:

- API: `POST /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### API: DELETE /usage-collector/v1/metric-types/{gts_id}

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-api-delete-metric`

The system **MUST** expose `DELETE /usage-collector/v1/metric-types/{gts_id}` as the REST Metric-deletion entry point per `usage-collector-v1.yaml`, returning `204 No Content` on a clean delete and deterministic `Problem` envelopes for `403` PDP denial, `404 MetricNotFound` when the plugin reports no such `gts_id`, `409 MetricReferenced` (with `context.gts_id` and `context.sample_ref_count`) when the plugin's `ON DELETE RESTRICT` foreign key on `usage_records.gts_id` rejects the delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, and propagated platform-error envelopes for upstream resolver or plugin SPI failures — never mutating the plugin's `metric_catalog` table or the gateway L1 cache on any non-`Deleted` outcome. The same operation is reachable via the SDK trait method `UsageCollectorClient::delete_metric_type` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; both surfaces converge on the same gateway domain service.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-delete-metric`

**Constraints**: `cpt-cf-usage-collector-fr-metric-deletion`

**Touches**:

- API: `DELETE /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`

### API: GET /usage-collector/v1/metric-types

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-api-list-metrics`

The system **MUST** expose `GET /usage-collector/v1/metric-types` as the REST Metric-list entry point per `usage-collector-v1.yaml`, serving a paged `MetricPage` from a paginated catalog-list dispatched through `cpt-cf-usage-collector-contract-storage-plugin` against the plugin's `metric_catalog` table per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. The same operation is reachable via the SDK trait method `UsageCollectorClient::list_metric_types(ctx, PageParams { cursor, limit })`; both surfaces converge on the same gateway domain service.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-list-metrics`

**Constraints**: `cpt-cf-usage-collector-fr-metric-existence-and-kind`

**Touches**:

- API: `GET /usage-collector/v1/metric-types`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### API: GET /usage-collector/v1/metric-types/{gts_id}

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-metric-lifecycle-api-get-metric`

The system **MUST** expose `GET /usage-collector/v1/metric-types/{gts_id}` as the REST Metric-lookup entry point per `usage-collector-v1.yaml`, serving the canonical `MetricTypeRecord` resource (`gts_id`, `metadata_fields`, `created_at`; `MetricKind` is derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` and is NOT a separate response field) via a catalog-get dispatch through `cpt-cf-usage-collector-contract-storage-plugin` (served from the gateway L1 cache when warm and refreshed from the plugin SoR on miss per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and returning a `404 MetricNotFound` `Problem` envelope when the catalog has no entry for the supplied `gts_id`. The same operation is reachable via the SDK trait method `UsageCollectorClient::read_metric_type(ctx, gts_id)`; both surfaces converge on the same gateway domain service.

**Implements**:

- `cpt-cf-usage-collector-flow-metric-lifecycle-get-metric`

**Constraints**: `cpt-cf-usage-collector-fr-metric-existence-and-kind`

**Touches**:

- API: `GET /usage-collector/v1/metric-types/{gts_id}`
- DB: `cpt-cf-usage-collector-dbtable-metric-catalog`
- Entities: `Metric`, `MetricKind`

### Error Mapping: SPI → REST / SDK

Definitive mapping of metric-lifecycle outcomes from the storage plugin SPI surface (per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) and the gateway L1 validation surface (per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) onto the REST `Problem` envelope and the flat `UsageCollectorError` SDK variant.

| SPI / gateway outcome                   | HTTP status   | REST `Problem.context.reason` | SDK `UsageCollectorError` variant                                    |
| --------------------------------------- | ------------- | ----------------------------- | -------------------------------------------------------------------- |
| `MetricReferenced`                      | `409`         | `metric_referenced`           | `MetricReferenced`                                                   |
| `MetricNotFound`                        | `404`         | `metric_not_found`            | `MetricNotFound`                                                     |
| `MetricAlreadyExists`                   | `409`         | `metric_already_exists`       | `MetricAlreadyExists`                                                |
| `InvalidKindPrefix` (register)          | `400`         | `invalid_kind_prefix`         | `InvalidKindPrefix`                                                  |
| `UnknownMetadataKey` (ingest)           | `400`         | `unknown_metadata_key`        | `UnknownMetadataKey`                                                 |
| Malformed `metadata_fields` (register)  | `400`         | `invalid_metadata_fields`     | `InvalidArgument`                                                    |
| PDP deny                                | `403`         | (PDP-supplied reason)         | (per `cpt-cf-usage-collector-flow-foundation-pdp-authorize`)         |
| Plugin transport / availability failure | `503` / `5xx` | platform-error envelope       | (per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`) |

The `MetricReferenced` payload MUST carry `context.gts_id` and `context.sample_ref_count` so callers can identify the offending metric and decide whether to retract `usage_records` rows before retrying the delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`. The `InvalidKindPrefix` payload MUST carry `context.gts_id` and `instance_path="/gts_id"` so callers can pinpoint the offending identifier; the `UnknownMetadataKey` payload MUST carry `context.key` and `instance_path` (e.g. `/metadata/{key}`) so callers can pinpoint the offending key per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.

### §2.2-item → DoD-ID Coverage Matrix

Coverage of every DECOMPOSITION §2.2 catalog item:

| §2.2 Item                                             | Kind              | DoD ID                                                                     |
| ----------------------------------------------------- | ----------------- | -------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-fr-metric-registration`       | FR                | `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-registration`       |
| `cpt-cf-usage-collector-fr-metric-deletion`           | FR                | `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-deletion`           |
| `cpt-cf-usage-collector-fr-metric-existence-and-kind` | FR                | `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-existence-and-kind` |
| `cpt-cf-usage-collector-fr-counter-semantics`         | FR                | `cpt-cf-usage-collector-dod-metric-lifecycle-fr-counter-semantics`         |
| `cpt-cf-usage-collector-fr-gauge-semantics`           | FR                | `cpt-cf-usage-collector-dod-metric-lifecycle-fr-gauge-semantics`           |
| `cpt-cf-usage-collector-nfr-authorization`            | NFR               | `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-authorization`            |
| `cpt-cf-usage-collector-nfr-availability`             | NFR               | `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`             |
| `cpt-cf-usage-collector-principle-kind-enforcement`   | Principle         | `cpt-cf-usage-collector-dod-metric-lifecycle-principle-kind-enforcement`   |
| `cpt-cf-usage-collector-constraint-no-business-logic` | Design constraint | `cpt-cf-usage-collector-dod-metric-lifecycle-constraint-no-business-logic` |
| `cpt-cf-usage-collector-component-metric-catalog`     | Design component  | `cpt-cf-usage-collector-dod-metric-lifecycle-component-metric-catalog`     |
| `cpt-cf-usage-collector-seq-register-metric`          | Sequence          | `cpt-cf-usage-collector-dod-metric-lifecycle-seq-register-metric`          |
| `cpt-cf-usage-collector-seq-delete-metric`            | Sequence          | `cpt-cf-usage-collector-dod-metric-lifecycle-seq-delete-metric`            |
| `cpt-cf-usage-collector-dbtable-metric-catalog`       | Data              | `cpt-cf-usage-collector-dod-metric-lifecycle-dbtable-metric-catalog`       |
| `cpt-cf-usage-collector-entity-metric`                | Domain entity     | `cpt-cf-usage-collector-dod-metric-lifecycle-entity-metric`                |
| `cpt-cf-usage-collector-entity-metric-kind`           | Domain entity     | `cpt-cf-usage-collector-dod-metric-lifecycle-entity-metric-kind`           |
| `POST /usage-collector/v1/metric-types`               | API               | `cpt-cf-usage-collector-dod-metric-lifecycle-api-post-metrics`             |
| `DELETE /usage-collector/v1/metric-types/{gts_id}`    | API               | `cpt-cf-usage-collector-dod-metric-lifecycle-api-delete-metric`            |
| `GET /usage-collector/v1/metric-types`                | API               | `cpt-cf-usage-collector-dod-metric-lifecycle-api-list-metrics`             |
| `GET /usage-collector/v1/metric-types/{gts_id}`       | API               | `cpt-cf-usage-collector-dod-metric-lifecycle-api-get-metric`               |

Coverage totals: FR=5, NFR=2, Principle=1, Design constraint=1, Design component=1, Sequence=2, Data=1, Domain entity=2, API=4 — total 19 DoD entries, zero duplicates, zero §2.2 gaps. The DoD set covers every DECOMPOSITION §2.2 coverage item with exactly one DoD entry per item, and the closing matrix maps every §2.2 row to its DoD ID.

## 6. Acceptance Criteria

- [ ] `p1` - After a successful `POST /usage-collector/v1/metric-types` with a valid `MetricTypeRegisterRequest` (`gts_id`, `metadata_fields`), a subsequent `GET /usage-collector/v1/metric-types/{gts_id}` returns the canonical `MetricTypeRecord` resource whose `gts_id` and `metadata_fields` are byte-identical to the persisted row in `cpt-cf-usage-collector-dbtable-metric-catalog` (and whose derived `MetricKind` matches the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and a `GET /usage-collector/v1/metric-types` page includes that same row with the same field values (catalog round-trip correctness per `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-registration`).
- [ ] `p1` - Every `POST /usage-collector/v1/metric-types` / `DELETE /usage-collector/v1/metric-types/{gts_id}` REST call and every `UsageCollectorClient::register_metric_type` / `delete_metric_type` SDK trait call accepts a resolved `cpt-cf-usage-collector-entity-security-context` (REST: `Extension<SecurityContext>` populated by ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK: `&SecurityContext` argument) at the surface boundary and dispatches authorization through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`) before any plugin SPI dispatch — both surfaces converge on the shared gateway domain service per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; a PDP `deny` decision returns the platform-authorization error envelope (HTTP `403` on REST, `UsageCollectorError` on SDK) and leaves the durable `metric_catalog` row count and the gateway L1 cache cardinality unchanged (PDP gating per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-authorization`).
- [ ] `p1` - A `DELETE /usage-collector/v1/metric-types/{gts_id}` (or `UsageCollectorClient::delete_metric_type`) whose target metric has zero referencing `usage_records` rows removes the `cpt-cf-usage-collector-dbtable-metric-catalog` row via the plugin SPI catalog-delete dispatch and synchronously evicts the corresponding gateway L1 cache entry; on any non-`Deleted` outcome (`404 MetricNotFound`, `409 MetricReferenced` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, or any platform-error envelope) the plugin's `metric_catalog` row and the gateway L1 cache entry are both unchanged (referential-delete protocol per `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-deletion` and `cpt-cf-usage-collector-dod-metric-lifecycle-seq-delete-metric`).
- [ ] `p1` - A `POST /usage-collector/v1/metric-types` (or `UsageCollectorClient::register_metric_type`) whose supplied `gts_id` does not begin with one of the two reserved kind base type id prefixes (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) returns HTTP `400` with the structured `InvalidKindPrefix` envelope (REST `context.reason="invalid_kind_prefix"`) or the corresponding `UsageCollectorError::InvalidKindPrefix` variant (SDK) citing `context.gts_id` and `instance_path="/gts_id"`, before any plugin dispatch, and leaves the plugin's `metric_catalog` table and the gateway L1 cache unchanged (kind-prefix validation on register per `cpt-cf-usage-collector-dod-metric-lifecycle-principle-kind-enforcement` and `cpt-cf-usage-collector-dod-metric-lifecycle-entity-metric-kind`).
- [ ] `p1` - The §2.3 Usage Emission ingestion path and the §2.4 Usage Query aggregation-query validation path resolve a target `gts_id` to its shape descriptor (`kind` derived from the `gts_id` prefix, plus `metadata_fields` from the cached entry) exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`; the gateway L1 cache populated from the plugin SoR is the single source of truth on the latency-critical hot path (populated at boot from a plugin catalog-list dispatch and synchronously refreshed on every successful register/delete per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), and an absent entry surfaces `not-found` rather than triggering a fallback plugin SPI dispatch on the hot path (ingestion-path L1 cache consumption per `cpt-cf-usage-collector-dod-metric-lifecycle-fr-metric-existence-and-kind`).
- [ ] `p1` - `GET /usage-collector/v1/metric-types` / `UsageCollectorClient::list_metric_types` and `GET /usage-collector/v1/metric-types/{gts_id}` / `UsageCollectorClient::read_metric_type` are served via a paginated/single-row dispatch through `cpt-cf-usage-collector-contract-storage-plugin` against the unified metric catalog per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; the gear declines readiness when the bound plugin is unavailable (parity with every other gateway-resident system gear per `cpt-cf-usage-collector-dod-metric-lifecycle-nfr-availability`, `cpt-cf-usage-collector-dod-metric-lifecycle-api-list-metrics`, and `cpt-cf-usage-collector-dod-metric-lifecycle-api-get-metric`) — there is no retain-prior-projection fallback.
- [ ] `p1` - **Given** the catalog contains a registered metric `T` whose `metadata_fields = ["region"]`, **when** any caller submits a usage row carrying `gts_id = T` and `metadata = { "region": "eu-west-1" }`, **then** the gateway L1 validation algorithm resolves the declared keys from the L1 cache, verifies every candidate key is a member of `metadata_fields` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`, and accepts the row for plugin dispatch (gateway L1 success per `cpt-cf-usage-collector-algo-metric-lifecycle-l1-metadata-validation`).
- [ ] `p1` - **Given** the catalog contains a registered metric `T` whose `metadata_fields = ["region"]`, **when** any caller submits a usage row carrying `gts_id = T` and `metadata = { "region": "eu-west-1", "extra_tag": "x" }`, **then** the gateway L1 validation algorithm returns HTTP `400` with an `UnknownMetadataKey` payload citing `context.reason = "unknown_metadata_key"`, `context.key = "extra_tag"`, and `context.instance_path = "/metadata/extra_tag"`, no plugin dispatch occurs, and no `usage_records` row is mutated (gateway L1 rejection per `cpt-cf-usage-collector-algo-metric-lifecycle-l1-metadata-validation`).
- [ ] `p1` - **Given** the catalog contains a registered leaf metric `L` and the plugin's `usage_records` table holds at least one row whose `gts_id = L`, **when** an operator calls `DELETE /usage-collector/v1/metric-types/{gts_id}` (or `UsageCollectorClient::delete_metric_type`) for `L`, **then** the plugin's `ON DELETE RESTRICT` foreign key rejects the delete inside the same transaction and surfaces `MetricReferenced`, the gateway returns HTTP `409` with the structured `Problem` envelope carrying `context.gts_id = L` and `context.sample_ref_count`, no `metric_catalog` row is removed, no `usage_records` row is mutated, and the gateway L1 cache is unchanged (referential-delete rejection per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`).

## 7. Changelog

- **0.2.1 — 2026-06-02** — Cascaded the ADR-0012 2026-06-02 amendment (simplifications 5 and 6) across §1 References, §2 Register Metric flow, §3 Metric Shape Validation / Catalog Kind Lookup / Gateway L1 Metadata Validation algorithms, §5 DoDs (Metric Registration / Metric Existence and Kind / Counter Semantics / Gauge Semantics / Principle Kind Enforcement / Constraint No Business Logic / Sequence Register Metric / Data Metric Catalog Table / Entity Metric / Entity Metric Kind / API POST + GET-by-id), the Error Mapping table, and §6 Acceptance Criteria. Register payload is now `(gts_id, metadata_fields)` (closed `array<string>`; values typed as `String` end-to-end); `MetricKind` (counter / gauge) is **derived** from the `gts_id` prefix against the two reserved kind base type ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) — no separate kind field, no trait, no catalog column. Catalog row shape collapses to `gts_id` (PK) + `metadata_fields` (array of strings) + `created_at`. Register-time validation rejects bad `gts_id` prefixes with `invalid_kind_prefix`; ingest-time validation rejects undeclared metadata keys with `unknown_metadata_key`. The prior generic metadata-validation symbol is retired in favor of these two specific error names. Gateway L1 Metadata Validation is now closed-shape (no free-form remainder, no preserved extras). Cites ADR 0012 §Amendment, PRD §5.1/§5.2/§5.7, and domain-model §2.5/§2.6/§2.8/§2.11.
- **0.2.0 — 2026-06-02** — Aligned the FEATURE with the unified plugin-DB metric catalog model (ADR 0012, supersedes ADRs 0007 / 0009 / 0010) and the updated DESIGN (component renamed to "Metric Catalog", REST/SPI path parameter `{type_uuid}` → `{gts_id}`, prior catalog row shape reduced to `gts_id` / `created_at` plus the per-metric metadata declaration, `usage_records` FK column renamed `metric_type_uuid` → `gts_id`). Removed the boot-seeded declared catalog flow (`Boot Seed Declared Metrics from Config`), the `Read Metric Chain` flow, and the `Declared-Metric Withdrawal` process; collapsed the two-catalog lookup precedence in `Catalog Kind Lookup` and `Gateway L1 Metadata Validation`. Dropped `parent_type_uuid`, indexable-trait gate, and `abstract` from all metric metadata schemas, examples, CDSL blocks, DoD bodies, and acceptance criteria. Renamed the `MetricTypeNotFound` / `MetricTypeAlreadyExists` SPI error variants to `MetricNotFound` / `MetricAlreadyExists` (REST `Problem.context.reason` and SDK `UsageCollectorError` variant). Cites DESIGN §3.2 Metric Catalog, §3.6 Register / Delete Metric sequences, §3.7 `metric_catalog` row shape, and ADR 0012. The in-plugin reference scheme (column type, index choice) is plugin-author choice per DESIGN §3.2 / §3.7. Preserved status transitions, deprecation semantics (referential-delete), and the kind+queryability classification model.
