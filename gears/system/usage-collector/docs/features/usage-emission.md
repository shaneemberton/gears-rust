<!--
cpt:
  version: 0.2.1
  updated: 2026-06-02
-->

# Feature: Usage Emission

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
  - [1.5 Explicit Non-Applicability](#15-explicit-non-applicability)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Emit Record (single)](#emit-record-single)
  - [Emit Records Batch](#emit-records-batch)
  - [Compensation Emission](#compensation-emission)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Attribution and PDP Authorization](#attribution-and-pdp-authorization)
  - [Kind Enforcement on Ingest](#kind-enforcement-on-ingest)
  - [Idempotency-Dedup Dispatch](#idempotency-dedup-dispatch)
  - [Metadata Size-Cap Enforcement](#metadata-size-cap-enforcement)
  - [Catalog Existence and Kind Lookup](#catalog-existence-and-kind-lookup)
- [4. States (CDSL)](#4-states-cdsl)
  - [Usage Record Ingestion Lifecycle State Machine](#usage-record-ingestion-lifecycle-state-machine)
- [5. Definitions of Done](#5-definitions-of-done)
  - [FR: Ingestion](#fr-ingestion)
  - [FR: Idempotency](#fr-idempotency)
  - [FR: Record Metadata](#fr-record-metadata)
  - [FR: Counter Semantics](#fr-counter-semantics)
  - [FR: Gauge Semantics](#fr-gauge-semantics)
  - [FR: Tenant Attribution](#fr-tenant-attribution)
  - [FR: Resource Attribution](#fr-resource-attribution)
  - [FR: Subject Attribution](#fr-subject-attribution)
  - [FR: Ingestion Authorization](#fr-ingestion-authorization)
  - [FR: Data Quality](#fr-data-quality)
  - [FR: Usage Compensation — Flow](#fr-usage-compensation--flow)
  - [FR: Usage Compensation — Value Matrix](#fr-usage-compensation--value-matrix)
  - [FR: Usage Compensation — L1 corrects_id](#fr-usage-compensation--l1-corrects_id)
  - [FR: Usage Compensation — Concurrency](#fr-usage-compensation--concurrency)
  - [FR: Usage Compensation — No Business Logic](#fr-usage-compensation--no-business-logic)
  - [NFR: Throughput](#nfr-throughput)
  - [NFR: Throughput Profile](#nfr-throughput-profile)
  - [NFR: Ingestion Latency](#nfr-ingestion-latency)
  - [NFR: Workload Isolation](#nfr-workload-isolation)
  - [NFR: Batch and Report Timing](#nfr-batch-and-report-timing)
  - [NFR: Availability Boundary](#nfr-availability-boundary)
  - [Principle: Idempotency by Key](#principle-idempotency-by-key)
  - [Principle: Kind Enforcement](#principle-kind-enforcement)
  - [Principle: Fail Closed](#principle-fail-closed)
  - [Principle: Pluggable Storage](#principle-pluggable-storage)
  - [Constraint: No Business Logic](#constraint-no-business-logic)
  - [Constraint: NFR Thresholds](#constraint-nfr-thresholds)
  - [ADR: Caller-supplied Attribution](#adr-caller-supplied-attribution)
  - [ADR: Mandatory Idempotency](#adr-mandatory-idempotency)
  - [Component: Ingestion Gateway](#component-ingestion-gateway)
  - [Sequence: Emit Usage Record](#sequence-emit-usage-record)
  - [Data: usage_records Table](#data-usage_records-table)
  - [Entity: Usage Record](#entity-usage-record)
  - [Entity: Record Metadata](#entity-record-metadata)
  - [Entity: Resource Ref](#entity-resource-ref)
  - [Entity: Subject Ref](#entity-subject-ref)
  - [Entity: Idempotency Key](#entity-idempotency-key)
  - [Entity: Metric](#entity-metric)
  - [Entity: Metric Kind](#entity-metric-kind)
  - [Entity: Security Context](#entity-security-context)
  - [API: POST /usage-collector/v1/records](#api-post-usage-collectorv1records)
  - [§2.3-item → DoD-ID Coverage Matrix](#23-item--dod-id-coverage-matrix)
- [6. Acceptance Criteria](#6-acceptance-criteria)
- [7. Changelog](#7-changelog)

<!-- /toc -->

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-usage-emission`

<!-- reference to DECOMPOSITION entry -->

- [ ] `p2` - `cpt-cf-usage-collector-feature-usage-emission`

## 1. Feature Context

### 1.1 Overview

Provides the single, contract-first write path for at-least-once ingestion of usage records from authenticated source gears. Every emit — single or batched, REST or SDK — flows through the Ingestion Gateway. The `cpt-cf-usage-collector-component-ingestion-gateway` accepts the source gear's `cpt-cf-usage-collector-entity-security-context` (resolved upstream by the ToolKit gateway on REST as `Extension<SecurityContext>` populated via `OperationBuilder::authenticated()`, or supplied verbatim by the in-process caller on the SDK trait surface where `UsageCollectorClientV1` methods take `ctx: &SecurityContext` as their first parameter) and authorizes every record's full attribution tuple (tenant, resource, optional subject, source gear, Metric `gts_id`) through the per-component `authz_scope` helper wrapping `cpt-cf-usage-collector-contract-authz-resolver` fail-closed; the metric-lifecycle Metrics Catalog projection confirms Metric existence and kind, kind-dependent invariants are enforced (counter records reject negative deltas, gauges accept point-in-time values as-is), the configurable `RecordMetadata` size cap is enforced, and the validated record is dispatched through the Plugin SPI for durable persistence under the dedup composite `(tenant_id, metric_gts_id, idempotency_key)`. This is the only write path into `usage_records`; aggregation, query, deactivation, and audit-ledger semantics are owned elsewhere.

**Consistency posture (ingestion ack vs. query visibility).** The synchronous `Acknowledged` outcome returned by this feature is the ONLY surface that binds the gear-level consistency floor for write-derived state: durability and `(tenant_id, metric_gts_id, idempotency_key)` dedup-tuple visibility on the ingestion path are guaranteed at ack per `cpt-cf-usage-collector-adr-mandatory-idempotency`. Visibility of the same record through the read surfaces owned by `cpt-cf-usage-collector-feature-usage-query` and the metric-catalog reads owned by `cpt-cf-usage-collector-feature-metric-lifecycle` is governed separately by `cpt-cf-usage-collector-nfr-query-freshness` and `cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011): eventually consistent with no upper bound at the gear floor, plugin-bound by the active plugin's published ceiling. Source gears that need same-request outcome (admission control, post-emit summary, immediate-readback dashboards) MUST consume the ingestion ack this feature returns; they MUST NOT round-trip through the Query SPI for that purpose. Full contract: DESIGN [§3.10.8](../DESIGN.md#3108-consistency-contract).

### 1.2 Purpose

This feature exists so that at-least-once delivery of usage records is uniformly safe across counter and gauge kinds — caller-supplied idempotency keys absorb retries without inflating counter totals or poisoning gauge point-in-time signals, the per-component `authz_scope` helper invocation against `cpt-cf-usage-collector-contract-authz-resolver` inside `cpt-cf-usage-collector-component-ingestion-gateway` makes every emit fail-closed on attribution authorization, the metric-lifecycle catalog projection makes kind-and-existence enforcement deterministic on the hot path without round-tripping the Plugin SPI per record, and persistence is delegated through the contract-stable Plugin SPI so the metering substrate keeps a single, narrow ingestion contract regardless of the operator-selected storage backend.

**Requirements**: `cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-idempotency`, `cpt-cf-usage-collector-fr-record-metadata`, `cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-gauge-semantics`, `cpt-cf-usage-collector-fr-tenant-attribution`, `cpt-cf-usage-collector-fr-resource-attribution`, `cpt-cf-usage-collector-fr-subject-attribution`, `cpt-cf-usage-collector-fr-ingestion-authorization`, `cpt-cf-usage-collector-fr-data-quality`, `cpt-cf-usage-collector-fr-usage-compensation`, `cpt-cf-usage-collector-nfr-throughput`, `cpt-cf-usage-collector-nfr-throughput-profile`, `cpt-cf-usage-collector-nfr-ingestion-latency`, `cpt-cf-usage-collector-nfr-workload-isolation`, `cpt-cf-usage-collector-nfr-batch-and-report-timing`, `cpt-cf-usage-collector-nfr-availability-boundary`

**Principles**: `cpt-cf-usage-collector-principle-idempotency-by-key`, `cpt-cf-usage-collector-principle-kind-enforcement`, `cpt-cf-usage-collector-principle-fail-closed`, `cpt-cf-usage-collector-principle-pluggable-storage`

### 1.3 Actors

| Actor                                             | Role in Feature                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-actor-usage-source`       | Authenticated source gear that emits usage records (single or batched, REST or SDK) through `POST /usage-collector/v1/records`; supplies the full attribution tuple (target tenant via `SecurityContext`, mandatory `ResourceRef`, optional `SubjectRef`, source-gear identity, Metric `gts_id`) and a mandatory caller-supplied idempotency key per record; subject to PDP authorization on the full tuple per `cpt-cf-usage-collector-fr-ingestion-authorization`, to kind-dependent invariants per `cpt-cf-usage-collector-fr-counter-semantics` / `cpt-cf-usage-collector-fr-gauge-semantics`, and to the configurable `RecordMetadata` size cap per `cpt-cf-usage-collector-fr-record-metadata` |
| `cpt-cf-usage-collector-actor-platform-developer` | Integrates source gears with the Usage Collector via the in-process SDK trait (`emit` / `emit_batch` operations) routed through the same Ingestion Gateway as the REST surface; consumes the published Plugin SPI documentation when authoring a storage backend that persists `usage_records` under the composite dedup key `(tenant_id, metric_gts_id, idempotency_key)`; the SDK trait deliberately excludes Metric catalog management per `sdk-trait.md` §Out of scope, so Metric existence/kind discovery flows through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` against the catalog projection rather than through a separate SDK call                                 |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md) -- Usage Record Ingestion §5.1, Idempotent Ingestion §5.1, Per-Record Extensible Metadata §5.1, Counter Metric Kind §5.2, Gauge Metric Kind §5.2, Tenant Attribution §5.3, Resource Attribution §5.3, Subject Attribution §5.3, Ingestion Authorization §5.3, Data Quality Preservation §5.8, Usage Compensation §5.4 (counter value-reversal on the unified ingestion path; `cpt-cf-usage-collector-fr-usage-compensation`), Ingestion Throughput §6.1, Throughput Profile §6.1, Ingestion Latency §6.1, Workload Isolation §6.1, Batch and Report Timing §6.1, Availability Boundary §6.1, Actor catalog §2 (Usage Source, Platform Developer)
- **Design**: [DESIGN.md](../DESIGN.md) -- Ingestion Gateway component (§3.2), Emit Usage Record sequence `cpt-cf-usage-collector-seq-emit-usage` (§3.6), `usage_records` row shape and composite dedup key (§3.7), Unified ingestion request shape with `entry_type` + `corrects_id` + signed `value` (§3.3 — single emit path; no dedicated compensate endpoint), Correction posture two-primitive taxonomy (§3.10.3 — deactivation + compensation, un-policed-net note), Domain Model entities `UsageRecord` / `EntryType` (`cpt-cf-usage-collector-entity-entry-type`) / `RecordMetadata` / `ResourceRef` / `SubjectRef` / `IdempotencyKey` / `Metric` / `MetricKind` / `SecurityContext` (§3.1), PRD→DESIGN realization rows for `fr-ingestion`, `fr-idempotency`, `fr-record-metadata`, `fr-counter-semantics`, `fr-gauge-semantics`, `fr-tenant-attribution`, `fr-resource-attribution`, `fr-subject-attribution`, `fr-ingestion-authorization`, `fr-data-quality`, `fr-usage-compensation`, `nfr-throughput`, `nfr-throughput-profile`, `nfr-ingestion-latency`, `nfr-workload-isolation`, `nfr-batch-and-report-timing`, `nfr-availability-boundary` (§5.3)
- **ADR**: [ADR/0008-usage-compensation.md](../ADR/0008-usage-compensation.md) -- `cpt-cf-usage-collector-adr-usage-compensation` — counter value-reversal as a signed-negative `entry_type=compensation` row on the unified ingestion path with PDP attribution + mandatory idempotency; complemented by [ADR/0005-monotonic-deactivation.md](../ADR/0005-monotonic-deactivation.md) (`cpt-cf-usage-collector-adr-monotonic-deactivation`) for the orthogonal cross-kind retraction primitive; [ADR/0011-consistency-contract.md](../ADR/0011-consistency-contract.md) (`cpt-cf-usage-collector-adr-consistency-contract`) — the synchronous `Acknowledged` outcome returned by this feature is the surface read-after-write source-gear flows MUST consume; the read-side floor and per-plugin ceiling live with `cpt-cf-usage-collector-feature-usage-query`; [ADR/0012-unified-plugin-catalog-and-gts-id-reference.md](../ADR/0012-unified-plugin-catalog-and-gts-id-reference.md) (`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) — unified plugin-DB Metric Catalog and `gts_id` reference (usage records reference metrics by `gts_id`; the in-plugin reference scheme — column type, index choice — is plugin-author choice per DESIGN §3.2 / §3.7 and out of FEATURE scope)
- **Decomposition**: [DECOMPOSITION.md](../DECOMPOSITION.md) -- §2.3 Usage Emission
- **Foundation feature**: [foundation.md](./foundation.md) -- SecurityContext acceptance at the surface boundaries (REST `Extension<SecurityContext>` from ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK trait methods accepting `ctx: &SecurityContext` as the first parameter), PDP enforcement via the per-component `authz_scope` helper (`cpt-cf-usage-collector-flow-foundation-pdp-authorize`), plugin host binding, audit-correlation propagation (`cpt-cf-usage-collector-algo-foundation-audit-correlation`), tenant isolation, fail-closed posture (reused, not re-defined)
- **Metric Lifecycle feature**: [metric-lifecycle.md](./metric-lifecycle.md) -- platform-global Metric catalog and the in-process Metrics Catalog projection consumed on the ingestion hot path via `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` (reused, not re-defined)
- **Plugin SPI reference**: [plugin-spi.md](../plugin-spi.md) -- usage-record persistence capability and the storage-plugin composite idempotency key `(tenant_id, metric_gts_id, idempotency_key)`
- **SDK trait reference**: [sdk-trait.md](../sdk-trait.md) -- `emit` and `emit_batch` operations routed through the Ingestion Gateway (Metric catalog management deliberately excluded per §Out of scope)
- **REST contract**: [usage-collector-v1.yaml](../usage-collector-v1.yaml) -- `POST /usage-collector/v1/records` path (single and batched submissions)
- **Dependencies**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-metric-lifecycle`

### 1.5 Explicit Non-Applicability

- **UX** (`UX-FDESIGN-001` user journey, `UX-FDESIGN-002` accessibility): Not applicable because the usage-emission feature is a backend write surface (`POST /usage-collector/v1/records` plus the in-process SDK `emit` / `emit_batch` operations routed through the same Ingestion Gateway); there is no human-facing UI in this gear, the only direct consumers are authenticated source gears (`cpt-cf-usage-collector-actor-usage-source`) and SDK integrators (`cpt-cf-usage-collector-actor-platform-developer`), and any UI surfacing of usage data is delivered downstream by the §2.4 Usage Query consumers outside this feature's scope. Developer experience on the ingestion contract is encoded through the deterministic `Problem` error envelopes and idempotency semantics published by `usage-collector-v1.yaml` and `sdk-trait.md`.

## 2. Actor Flows (CDSL)

### Emit Record (single)

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-usage-emission-emit-record`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Success Scenarios**:

- An authenticated source gear submits a single usage record (via `POST /usage-collector/v1/records` with a one-item `IngestRecordsRequest` or via the SDK single-emit operation routed through `cpt-cf-usage-collector-component-ingestion-gateway`); `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` resolves the caller into a `cpt-cf-usage-collector-entity-security-context` and authorizes the full attribution tuple, `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` resolves the Metric `gts_id` and `cpt-cf-usage-collector-entity-metric-kind`, `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` enforces counter/gauge invariants, `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` enforces the configurable 8 KiB `cpt-cf-usage-collector-entity-record-metadata` cap, and `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` calls the Plugin SPI single-record persist capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records`; the per-record acknowledgement returns `outcome="accepted"` with the plugin-minted `id` per `usage-collector-v1.yaml`.
- An EXACT-EQUALITY retry sharing the same composite key — where ALL caller canonical fields (`value`, `timestamp`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, and `cpt-cf-usage-collector-entity-record-metadata`) match the stored record — returns `outcome="duplicate"` with the prior record's `id` from the SPI's `Deduplicated` outcome — no second write occurs and counter totals are not inflated per `cpt-cf-usage-collector-principle-idempotency-by-key`.

**Error Scenarios**:

- Request arrives without a resolved `cpt-cf-usage-collector-entity-security-context` (REST handler did not receive `Extension<SecurityContext>` from ToolKit gateway middleware, or the SDK trait was invoked without a `ctx` argument) — whole-request rejection via the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml`; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` and no record is dispatched.
- PDP denies the ingestion attribution tuple — surfaced as the per-record `outcome="rejected"` inside `IngestRecordsResponse` carrying a per-record `error: Problem` with `context.reason="authz"` per `usage-collector-v1.yaml` and the foundation per-tuple PDP contract; no SPI dispatch occurs.
- Metric `gts_id` is not present in the in-process catalog projection — surfaced as the per-record `outcome="rejected"` inside `IngestRecordsResponse` carrying a per-record `error: Problem` with `context.reason="unknown_metric"`; no SPI dispatch occurs.
- Counter record carries a negative `value` (violates the §3.7 referential rule `value >= 0`) — surfaced as the per-record `outcome="rejected"` with `context.reason="kind_invariant"` from `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`.
- `cpt-cf-usage-collector-entity-record-metadata` exceeds the configured size cap (default 8 KiB) — surfaced as the per-record `outcome="rejected"` with `context.reason="metadata_size"`.
- Plugin SPI transport / readiness / persistence error — surfaced as the per-record `outcome="rejected"` with `context.reason="plugin_readiness"` and the audit-correlation context preserved through `cpt-cf-usage-collector-algo-foundation-audit-correlation`.
- A same-key submission whose canonical fields DIFFER from the stored record (e.g., the same `(tenant_id, metric_gts_id, idempotency_key)` resubmitted with a different `value`) — the plugin returns `PersistOutcome::Conflict { id }` and the per-record outcome is `outcome="rejected"` carrying a per-record `error: Problem` with `context.reason="idempotency_conflict"` (AlreadyExists/409) and the existing record's `id`; the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Steps**:

1. [ ] - `p1` - Caller submits a single `cpt-cf-usage-collector-entity-usage-record` payload — on REST through `POST /usage-collector/v1/records` (one-item `IngestRecordsRequest`); the REST handler receives `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and W3C audit-correlation headers — or on the SDK through `UsageCollectorClientV1::submit_usage_record(ctx, ...)` with `ctx: &SecurityContext` as the first parameter per `sdk-trait.md` Method 1; the payload carries the mandatory caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` - `inst-emit-record-submit`
2. [ ] - `p1` - **IF** the REST handler receives no `Extension<SecurityContext>` (gateway middleware rejected the call upstream) or the SDK trait is invoked without a `ctx` argument **RETURN** the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml` default response; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` - `inst-emit-record-missing-ctx`
3. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` to authorize the attribution tuple through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper wrapping `cpt-cf-usage-collector-contract-authz-resolver`) against the inbound `cpt-cf-usage-collector-entity-security-context` and the attribution tuple (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`), receiving the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair - `inst-emit-record-attrib-authz`
4. [ ] - `p1` - **IF** the per-record authorization outcome is `deny` compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="authz"`) per `usage-collector-v1.yaml` and the foundation per-tuple PDP contract, then **RETURN** that response — no SPI dispatch occurs - `inst-emit-record-pdp-deny`
5. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` with the record's Metric `gts_id` to obtain the (`cpt-cf-usage-collector-entity-metric-kind`, optional `unit`) pair from the metric-lifecycle Metrics Catalog projection - `inst-emit-record-catalog-lookup`
6. [ ] - `p1` - **IF** the catalog lookup returns `not-found` compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="unknown_metric"`) per `usage-collector-v1.yaml` without falling back to a direct Plugin SPI catalog read per `cpt-cf-usage-collector-principle-fail-closed`, then **RETURN** that response - `inst-emit-record-metric-not-found`
7. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` with the submitted value, timestamp, and the catalog-resolved `cpt-cf-usage-collector-entity-metric-kind` (and optional `unit`) - `inst-emit-record-kind-enforce`
8. [ ] - `p1` - **IF** the kind-enforcement algorithm returns a counter-or-gauge invariant violation compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="kind_invariant"`) per `cpt-cf-usage-collector-fr-counter-semantics` and `cpt-cf-usage-collector-fr-gauge-semantics`, then **RETURN** that response - `inst-emit-record-kind-invalid`
9. [ ] - `p1` - Perform the **closed-shape** metadata-key check against the metric's `metadata_fields` (resolved from the gateway L1 cache populated from the Metric Catalog per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`): every key in the candidate `cpt-cf-usage-collector-entity-record-metadata` MUST be a declared member of `metadata_fields`; otherwise compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="unknown_metadata_key"`) citing `context.key` (the offending key) and `instance_path="/metadata/{key}"`, then **RETURN** that response - `inst-emit-record-metadata-closed-shape`
10. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` against the record's `cpt-cf-usage-collector-entity-record-metadata` payload (configurable default 8 KiB cap per `cpt-cf-usage-collector-fr-record-metadata`) - `inst-emit-record-metadata-cap`
11. [ ] - `p1` - **IF** the metadata-size-cap algorithm returns `metadata-too-large` compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="metadata_size"`) carrying the measured size and the configured cap, then **RETURN** that response - `inst-emit-record-metadata-too-large`
12. [ ] - `p1` - **TRY** dispatch the validated record via `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`, which invokes the Plugin SPI single-record persist capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` and surfaces a single `PersistOutcome` (`Persisted { id }`, `Deduplicated { id }`, or `Conflict { id }`) - `inst-emit-record-spi-dispatch`
13. [ ] - `p1` - **CATCH** Plugin SPI transport / readiness / persistence error - `inst-emit-record-spi-catch`
    1. [ ] - `p1` - Compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="plugin_readiness"`) while preserving the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`, then **RETURN** that response; no record is acknowledged - `inst-emit-record-spi-fail`
14. [ ] - `p1` - **IF** the SPI returned `Deduplicated { id }` (an exact-equality retry — ALL caller canonical fields equal) compose `IngestRecordsResponse` with `results[0].outcome="duplicate"`, `results[0].id` set to the prior record's `id`, and `results[0].idempotency_key` set to the canonical idempotency key per `usage-collector-v1.yaml`, then **RETURN** that response — duplicates MUST NOT inflate counter totals per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-emit-record-duplicate`
15. [ ] - `p1` - **ELSE IF** the SPI returned `Conflict { id }` (a same-key submission whose canonical fields differ from the stored record) compose `IngestRecordsResponse` with `results[0].outcome="rejected"` and `results[0].error: Problem` (`context.reason="idempotency_conflict"`, AlreadyExists/409) carrying the existing record's `id`, then **RETURN** that response — the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency` - `inst-emit-record-conflict`
16. [ ] - `p1` - **ELSE** the SPI returned `Persisted { id }`; compose `IngestRecordsResponse` with `results[0].outcome="accepted"`, `results[0].id` set to the plugin-minted `id`, and `results[0].idempotency_key` set to the canonical idempotency key per `usage-collector-v1.yaml`, propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`, then **RETURN** that response - `inst-emit-record-accepted`

### Emit Records Batch

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Success Scenarios**:

- An authenticated source gear submits a batch of up to 100 usage records via `POST /usage-collector/v1/records` (`IngestRecordsRequest.records` with `maxItems: 100` per the wire-level cap from `cpt-cf-usage-collector-nfr-batch-and-report-timing`) or via the SDK batch-emit operation routed through `cpt-cf-usage-collector-component-ingestion-gateway`; `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` authorizes each record's attribution tuple, `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` resolves each record's `gts_id` against the metric-lifecycle Metrics Catalog projection, `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` and `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` validate each record independently, and `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` invokes the Plugin SPI batch persist capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` per record; per-record outcomes are surfaced in input order (`outcome="accepted"` / `outcome="duplicate"` / `outcome="rejected"` with per-record `error: Problem` detail) per `usage-collector-v1.yaml`.
- A mixed-outcome batch (at least one rejected plus at least one accepted or deduplicated) returns HTTP `207 Multi-Status` carrying the per-record outcome array in input order; per-record validation failures, PDP denials per record, unknown Metrics, kind invariant violations, metadata oversize, same-key canonical-field mismatches (`context.reason="idempotency_conflict"`), and per-record SPI errors are all surfaced as `Err` entries inside the same batch result — there is no whole-batch rollback per `cpt-cf-usage-collector-component-ingestion-gateway` per-record acceptance promise.

**Error Scenarios**:

- Batch size exceeds the per-call cap of 100 records (`cpt-cf-usage-collector-nfr-batch-and-report-timing` wire-level enforcement via the `maxItems: 100` constraint on `IngestRecordsRequest.records`) — request-level structural validation rejection with the `Problem` envelope (HTTP `400`) per `usage-collector-v1.yaml` before any per-record processing.
- Empty `records` list (violates the `minItems: 1` schema constraint) — request-level structural validation rejection with the `Problem` envelope (HTTP `400`) per `usage-collector-v1.yaml`.
- Request arrives without a resolved `cpt-cf-usage-collector-entity-security-context` (REST handler did not receive `Extension<SecurityContext>` from ToolKit gateway middleware, or the SDK trait was invoked without a `ctx` argument) — request-level rejection via the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml`; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` and no per-record processing occurs.
- PDP denies every record in the batch (e.g., the source gear is unauthorized for the requested tenant in aggregate) — the per-record outcome array surfaces a `rejected` entry with `context.reason="authz"` for every record; whole-batch HTTP `207 Multi-Status` is returned per `usage-collector-v1.yaml` because PDP is a per-tuple decision under `cpt-cf-usage-collector-flow-foundation-pdp-authorize` and there is no envelope-level PDP deny mode.
- Per-record errors (unknown Metric, kind invariant violation, metadata oversize, same-key canonical-field mismatch with `context.reason="idempotency_conflict"`, per-record SPI failure) — surfaced per record inside the result list while the request itself returns HTTP `200` (all accepted/duplicated) or HTTP `207` (mixed or all-rejected — i.e., whenever ≥1 record is rejected, single-record conflict included), per `usage-collector-v1.yaml`.

**Steps**:

1. [ ] - `p1` - Caller submits an `IngestRecordsRequest` of up to 100 `cpt-cf-usage-collector-entity-usage-record` payloads — on REST through `POST /usage-collector/v1/records`; the REST handler receives `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and W3C audit-correlation headers — or on the SDK through `UsageCollectorClientV1::submit_usage_records(ctx, ...)` with `ctx: &SecurityContext` as the first parameter per `sdk-trait.md` Method 2; the payload carries one mandatory caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` per record - `inst-emit-batch-submit`
2. [ ] - `p1` - **IF** the REST handler receives no `Extension<SecurityContext>` (gateway middleware rejected the call upstream) or the SDK trait is invoked without a `ctx` argument **RETURN** the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml` default response; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` - `inst-emit-batch-missing-ctx`
3. [ ] - `p1` - **IF** the request `records` array is empty (`minItems: 1` violation) or larger than 100 entries (`maxItems: 100` violation per the wire-level cap from `cpt-cf-usage-collector-nfr-batch-and-report-timing`) **RETURN** the request-level structural validation `Problem` envelope (HTTP `400`) per `usage-collector-v1.yaml` without any per-record processing - `inst-emit-batch-cap-check`
4. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` once on the request envelope — the algorithm performs per-record PDP authorization through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper wrapping `cpt-cf-usage-collector-contract-authz-resolver` (one PDP call per record's caller-supplied attribution tuple per `cpt-cf-usage-collector-adr-caller-supplied-attribution`), returning a per-record `allow`/`deny` outcome list in input order - `inst-emit-batch-pdp`
5. [ ] - `p1` - **FOR EACH** `cpt-cf-usage-collector-entity-usage-record` in the request `records` array (in input order, preserving index for the per-record result), consume its `allow`/`deny` outcome from step 4 - `inst-emit-batch-foreach-validate`
   1. [ ] - `p1` - Read this record's PDP outcome resolved at step 4 (no re-invocation of the algorithm per record) - `inst-emit-batch-record-pdp`
   2. [ ] - `p1` - **IF** the per-record PDP outcome is `deny` record the per-record outcome `rejected` with the propagated platform-authorization envelope (`context.reason="authz"`) and CONTINUE to the next record - `inst-emit-batch-record-deny`
   3. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` with the record's Metric `gts_id` - `inst-emit-batch-record-catalog`
   4. [ ] - `p1` - **IF** the catalog lookup returned `not-found` record the per-record outcome `rejected` (`context.reason="unknown_metric"`) and CONTINUE - `inst-emit-batch-record-unknown-metric`
   5. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` with the catalog-resolved `cpt-cf-usage-collector-entity-metric-kind` (and optional `unit`) - `inst-emit-batch-record-kind`
   6. [ ] - `p1` - **IF** the kind-enforcement algorithm returned any `invalid-*` outcome record the per-record outcome `rejected` with the appropriate `context.reason` per the algorithm-outcome → wire-reason-code mapping defined under `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` (`kind_invariant` for matrix-cell value-sign or shape violations including `invalid-counter-delta`, `invalid-compensation-value`, `invalid-corrects-id-required`, `invalid-corrects-id-forbidden`, and the defensive `unsupported-kind`; `gauge_compensation_rejected` for `invalid-gauge-compensation`; `corrects_id_not_found` / `corrects_id_wrong_entry_type` / `corrects_id_wrong_scope` / `corrects_id_inactive` for the four L1 referential failures) and CONTINUE - `inst-emit-batch-record-kind-invalid`
   7. [ ] - `p1` - Perform the **closed-shape** metadata-key check against the metric's `metadata_fields` (resolved via the catalog lookup at step 5.3 per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`); **IF** any key in the candidate `cpt-cf-usage-collector-entity-record-metadata` is not a declared member of `metadata_fields` record the per-record outcome `rejected` (`context.reason="unknown_metadata_key"`, citing `context.key` and `instance_path="/metadata/{key}"`) and CONTINUE - `inst-emit-batch-record-metadata-closed-shape`
   8. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` against the record's `cpt-cf-usage-collector-entity-record-metadata` - `inst-emit-batch-record-metadata`
   9. [ ] - `p1` - **IF** the metadata-size-cap algorithm returned `metadata-too-large` record the per-record outcome `rejected` (`context.reason="metadata_size"`) and CONTINUE - `inst-emit-batch-record-metadata-too-large`
   10. [ ] - `p1` - Mark the record as eligible-for-persist and append it to the batch dispatch buffer in input-order index - `inst-emit-batch-record-eligible`
6. [ ] - `p1` - **TRY** dispatch the eligible-for-persist buffer via `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` in batch mode — invokes the Plugin SPI batch persist capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` per record and receives a per-record `Result<PersistOutcome, _>` list in input order per `plugin-spi.md` Method 2 - `inst-emit-batch-spi-dispatch`
7. [ ] - `p1` - **CATCH** Plugin SPI call-level transport / readiness error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) - `inst-emit-batch-spi-catch`
   1. [ ] - `p1` - Mark every eligible-for-persist record's per-record outcome `rejected` (`context.reason="plugin_readiness"`) preserving the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`; the failure is per-record, not whole-batch rollback - `inst-emit-batch-spi-fail-mark`
8. [ ] - `p1` - **FOR EACH** SPI per-record result paired with its original input index - `inst-emit-batch-foreach-spi`
   1. [ ] - `p1` - **IF** the SPI returned `Persisted { id }` record the per-record outcome `accepted` with the plugin-minted `id` per `usage-collector-v1.yaml` - `inst-emit-batch-record-accepted`
   2. [ ] - `p1` - **ELSE IF** the SPI returned `Deduplicated { id }` (an exact-equality retry — ALL caller canonical fields equal) record the per-record outcome `duplicate` with the prior record's `id` per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-emit-batch-record-dedup`
   3. [ ] - `p1` - **ELSE IF** the SPI returned `Conflict { id }` (a same-key submission whose canonical fields differ from the stored record) record the per-record outcome `rejected` with a per-record `error: Problem` (`context.reason="idempotency_conflict"`, AlreadyExists/409) carrying the existing record's `id`; the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency` - `inst-emit-batch-record-conflict`
   4. [ ] - `p1` - **ELSE** the SPI returned a per-record `Err` (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`); record the per-record outcome `rejected` (`context.reason="plugin_readiness"`) - `inst-emit-batch-record-spi-err`
9. [ ] - `p1` - Compose the `IngestRecordsResponse` `results` array in input-order index, merging the validation-stage outcomes from step 5 with the SPI-stage outcomes from step 8 - `inst-emit-batch-compose-response`
10. [ ] - `p1` - **IF** every per-record outcome is `accepted` or `duplicate` **RETURN** HTTP `200` with the `IngestRecordsResponse` per `usage-collector-v1.yaml` - `inst-emit-batch-return-200`
11. [ ] - `p1` - **ELSE** **RETURN** HTTP `207 Multi-Status` with the `IngestRecordsResponse` carrying the mixed per-record outcomes in input order, propagating the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` (no whole-batch rollback per `cpt-cf-usage-collector-component-ingestion-gateway` per-record acceptance promise) - `inst-emit-batch-return-207`

### Compensation Emission

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-usage-emission-compensation`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Trigger** (real-world give-back from the source gear): a capacity refund, a partial cancellation, a dispute resolution, a billing-period correction, or any other source-gear-determined value-reversal event. The trigger is owned by the source gear — UC records the reversal the source gear decides to apply, never computes one itself per `cpt-cf-usage-collector-adr-usage-compensation` and `cpt-cf-usage-collector-constraint-no-business-logic`.

**Success Scenarios**:

- An authenticated source gear submits a counter-only value-reversal record on the **same unified ingestion path** (`POST /usage-collector/v1/records` with a one-item or batched `IngestRecordsRequest` whose record carries `entry_type=compensation`, `value < 0`, and a non-empty `corrects_id` referencing a target `entry_type=usage` row; or the equivalent SDK emit operation routed through `cpt-cf-usage-collector-component-ingestion-gateway`) — there is NO dedicated `compensate` REST path, SDK method, or Plugin SPI call per `cpt-cf-usage-collector-adr-usage-compensation` and DESIGN §3.3. `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` performs PDP attribution on the same per-record tuple as ordinary ingestion, `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` resolves the Metric `gts_id` and `cpt-cf-usage-collector-entity-metric-kind`, `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` enforces the four-cell `(MetricKind × EntryType)` value matrix and the L1 `corrects_id` referential checks (existence, `entry_type=usage`, same `(tenant_id, metric_gts_id)`, `status=active`), `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` enforces the `cpt-cf-usage-collector-entity-record-metadata` cap, and `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` invokes the Plugin SPI persist capability under the same composite key `(tenant_id, metric_gts_id, idempotency_key)`. The per-record acknowledgement returns `outcome="accepted"` with the plugin-minted `id`.
- An EXACT-EQUALITY retry of the same compensation submission (same composite `(tenant_id, metric_gts_id, idempotency_key)` and identical canonical fields including `entry_type`, `value`, `corrects_id`, `timestamp`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, and `cpt-cf-usage-collector-entity-record-metadata`) returns `outcome="duplicate"` with the prior compensation row's `id` — no second write occurs and no second refund effect is recorded per `cpt-cf-usage-collector-principle-idempotency-by-key`. Mandatory idempotency prevents double-refund for free per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Error Scenarios**:

- Request arrives without a resolved `cpt-cf-usage-collector-entity-security-context` — whole-request rejection via the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml`; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed`.
- PDP denies the ingestion attribution tuple — surfaced as the per-record `outcome="rejected"` with `context.reason="authz"` (same path as ordinary ingestion; no parallel authorization surface).
- Metric `gts_id` is not present in the in-process catalog projection — surfaced as the per-record `outcome="rejected"` with `context.reason="unknown_metric"`.
- The target Metric is `gauge` (gauges have no `SUM` semantics) — surfaced as the per-record `outcome="rejected"` with `context.reason="gauge_compensation_rejected"` per the four-cell value matrix (`gauge + compensation → REJECTED`) and `usage-collector-v1.yaml` Problem.context.reason taxonomy (HTTP `422`).
- `entry_type=compensation` with `value >= 0` (compensation MUST be strictly negative per `counter + compensation → value < 0`) — surfaced as the per-record `outcome="rejected"` with `context.reason="kind_invariant"` (matrix-cell value-sign violation).
- `entry_type=compensation` with empty or missing `corrects_id` — surfaced as the per-record `outcome="rejected"` with `context.reason="kind_invariant"`; the `counter + compensation` matrix cell REQUIRES a non-empty `corrects_id`, so a missing pointer violates the cell's shape and folds into the matrix-invariant code (no separate `corrects_id_required` wire code is defined in `usage-collector-v1.yaml` or `sdk-trait.md`).
- `entry_type=usage` with a non-empty `corrects_id` (usage rows MUST NOT carry `corrects_id`) — surfaced as the per-record `outcome="rejected"` with `context.reason="kind_invariant"` for the symmetric shape violation of the `*+usage` cells.
- `corrects_id` refers to a row that does not exist — surfaced as the per-record `outcome="rejected"` with `context.reason="corrects_id_not_found"` (HTTP `404`) per `usage-collector-v1.yaml` and `sdk-trait.md` `CorrectsIdNotFound`.
- `corrects_id` refers to a row whose `entry_type != usage` (e.g. another `compensation` row; compensating a compensation is a non-goal per `cpt-cf-usage-collector-adr-usage-compensation`) — surfaced as the per-record `outcome="rejected"` with `context.reason="corrects_id_wrong_entry_type"` (HTTP `409`) per `usage-collector-v1.yaml` and `sdk-trait.md` `CorrectsIdWrongEntryType`.
- `corrects_id` refers to a row whose `(tenant_id, metric_gts_id)` does not match the incoming compensation (cross-tenant or cross-Metric reference) — surfaced as the per-record `outcome="rejected"` with `context.reason="corrects_id_wrong_scope"` (HTTP `409`) per `usage-collector-v1.yaml` and `sdk-trait.md` `CorrectsIdWrongScope`.
- `corrects_id` refers to a row whose `status != active` (deactivated, including a row **concurrently being deactivated** — the L1 "must be active" check serialises against the cascade transition per `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`) — surfaced as the per-record `outcome="rejected"` with `context.reason="corrects_id_inactive"` (HTTP `409`) per `usage-collector-v1.yaml` and `sdk-trait.md` `CorrectsIdInactive`. There is no quarantine, no retry queue, and no compensating cascade for the rejection — the source gear retries at its own discretion (idempotency key makes retries safe).
- Missing idempotency key — surfaced as the per-record `outcome="rejected"` per `cpt-cf-usage-collector-adr-mandatory-idempotency` (the wire-level `idempotency_key` requirement is uniform across `entry_type`).
- A same-key submission with `entry_type=compensation` whose canonical fields differ from the stored record (including a `corrects_id` mismatch, a `value` mismatch, or a metadata-only difference) — surfaced as the per-record `outcome="rejected"` with `context.reason="idempotency_conflict"` (AlreadyExists/409) carrying the existing record's `id`; the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Steps**:

1. [ ] - `p1` - Source gear computes a give-back amount according to its own business logic and constructs a single or batched `cpt-cf-usage-collector-entity-usage-record` payload with `entry_type=compensation`, a signed-negative `value`, a non-empty `corrects_id` referencing the target `entry_type=usage` row, and a mandatory caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` — submitted on the **same unified ingestion path** as ordinary usage emission (REST `POST /usage-collector/v1/records` or the SDK emit operation) - `inst-compensation-submit`
2. [ ] - `p1` - **IF** the REST handler receives no `Extension<SecurityContext>` or the SDK trait is invoked without a `ctx` argument **RETURN** the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml`; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` - `inst-compensation-missing-ctx`
3. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` to authorize the per-record attribution tuple via `cpt-cf-usage-collector-flow-foundation-pdp-authorize` — same PDP surface, same per-component `authz_scope` helper, same per-record `allow`/`deny` outcome shape as ordinary ingestion - `inst-compensation-attrib-authz`
4. [ ] - `p1` - **IF** the per-record authorization outcome is `deny` record `outcome="rejected"` with `context.reason="authz"` and **RETURN** the `IngestRecordsResponse` — no SPI dispatch occurs - `inst-compensation-pdp-deny`
5. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` with the record's Metric `gts_id` to obtain the (`cpt-cf-usage-collector-entity-metric-kind`, optional `unit`) pair - `inst-compensation-catalog-lookup`
6. [ ] - `p1` - **IF** the catalog lookup returns `not-found` record `outcome="rejected"` with `context.reason="unknown_metric"` and **RETURN** the response - `inst-compensation-metric-not-found`
7. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` with the submitted (`value`, `entry_type`, `corrects_id`, `tenant_id`, `metric_gts_id`) tuple and the catalog-resolved `cpt-cf-usage-collector-entity-metric-kind`; the algorithm runs the full four-cell `(MetricKind × EntryType)` value matrix AND the L1 `corrects_id` referential checks (existence, `entry_type=usage`, same `(tenant_id, metric_gts_id)`, `status=active`) - `inst-compensation-validate`
8. [ ] - `p1` - **IF** the algorithm returns any `invalid-*` outcome record `outcome="rejected"` with the appropriate `context.reason` per the algorithm-outcome → wire-reason-code mapping defined under `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` (`gauge_compensation_rejected` for `invalid-gauge-compensation`; `kind_invariant` for `invalid-counter-delta`, `invalid-compensation-value`, `invalid-corrects-id-required`, `invalid-corrects-id-forbidden`, or the defensive `unsupported-kind`; `corrects_id_not_found` for `invalid-corrects-id-not-found`; `corrects_id_wrong_entry_type` for `invalid-corrects-id-not-usage`; `corrects_id_wrong_scope` for `invalid-corrects-id-cross-tenant-or-metric`; `corrects_id_inactive` for `invalid-corrects-id-inactive` — which includes the "referenced record must be active" branch that handles concurrent deactivation) and **RETURN** the response — no SPI dispatch occurs - `inst-compensation-validate-fail`
9. [ ] - `p1` - Perform the **closed-shape** metadata-key check against the metric's `metadata_fields` (resolved at step 5 per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`): every key in the candidate `cpt-cf-usage-collector-entity-record-metadata` MUST be a declared member of `metadata_fields`; otherwise record `outcome="rejected"` with `context.reason="unknown_metadata_key"`, `context.key`, and `instance_path="/metadata/{key}"`, and **RETURN** the response - `inst-compensation-metadata-closed-shape`
10. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` against the record's `cpt-cf-usage-collector-entity-record-metadata` payload - `inst-compensation-metadata-cap`
11. [ ] - `p1` - **IF** the metadata-size-cap algorithm returns `metadata-too-large` record `outcome="rejected"` with `context.reason="metadata_size"` and **RETURN** the response - `inst-compensation-metadata-too-large`
12. [ ] - `p1` - **TRY** dispatch the validated compensation row via `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` — invokes the Plugin SPI persist capability under the same composite key `(tenant_id, metric_gts_id, idempotency_key)` as ordinary ingestion; the SPI receives the row with `entry_type=compensation`, signed-negative `value`, and the `corrects_id` pointer to the referenced `usage` row, and persists it with `status=active` - `inst-compensation-spi-dispatch`
13. [ ] - `p1` - **CATCH** Plugin SPI transport / readiness / persistence error - `inst-compensation-spi-catch`
    1. [ ] - `p1` - Compose `IngestRecordsResponse` with `outcome="rejected"` and `context.reason="plugin_readiness"` while preserving the audit-correlation context, then **RETURN** that response; no record is acknowledged - `inst-compensation-spi-fail`
14. [ ] - `p1` - **IF** the SPI returned `Deduplicated { id }` (an exact-equality retry — ALL caller canonical fields including `entry_type`, `value`, `corrects_id` equal) record `outcome="duplicate"` with the prior compensation row's `id` and **RETURN** the response — no double-refund effect, no second write per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-compensation-duplicate`
15. [ ] - `p1` - **ELSE IF** the SPI returned `Conflict { id }` (a same-key submission whose canonical fields differ from the stored record) record `outcome="rejected"` with `context.reason="idempotency_conflict"` (AlreadyExists/409) carrying the existing record's `id`, and **RETURN** the response — the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency` - `inst-compensation-conflict`
16. [ ] - `p1` - **ELSE** the SPI returned `Persisted { id }`; record `outcome="accepted"` with the plugin-minted `id` and the canonical idempotency key, propagate the audit-correlation context, then **RETURN** the response - `inst-compensation-accepted`

## 3. Processes / Business Logic (CDSL)

### Attribution and PDP Authorization

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`

**Input**: an inbound `POST /usage-collector/v1/records` REST request carrying the gateway-resolved `Extension<SecurityContext>`, audit-correlation headers, and a per-record attribution tuple (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`) per `cpt-cf-usage-collector-adr-caller-supplied-attribution`; OR an SDK `UsageCollectorClientV1::submit_usage_record(ctx, ...)` / `submit_usage_records(ctx, ...)` invocation carrying `ctx: &SecurityContext` as the first parameter and an equivalent attribution tuple per record.

**Output**: A per-record outcome list (each entry is `allow` with the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair attached, or `deny` with the propagated platform-authorization envelope (`context.reason="authz"`)) plus the audit-correlation context propagated for downstream stages. PDP decisions are per-attribution-tuple — there is no envelope-level PDP `deny` aggregation. This algorithm MUST NOT re-implement PDP logic — it invokes the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) once per record and forwards the attribution tuple to the PDP. Authentication is owned by the ToolKit gateway upstream of the REST handler and by the in-process caller on the SDK trait surface; the collector NEVER synthesizes identity and NEVER consults an authentication contract.

**Steps**:

1. [ ] - `p1` - Receive the inbound `cpt-cf-usage-collector-entity-security-context` at the `cpt-cf-usage-collector-component-ingestion-gateway` boundary — on REST as `Extension<SecurityContext>` from the gateway middleware, on SDK as the `ctx: &SecurityContext` first argument — and extract the per-record attribution tuples from the request payload - `inst-algo-attrib-receive-ctx`
2. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-foundation-audit-correlation` to open the server span and capture the `request-id` correlation pair (read from the inbound `SecurityContext.correlation_id`) so every PDP and Plugin SPI dispatch shares a single trace - `inst-algo-attrib-correlate`
3. [ ] - `p1` - **FOR EACH** per-record attribution tuple in the request envelope - `inst-algo-attrib-foreach`
   1. [ ] - `p1` - Compose the per-record attribution tuple (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`) required by `cpt-cf-usage-collector-flow-foundation-pdp-authorize` - `inst-algo-attrib-compose-tuple`
   2. [ ] - `p1` - Invoke `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the inbound `cpt-cf-usage-collector-entity-security-context` and the composed attribution tuple - `inst-algo-attrib-pdp-call`
   3. [ ] - `p1` - **IF** the foundation PDP flow returns `deny` record the per-record outcome `deny` with the propagated platform-authorization envelope (`context.reason="authz"`) - `inst-algo-attrib-pdp-deny`
   4. [ ] - `p1` - **ELSE** record the per-record outcome `allow` with the (`cpt-cf-usage-collector-entity-pdp-decision`, `cpt-cf-usage-collector-entity-pdp-constraint` set) pair attached to the record's downstream context - `inst-algo-attrib-pdp-allow`
4. [ ] - `p1` - **RETURN** the per-record (`outcome`, optional decision pair) list plus the audit-correlation context to the calling flow without caching the PDP decision per `cpt-cf-usage-collector-principle-fail-closed`; PDP decisions are per-attribution-tuple, so the calling flow surfaces every `deny` outcome as a per-record `outcome="rejected"` (`context.reason="authz"`) — there is no envelope-level PDP `deny` aggregation - `inst-algo-attrib-return`

### Kind Enforcement on Ingest

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`

**Input**: A single submitted `cpt-cf-usage-collector-entity-usage-record` payload (`value`, `timestamp`, `entry_type` per `cpt-cf-usage-collector-entity-entry-type`, optional `corrects_id`, `tenant_id`, `metric_gts_id`) plus the catalog-resolved (`cpt-cf-usage-collector-entity-metric-kind`, optional `unit`) returned by `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`.

**Output**: `valid` when the submitted record satisfies the kind-specific invariants for its (`cpt-cf-usage-collector-entity-metric-kind`, `cpt-cf-usage-collector-entity-entry-type`) cell of the four-cell value matrix AND — when `entry_type=compensation` — the L1 `corrects_id` referential checks pass; or one of `invalid-counter-delta` / `invalid-gauge-compensation` / `invalid-compensation-value` / `invalid-corrects-id-required` / `invalid-corrects-id-forbidden` / `invalid-corrects-id-not-found` / `invalid-corrects-id-not-usage` / `invalid-corrects-id-cross-tenant-or-metric` / `invalid-corrects-id-inactive` / `unsupported-kind`. This algorithm MUST encode the locked four-cell `(MetricKind × EntryType)` value matrix verbatim, MUST enforce the L1 `corrects_id` rule synchronously on the ingestion path, and MUST NOT dispatch the Plugin SPI persist path or maintain any L2 remaining-amount tracking. Recording a caller-supplied signed-negative value when `entry_type=compensation` is recording, not computing — this algorithm validates and rejects; it does NOT compute refunds, credits, credit-notes, or quota per `cpt-cf-usage-collector-constraint-no-business-logic` and `cpt-cf-usage-collector-adr-usage-compensation`. The concurrency rule against in-flight deactivation is realized by the L1 "referenced record MUST be active" check — a compensation arriving while the referenced row is being deactivated is rejected without quarantine or retry queue (versioned `-v2` to capture the four-cell matrix + L1 `corrects_id` extension per `cpt-cf-usage-collector-fr-usage-compensation`; supersedes the prior `kind-enforcement-on-ingest` algorithm which covered only the `entry_type=usage` cells).

**Four-cell value matrix** (verbatim — every step of this algorithm respects it):

| MetricKind | EntryType      | Allowed `value`                 |
| ---------- | -------------- | ------------------------------- |
| `counter`  | `usage`        | `value >= 0` (unchanged)        |
| `counter`  | `compensation` | `value < 0` (strictly negative) |
| `gauge`    | `usage`        | Any signed value (unchanged)    |
| `gauge`    | `compensation` | REJECTED before persistence     |

**Steps**:

1. [ ] - `p1` - Read the submitted `value`, `timestamp`, `entry_type` (defaults to `usage` for backward compatibility per `cpt-cf-usage-collector-entity-entry-type`), optional `corrects_id`, `tenant_id`, and `metric_gts_id` from the inbound `cpt-cf-usage-collector-entity-usage-record` payload without any Plugin SPI persist dispatch - `inst-algo-kind-read-input-v2`
2. [ ] - `p1` - Read the catalog-resolved `cpt-cf-usage-collector-entity-metric-kind` (and optional `unit`) supplied by `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` - `inst-algo-kind-read-kind-v2`
3. [ ] - `p1` - **IF** `entry_type = usage` AND `corrects_id` is set **RETURN** `invalid-corrects-id-forbidden` — `usage` rows MUST NOT carry `corrects_id` per `cpt-cf-usage-collector-entity-entry-type` and the L1 `corrects_id` rule (`corrects_id` is conditional on `entry_type=compensation`) - `inst-algo-kind-usage-corrects-id-forbidden`
4. [ ] - `p1` - **IF** `entry_type = compensation` AND `corrects_id` is empty or missing **RETURN** `invalid-corrects-id-required` — `compensation` rows MUST carry a non-empty `corrects_id` per the L1 rule - `inst-algo-kind-compensation-corrects-id-required`
5. [ ] - `p1` - **IF** `cpt-cf-usage-collector-entity-metric-kind` is `counter` - `inst-algo-kind-counter-branch-v2`
   1. [ ] - `p1` - **IF** `entry_type = usage` - `inst-algo-kind-counter-usage`
      1. [ ] - `p1` - **IF** the submitted `value` is below zero **RETURN** `invalid-counter-delta` per `cpt-cf-usage-collector-fr-counter-semantics` and the four-cell matrix (counter+usage requires `value >= 0`) - `inst-algo-kind-counter-usage-negative`
      2. [ ] - `p1` - **RETURN** `valid` — the counter+usage cell holds (`value >= 0`) - `inst-algo-kind-counter-usage-valid`
   2. [ ] - `p1` - **ELSE** `entry_type = compensation` - `inst-algo-kind-counter-compensation`
      1. [ ] - `p1` - **IF** the submitted `value` is greater than or equal to zero **RETURN** `invalid-compensation-value` per the four-cell matrix (counter+compensation requires `value < 0`; zero is not accepted as a no-op compensation) and `cpt-cf-usage-collector-fr-usage-compensation` - `inst-algo-kind-counter-compensation-non-negative`
      2. [ ] - `p1` - Perform the L1 `corrects_id` referential lookup against the storage plugin's `usage_records` projection: read the referenced row by `corrects_id` (single ingestion-time read; idempotent; not a persist dispatch) - `inst-algo-kind-l1-lookup`
      3. [ ] - `p1` - **IF** the lookup returns `not-found` **RETURN** `invalid-corrects-id-not-found` — the referenced row MUST exist (L1 rule) - `inst-algo-kind-l1-not-found`
      4. [ ] - `p1` - **IF** the referenced row's `entry_type != usage` **RETURN** `invalid-corrects-id-not-usage` — compensating a compensation is a non-goal per `cpt-cf-usage-collector-adr-usage-compensation` (L1 rule) - `inst-algo-kind-l1-not-usage`
      5. [ ] - `p1` - **IF** the referenced row's `tenant_id != incoming.tenant_id` OR `metric_gts_id != incoming.metric_gts_id` **RETURN** `invalid-corrects-id-cross-tenant-or-metric` — cross-tenant or cross-metric compensation is rejected per the L1 rule (compensation MUST share `(tenant_id, metric_gts_id)` with the referenced `usage` row) - `inst-algo-kind-l1-cross-scope`
      6. [ ] - `p1` - **IF** the referenced row's `status != active` **RETURN** `invalid-corrects-id-inactive` — the referenced row MUST be `active` per the L1 rule; a compensation referencing a row that is **concurrently being deactivated** is rejected by this same check (no quarantine, no retry queue, no compensating cascade for the rejection — the source gear retries at its own discretion; idempotency key makes retries safe per `cpt-cf-usage-collector-adr-mandatory-idempotency`) - `inst-algo-kind-l1-inactive-or-deactivating`
      7. [ ] - `p1` - **RETURN** `valid` — the counter+compensation cell holds (`value < 0`) and the L1 `corrects_id` referential checks all passed; the row is recorded as-supplied (no L2 remaining-amount tracking, no refund/credit/credit-note/quota computation per `cpt-cf-usage-collector-constraint-no-business-logic`) - `inst-algo-kind-counter-compensation-valid`
6. [ ] - `p1` - **ELSE IF** `cpt-cf-usage-collector-entity-metric-kind` is `gauge` - `inst-algo-kind-gauge-branch-v2`
   1. [ ] - `p1` - **IF** `entry_type = compensation` **RETURN** `invalid-gauge-compensation` — `gauge + compensation` is rejected before persistence per the four-cell matrix (gauges have no `SUM` semantics; the only correction for a gauge is deactivation per `cpt-cf-usage-collector-adr-monotonic-deactivation`) - `inst-algo-kind-gauge-compensation-rejected`
   2. [ ] - `p1` - Accept the submitted `value` as a point-in-time replacement per `cpt-cf-usage-collector-fr-gauge-semantics` and DESIGN §3.1 (gauges are stored as-is, no delta accumulation, no shape rewriting) - `inst-algo-kind-gauge-accept-v2`
   3. [ ] - `p1` - **RETURN** `valid` — the gauge+usage cell accepts any signed value as-is - `inst-algo-kind-gauge-valid-v2`
7. [ ] - `p1` - **ELSE** **RETURN** `unsupported-kind` (defensive — this branch is unreachable when the catalog projection is consistent because `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` constrains `cpt-cf-usage-collector-entity-metric-kind` to the registered enum set) - `inst-algo-kind-unsupported-v2`

**Algorithm outcome → wire `context.reason` mapping** (verbatim contract from this algorithm to the `Problem.context.reason` taxonomy locked in `usage-collector-v1.yaml` and the SDK trait error catalog in `sdk-trait.md`):

| Algorithm outcome                            | Wire `context.reason`          | HTTP status | SDK trait variant           |
| -------------------------------------------- | ------------------------------ | ----------- | --------------------------- |
| `valid`                                      | (none — accepted)              | `200`/`207` | n/a                         |
| `invalid-counter-delta`                      | `kind_invariant`               | `422`       | `Validation`                |
| `invalid-compensation-value`                 | `kind_invariant`               | `422`       | `Validation`                |
| `invalid-corrects-id-required`               | `kind_invariant`               | `422`       | `Validation`                |
| `invalid-corrects-id-forbidden`              | `kind_invariant`               | `422`       | `Validation`                |
| `invalid-gauge-compensation`                 | `gauge_compensation_rejected`  | `422`       | `GaugeCompensationRejected` |
| `invalid-corrects-id-not-found`              | `corrects_id_not_found`        | `404`       | `CorrectsIdNotFound`        |
| `invalid-corrects-id-not-usage`              | `corrects_id_wrong_entry_type` | `409`       | `CorrectsIdWrongEntryType`  |
| `invalid-corrects-id-cross-tenant-or-metric` | `corrects_id_wrong_scope`      | `409`       | `CorrectsIdWrongScope`      |
| `invalid-corrects-id-inactive`               | `corrects_id_inactive`         | `409`       | `CorrectsIdInactive`        |
| `unsupported-kind`                           | `kind_invariant`               | `422`       | `Validation` (defensive)    |

Notes (locked):

- The four granular `corrects_id_*` codes are sourced verbatim from `usage-collector-v1.yaml` `Problem.context.reason` enumeration (`corrects_id_not_found`, `corrects_id_wrong_entry_type`, `corrects_id_wrong_scope`, `corrects_id_inactive`) and `sdk-trait.md` (`CorrectsIdNotFound`, `CorrectsIdWrongEntryType`, `CorrectsIdWrongScope`, `CorrectsIdInactive`); they MUST NOT be collapsed back into a single generic code on the wire.
- Missing `corrects_id` on a `compensation` row and present `corrects_id` on a `usage` row are matrix-cell shape violations of the `counter+compensation` and `*+usage` cells respectively, so both surface as `kind_invariant` — there is no separate `corrects_id_required` or `corrects_id_forbidden` wire code in the locked taxonomy and none is introduced here.
- `gauge + compensation` lifts to the dedicated `gauge_compensation_rejected` code (HTTP `422`) rather than the generic `kind_invariant` code, because the locked five-code compensation taxonomy in `usage-collector-v1.yaml:1568` and `sdk-trait.md:677` carves it out as its own enum.

### Idempotency-Dedup Dispatch

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Input**: One eligible-for-persist `cpt-cf-usage-collector-entity-usage-record` (single dispatch) or a non-empty list of eligible-for-persist records (batch dispatch), each carrying `tenant_id`, `metric_gts_id`, the caller-supplied `cpt-cf-usage-collector-entity-idempotency-key`, and the resolved trace/audit-correlation context.

**Output**: A per-record outcome (`persisted { id }`, `duplicate { id }`, `conflict { id }`, or `spi-error { reason }`) preserving input order. On `duplicate { id }` — an EXACT-EQUALITY retry where ALL caller canonical fields match the stored record — the prior persisted record's `id` is returned without a second write and counter totals are not inflated per `cpt-cf-usage-collector-principle-idempotency-by-key`. On `conflict { id }` — a same-key submission whose canonical fields differ from the stored record — the existing record's `id` is carried and the outcome is surfaced as the `idempotency_conflict` rejection (AlreadyExists/409); the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency`. The canonical-field equality comparison (`value`, `timestamp`, `cpt-cf-usage-collector-entity-resource-ref`, `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, and `cpt-cf-usage-collector-entity-record-metadata`; the dedup-key tuple `(tenant_id, metric_gts_id, idempotency_key)` and the server-owned `id`/`status` are excluded) is performed by the storage plugin per `plugin-spi.md` — this algorithm documents the OUTCOME handling, NOT the comparison authority. This algorithm MUST cite the Plugin SPI composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records` and MUST NOT split the dedup check and the persist into separate SPI calls — the Plugin SPI is the single source of dedup truth and exposes one capability per dispatch (Method 1 `persist_usage_record` for single, Method 2 `persist_usage_records` for batch).

**Steps**:

1. [ ] - `p1` - Assemble the per-record composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records` UNIQUE constraint and `cpt-cf-usage-collector-fr-idempotency`; the `cpt-cf-usage-collector-entity-idempotency-key` window is UNBOUNDED (the key never expires, has no TTL, and the UNIQUE constraint is permanent) and the storage plugin MUST preserve this tuple permanently even when record bodies are purged/archived by retention per `plugin-spi.md`; verify the caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` is present per `cpt-cf-usage-collector-adr-mandatory-idempotency` (upstream validation has already enforced this; this step is defensive) - `inst-algo-dedup-compose-key`
2. [ ] - `p1` - Ensure the active `tracing::Span` / OpenTelemetry context (carrying the W3C `traceparent` / `tracestate` owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`) is current around the Plugin SPI call so the call participates in the end-to-end trace per `plugin-spi.md` §"Trace context propagation"; trace context is ambient, not an explicit parameter - `inst-algo-dedup-attach-trace`
3. [ ] - `p1` - **IF** the input is a single eligible-for-persist record - `inst-algo-dedup-single-branch`
   1. [ ] - `p1` - **TRY** invoke the Plugin SPI Method 1 single-record persist capability under the composite key — this single call atomically performs the dedup check and the persist via the storage plugin's UNIQUE `(tenant_id, metric_gts_id, idempotency_key)` enforcement per `plugin-spi.md` Method 1 - `inst-algo-dedup-single-call`
   2. [ ] - `p1` - **CATCH** plugin-error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) **RETURN** `spi-error { reason }` preserving the deterministic plugin-error taxonomy per `cpt-cf-usage-collector-principle-fail-closed`; no record is acknowledged - `inst-algo-dedup-single-catch`
   3. [ ] - `p1` - **IF** the SPI returned `PersistOutcome::Persisted { id }` **RETURN** `persisted { id }` - `inst-algo-dedup-single-persisted`
   4. [ ] - `p1` - **ELSE IF** the SPI returned `PersistOutcome::Deduplicated { id }` (ALL caller canonical fields equal — an exact-equality retry) **RETURN** `duplicate { id }` with the prior record's `id` (no second write was performed and counter totals are not inflated) per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-algo-dedup-single-dedup`
   5. [ ] - `p1` - **ELSE** the SPI returned `PersistOutcome::Conflict { id }` (ANY caller canonical field differs from the stored record); **RETURN** `conflict { id }` carrying the existing record's `id`, surfaced by the calling flow as the `idempotency_conflict` rejection — the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency` - `inst-algo-dedup-single-conflict`
4. [ ] - `p1` - **ELSE** the input is a batch of eligible-for-persist records (1..=100 per the wire-level cap from `cpt-cf-usage-collector-nfr-batch-and-report-timing`) - `inst-algo-dedup-batch-branch`
   1. [ ] - `p1` - **TRY** invoke the Plugin SPI Method 2 batch persist capability under the per-record composite key `(tenant_id, metric_gts_id, idempotency_key)` — the single SPI call drives the plugin's native bulk-write path and returns a per-record `Result<PersistOutcome, _>` list in input order per `plugin-spi.md` Method 2 - `inst-algo-dedup-batch-call`
   2. [ ] - `p1` - **CATCH** call-level plugin error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) - `inst-algo-dedup-batch-catch`
      1. [ ] - `p1` - Mark every eligible-for-persist record's per-record outcome `spi-error { reason }`; the failure is per-record and MUST NOT be surfaced as a whole-batch rollback per `cpt-cf-usage-collector-component-ingestion-gateway` per-record acceptance promise - `inst-algo-dedup-batch-fail-mark`
   3. [ ] - `p1` - **FOR EACH** SPI per-record result paired with its eligible-for-persist input index - `inst-algo-dedup-batch-foreach`
      1. [ ] - `p1` - **IF** the per-record result is `Ok(PersistOutcome::Persisted { id })` record `persisted { id }` - `inst-algo-dedup-batch-persisted`
      2. [ ] - `p1` - **ELSE IF** the per-record result is `Ok(PersistOutcome::Deduplicated { id })` (ALL caller canonical fields equal — an exact-equality retry) record `duplicate { id }` per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-algo-dedup-batch-dedup`
      3. [ ] - `p1` - **ELSE IF** the per-record result is `Ok(PersistOutcome::Conflict { id })` (ANY caller canonical field differs from the stored record) record `conflict { id }` carrying the existing record's `id`, surfaced by the calling flow as the `idempotency_conflict` rejection — the second write is NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency` - `inst-algo-dedup-batch-conflict`
      4. [ ] - `p1` - **ELSE** the per-record result is `Err(plugin-error)`; record `spi-error { reason }` preserving the deterministic plugin-error taxonomy - `inst-algo-dedup-batch-err`
5. [ ] - `p1` - **RETURN** the per-record outcome list in input order to the calling flow without caching any SPI result per `cpt-cf-usage-collector-principle-fail-closed` - `inst-algo-dedup-return`

### Metadata Size-Cap Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`

**Input**: A submitted `cpt-cf-usage-collector-entity-record-metadata` payload (key/value map; every key MUST be a declared member of the metric's `metadata_fields` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`; values are conveyed as `String` end-to-end) plus the configurable size cap (default 8 KiB per record per `cpt-cf-usage-collector-fr-record-metadata`; the exact cap and configuration key are operator-tunable per `cpt-cf-usage-collector-component-ingestion-gateway` responsibility scope). The closed-shape key-set check is performed before this algorithm; this algorithm enforces the orthogonal size cap.

**Output**: `valid` when the on-the-wire serialized size of `cpt-cf-usage-collector-entity-record-metadata` is at or below the configured cap; or `metadata-too-large` carrying the measured size and the configured cap so the caller can surface an actionable validation error per `cpt-cf-usage-collector-fr-record-metadata`. This algorithm MUST cite the configurable size cap with its default value of 8 KiB and MUST NOT mutate the `cpt-cf-usage-collector-entity-record-metadata` payload (per `sdk-trait.md` Method 1 invariant "Persist `metadata` byte-for-byte"; SPI MUST NOT silently truncate).

**Steps**:

1. [ ] - `p1` - Read the submitted `cpt-cf-usage-collector-entity-record-metadata` payload from the inbound `cpt-cf-usage-collector-entity-usage-record` without copying or mutating its contents - `inst-algo-metadata-read-input`
2. [ ] - `p1` - Read the configured size cap from operator configuration; the default value is 8 KiB per record per `cpt-cf-usage-collector-fr-record-metadata` and `cpt-cf-usage-collector-component-ingestion-gateway` responsibility scope - `inst-algo-metadata-read-cap`
3. [ ] - `p1` - Serialize `cpt-cf-usage-collector-entity-record-metadata` to its canonical on-the-wire representation (the same representation that the Plugin SPI will persist byte-for-byte per `cpt-cf-usage-collector-fr-record-metadata` and `plugin-spi.md` Method 1 invariant 2) - `inst-algo-metadata-serialize`
4. [ ] - `p1` - Measure the serialized size in bytes - `inst-algo-metadata-measure`
5. [ ] - `p1` - **IF** the measured size exceeds the configured cap **RETURN** `metadata-too-large` carrying both the measured bytes and the configured cap bytes so the calling flow can populate the actionable validation error envelope (`context.reason="metadata_size"`) per `cpt-cf-usage-collector-fr-record-metadata` - `inst-algo-metadata-exceeds`
6. [ ] - `p1` - **ELSE** **RETURN** `valid`; the payload is forwarded unmodified to the next stage (`cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`) per the byte-for-byte persistence invariant - `inst-algo-metadata-valid`

### Catalog Existence and Kind Lookup

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`

**Input**: A target Metric `gts_id` extracted from a per-record `cpt-cf-usage-collector-entity-usage-record` payload by `cpt-cf-usage-collector-component-ingestion-gateway`.

**Output**: A `found: true` shape descriptor carrying the resolved `cpt-cf-usage-collector-entity-metric-kind` (derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` — counter ⇐ `gts.cf.core.usage.counter.v1~`, gauge ⇐ `gts.cf.core.usage.gauge.v1~`) plus the metric's closed `metadata_fields: array<string>` (the declared metadata key set used by the ingest-time closed-shape check) when the `gts_id` is present in the gateway L1 cache populated from the metric-lifecycle Metric Catalog projection; or `found: false` when the `gts_id` is absent (the calling flow MUST surface this as the actionable not-found error envelope per `cpt-cf-usage-collector-fr-metric-existence-and-kind`). This algorithm MUST cite `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` and MUST NOT re-implement index access, MUST NOT dispatch the Plugin SPI for the catalog read on the latency-critical ingestion path, and MUST NOT bypass the gateway L1 cache — index ownership lives with the metric-lifecycle Metric Catalog component (managed via the Plugin SPI, persisted in the active storage plugin's database) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`.

**Steps**:

1. [ ] - `p1` - Read the target Metric `gts_id` from the calling pipeline at the `cpt-cf-usage-collector-component-ingestion-gateway` boundary without any Plugin SPI dispatch per `cpt-cf-usage-collector-nfr-ingestion-latency` - `inst-algo-catalog-read-input`
2. [ ] - `p1` - Delegate the lookup to `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` with the supplied `gts_id` — that algorithm owns the gateway L1 cache populated from the Metric Catalog (managed via the Plugin SPI, persisted in the active storage plugin's database) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` - `inst-algo-catalog-delegate`
3. [ ] - `p1` - **IF** the metric-lifecycle catalog-kind-lookup returns `not-found` (either because the projection is cold or because the `gts_id` is unregistered) **RETURN** `found: false` so the calling flow surfaces the actionable not-found error envelope per `cpt-cf-usage-collector-fr-metric-existence-and-kind` — this algorithm MUST NOT fall back to a direct Plugin SPI catalog read per `cpt-cf-usage-collector-principle-fail-closed` and MUST NOT trigger a projection refresh per metric-lifecycle ownership - `inst-algo-catalog-not-found`
4. [ ] - `p1` - **ELSE** the metric-lifecycle catalog-kind-lookup returned the (`cpt-cf-usage-collector-entity-metric-kind` derived from the `gts_id` prefix, plus the metric's `metadata_fields` declared key set) shape descriptor; **RETURN** `found: true` with that shape descriptor to the calling flow for consumption by `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` and the ingest-time closed-shape metadata-key check - `inst-algo-catalog-found`

## 4. States (CDSL)

### Usage Record Ingestion Lifecycle State Machine

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-state-usage-emission-usage-record-ingestion-lifecycle`

**States**: `submitted`, `validated`, `persisted`, `rejected-validation`, `deduplicated`, `spi-errored`

**Initial State**: `submitted`

**Transitions**:

1. [ ] - `p1` - **FROM** `submitted` **TO** `validated` **WHEN** `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`, `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`, `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`, and `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` all return their success outcome on the per-record path (mirrors `inst-emit-record-attrib-authz`, `inst-emit-record-catalog-lookup`, `inst-emit-record-kind-enforce`, `inst-emit-record-metadata-cap` in `cpt-cf-usage-collector-flow-usage-emission-emit-record` and the per-record equivalents `inst-emit-batch-record-pdp`, `inst-emit-batch-record-catalog`, `inst-emit-batch-record-kind`, `inst-emit-batch-record-metadata` in `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`) - `inst-state-usage-record-validated`
2. [ ] - `p1` - **FROM** `submitted` **TO** `rejected-validation` **WHEN** any of the gateway-side validation algorithms returns a deterministic rejection — the inbound `cpt-cf-usage-collector-entity-security-context` is missing (mirrors `inst-emit-record-missing-ctx`, `inst-emit-batch-missing-ctx`; surfaced as the canonical `Unauthenticated` `Problem` envelope), `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization` returns per-record `deny` (mirrors `inst-emit-record-pdp-deny`, `inst-emit-batch-record-deny`), `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` returns `not-found` (mirrors `inst-emit-record-metric-not-found`, `inst-emit-batch-record-unknown-metric`), `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` returns a counter-or-gauge invariant violation (mirrors `inst-emit-record-kind-invalid`, `inst-emit-batch-record-kind-invalid`), or `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement` returns `metadata-too-large` (mirrors `inst-emit-record-metadata-too-large`, `inst-emit-batch-record-metadata-too-large`); the SPI-dispatch stage joins the same `rejected-validation` disposition when `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` returns `PersistOutcome::Conflict { id }` for a same-key submission whose canonical fields differ (mirrors `inst-emit-record-conflict`, `inst-algo-dedup-single-conflict`, `inst-algo-dedup-batch-conflict`, `inst-emit-batch-record-conflict`), surfaced as `outcome="rejected"` with `context.reason="idempotency_conflict"` (AlreadyExists/409) carrying the existing record's `id` and NOT silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency`; the actionable error envelope is surfaced and no record is acknowledged per `cpt-cf-usage-collector-principle-fail-closed` - `inst-state-usage-record-rejected-validation`
3. [ ] - `p1` - **FROM** `validated` **TO** `persisted` **WHEN** `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` returns `PersistOutcome::Persisted { id }` from the Plugin SPI single-record or batch persist capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records` (mirrors `inst-emit-record-accepted`, `inst-algo-dedup-single-persisted`, `inst-algo-dedup-batch-persisted`, `inst-emit-batch-record-accepted`); the plugin-minted `id` is returned with `outcome="accepted"` per `usage-collector-v1.yaml` - `inst-state-usage-record-persisted`
4. [ ] - `p1` - **FROM** `validated` **TO** `deduplicated` **WHEN** `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` returns `PersistOutcome::Deduplicated { id }` from the same Plugin SPI dispatch under the composite key `(tenant_id, metric_gts_id, idempotency_key)` (mirrors `inst-emit-record-duplicate`, `inst-algo-dedup-single-dedup`, `inst-algo-dedup-batch-dedup`, `inst-emit-batch-record-dedup`); the prior record's `id` is returned with `outcome="duplicate"` and counter totals are not inflated per `cpt-cf-usage-collector-principle-idempotency-by-key` - `inst-state-usage-record-deduplicated`
5. [ ] - `p1` - **FROM** `validated` **TO** `spi-errored` **WHEN** `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` surfaces a Plugin SPI transport / readiness / persistence error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) — mirrors `inst-emit-record-spi-fail`, `inst-algo-dedup-single-catch`, `inst-algo-dedup-batch-fail-mark`, `inst-emit-batch-record-spi-err`; the per-record outcome is `rejected` (`context.reason="plugin_readiness"`) with the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` preserved, no record is acknowledged, and no whole-batch rollback occurs per `cpt-cf-usage-collector-component-ingestion-gateway` per-record acceptance promise - `inst-state-usage-record-spi-error`

## 5. Definitions of Done

### FR: Ingestion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion`

The system **MUST** expose `POST /usage-collector/v1/records` as the single contract-first write path for at-least-once ingestion of usage records from authenticated source gears, accept an `IngestRecordsRequest` carrying 1..100 `cpt-cf-usage-collector-entity-usage-record` payloads (single or batched) per `usage-collector-v1.yaml`, route every submission through `cpt-cf-usage-collector-component-ingestion-gateway`, and end the synchronous path with persistence through the Plugin Host into `cpt-cf-usage-collector-dbtable-usage-records` per `cpt-cf-usage-collector-seq-emit-usage` — surfacing deterministic per-record acknowledgements (`accepted`, `duplicate`, or `rejected` with a per-record `error: Problem` envelope) in input order with no whole-batch rollback.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-seq-emit-usage`

**Constraints**: `cpt-cf-usage-collector-fr-ingestion`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### FR: Idempotency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-idempotency`

The system **MUST** require a caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` on every record and dedup retried submissions via the Plugin SPI composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records` UNIQUE constraint; the dedup check and the persist MUST be a single SPI capability (Method 1 `persist_usage_record` for single, Method 2 `persist_usage_records` for batch) and MUST NOT be split into separate non-transactional calls. The system **MUST** distinguish two same-key outcomes per the plugin's canonical-field equality comparison: (a) an EXACT-EQUALITY retry — where ALL caller canonical fields (`value`, `timestamp`, `cpt-cf-usage-collector-entity-resource-ref`, `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, and `cpt-cf-usage-collector-entity-record-metadata`) match the stored record — dedups silently, returning `outcome="duplicate"` carrying the prior record's `id` without performing a second write and without inflating counter totals per `cpt-cf-usage-collector-principle-idempotency-by-key`; and (b) a same-key submission with ANY canonical-field mismatch (including a metadata-only difference) MUST be rejected with `outcome="rejected"` and a per-record `error: Problem` (`context.reason="idempotency_conflict"`, AlreadyExists/409) carrying the existing record's `id` — the second write MUST NOT be silently absorbed per `cpt-cf-usage-collector-adr-mandatory-idempotency`. (c) The idempotency window **MUST** be UNBOUNDED — the `cpt-cf-usage-collector-entity-idempotency-key` never expires, has no TTL, and the UNIQUE `(tenant_id, metric_gts_id, idempotency_key)` constraint is permanent — and the storage plugin **MUST** preserve the `(tenant_id, metric_gts_id, idempotency_key)` tuple permanently even when record bodies are purged/archived by retention (retention/purge MUST NOT free a dedup key) per `plugin-spi.md`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-mandatory-idempotency`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `IdempotencyKey`, `UsageRecord`

### FR: Record Metadata

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-record-metadata`

The system **MUST** enforce a **closed-shape** check on the `cpt-cf-usage-collector-entity-record-metadata` payload at the Ingestion Gateway before any Plugin SPI dispatch: every key in the submitted metadata MUST be a declared member of the metric's `metadata_fields` (resolved from the gateway L1 cache populated from the Metric Catalog per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) and every value is conveyed as `String` end-to-end; any undeclared key MUST be rejected with the `unknown_metadata_key` error name (REST `context.reason="unknown_metadata_key"`; SDK `UsageCollectorError::UnknownMetadataKey`) lifting to AIP-193 `InvalidArgument` / HTTP `400` carrying `context.key` and `instance_path` (e.g. `/metadata/{key}`) per `usage-collector-v1.yaml`. The system **MUST** also enforce a configurable size cap on the conforming payload (default 8 KiB per record) at the Ingestion Gateway before any Plugin SPI dispatch, surface oversize records with the actionable validation error envelope (`context.reason="metadata_size"`) carrying the measured size and the configured cap per `usage-collector-v1.yaml`, and persist conforming payloads byte-for-byte through the Plugin SPI (no silent truncation, no rewriting, no interpretation of declared-key values) per the `sdk-trait.md` Method 1 invariant and `plugin-spi.md` Method 1 invariant 2. There is no free-form remainder and no preserved extras — undeclared keys are validation errors, not silently-stored extras.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`

**Constraints**: `cpt-cf-usage-collector-fr-record-metadata`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `RecordMetadata`

### FR: Counter Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-counter-semantics`

The system **MUST** enforce counter non-negativity at the Ingestion Gateway before any Plugin SPI dispatch — rejecting any counter-kind record whose submitted `value` is below zero — by consulting the `cpt-cf-usage-collector-entity-metric-kind` derived from the metric's `gts_id` prefix (`gts.cf.core.usage.counter.v1~`) per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` through `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` and surfacing the actionable validation error envelope (`context.reason="kind_invariant"`) per `usage-collector-v1.yaml` so the §3.7 referential rule that counter `cpt-cf-usage-collector-dbtable-usage-records` rows MUST have `value >= 0` is upheld.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`

**Constraints**: `cpt-cf-usage-collector-principle-kind-enforcement`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `MetricKind`, `UsageRecord`

### FR: Gauge Semantics

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-gauge-semantics`

The system **MUST** accept gauge-kind records as point-in-time values stored as-is — no delta accumulation, no rewriting, no server-side shape rewriting — per DESIGN §3.1 and `cpt-cf-usage-collector-fr-gauge-semantics` (`cpt-cf-usage-collector-entity-metric-kind` derived from the metric's `gts_id` prefix `gts.cf.core.usage.gauge.v1~` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`), preserving the gauge replacement semantic that the most recent accepted value supersedes prior values for the same `(tenant_id, metric_gts_id)` pair without delta arithmetic, and rejecting any `entry_type=compensation` submission against a gauge metric per the locked (`MetricKind` × `EntryType`) value matrix in `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`

**Constraints**: `cpt-cf-usage-collector-principle-kind-enforcement`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `MetricKind`, `UsageRecord`

### FR: Tenant Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-tenant-attribution`

The system **MUST** treat tenant attribution as caller-supplied in the request payload as the mandatory `tenant_id` field of `UsageRecordInput` per `usage-collector-v1.yaml` rather than server-synthesized or inferred from the inbound `cpt-cf-usage-collector-entity-security-context`, include the caller-supplied `tenant_id` in the per-record attribution tuple sent to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper inside `cpt-cf-usage-collector-component-ingestion-gateway` (which authorizes the caller's `cpt-cf-usage-collector-entity-security-context` for the requested `tenant_id`), materialize the caller-supplied `tenant_id` byte-identical on every persisted record as the NOT NULL `tenant_id` column of `cpt-cf-usage-collector-dbtable-usage-records`, and refuse any ingestion attempt whose attribution tuple cannot be authorized per `cpt-cf-usage-collector-adr-caller-supplied-attribution` and the PDP permit/deny outcome.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-caller-supplied-attribution`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `SecurityContext`, `UsageRecord`

### FR: Resource Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-resource-attribution`

The system **MUST** require a mandatory caller-supplied `cpt-cf-usage-collector-entity-resource-ref` (composite `resource_id` plus `resource_type`) on every `cpt-cf-usage-collector-entity-usage-record` payload, materialize the components on every persisted record as the `resource_id` and `resource_type` columns of `cpt-cf-usage-collector-dbtable-usage-records` per the §3.7 NOT NULL constraints, include the resource attribution in the per-record attribution tuple sent to `cpt-cf-usage-collector-flow-foundation-pdp-authorize`, and never synthesize resource attribution server-side per `cpt-cf-usage-collector-adr-caller-supplied-attribution`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-caller-supplied-attribution`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `ResourceRef`, `UsageRecord`

### FR: Subject Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-subject-attribution`

The system **MUST** treat `cpt-cf-usage-collector-entity-subject-ref` as caller-supplied and optional — `subject_id` is the only mandatory component when subject attribution is supplied, `subject_type` is optional and MUST NOT be supplied without `subject_id` per the §3.7 nullable-column rules — materialize the supplied components as the `subject_id` and `subject_type` columns of `cpt-cf-usage-collector-dbtable-usage-records`, omit the entity entirely for system-level consumption, include subject attribution in the per-record attribution tuple sent to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` when present, and never synthesize subject attribution server-side per `cpt-cf-usage-collector-adr-caller-supplied-attribution`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-caller-supplied-attribution`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `SubjectRef`, `UsageRecord`

### FR: Ingestion Authorization

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion-authorization`

The system **MUST** accept an inbound `cpt-cf-usage-collector-entity-security-context` at both ingestion entry points — on REST as `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`), on the SDK trait as `ctx: &SecurityContext` first parameter to `UsageCollectorClientV1::submit_usage_record` / `submit_usage_records` per `sdk-trait.md` Methods 1 and 2 — and authorize every ingestion call by dispatching per-attribution-tuple PDP authorization to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` through the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) inside `cpt-cf-usage-collector-component-ingestion-gateway` against the full attribution tuple (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`); fail closed when the inbound `cpt-cf-usage-collector-entity-security-context` is missing (canonical `Unauthenticated` `Problem` envelope per the `usage-collector-v1.yaml` `default` response) or when the PDP resolver is unavailable (no synthesized identity, no cached PDP decision); surface per-tuple PDP `deny` decisions as per-record `outcome="rejected"` with `context.reason="authz"` inside `IngestRecordsResponse` — never as whole-request rejection, because PDP authorization is per attribution tuple and there is no envelope-level PDP deny aggregation — without any Plugin SPI dispatch in either case.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-principle-fail-closed`

**Touches**:

- API: `POST /usage-collector/v1/records`
- Component: `cpt-cf-usage-collector-component-ingestion-gateway`
- Entities: `SecurityContext`

### FR: Data Quality

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-fr-data-quality`

The system **MUST** preserve data quality at ingestion through the five product-level guarantees of PRD §5.8 — (1) **Accuracy**: kind-invariant enforcement via `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` (MetricKind derived from the `gts_id` prefix per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`) and idempotency via `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch` prevent negative-delta poisoning and duplicate counter inflation; (2) **Completeness**: mandatory caller-supplied attribution (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, `source_gear`, Metric `gts_id`, `cpt-cf-usage-collector-entity-idempotency-key`) per `usage-collector-v1.yaml` `UsageRecordInput.required`, with structural validation rejecting any submission missing these fields before Plugin SPI dispatch; (3) **Freshness**: end-to-end ingestion latency bounded by `cpt-cf-usage-collector-nfr-ingestion-latency` with no parallel ingestion path that could lag this guarantee; (4) **Validation**: gateway-side structural attribution, closed-shape `cpt-cf-usage-collector-entity-record-metadata` key-set validation against the metric's declared `metadata_fields`, and `cpt-cf-usage-collector-entity-record-metadata` size-cap validation per `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`, plus Metric existence and `cpt-cf-usage-collector-entity-metric-kind` resolution exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` against the in-process Metrics Catalog projection (no Plugin SPI catalog read on the hot path, no projection refresh) surfaced as the actionable error envelope (`context.reason` in {`unknown_metric`, `kind_invariant`, `unknown_metadata_key`, `metadata_size`}) per `usage-collector-v1.yaml`; (5) **Cleansing**: once accepted, raw usage records MUST NOT be silently amended by the gear — `cpt-cf-usage-collector-entity-usage-record` is append-only after acceptance per DESIGN §3.1, and corrections are expressed as deactivation (owned by `cpt-cf-usage-collector-feature-event-deactivation`) plus a fresh idempotency-keyed re-emission.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`
- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-fr-data-quality`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Component: `cpt-cf-usage-collector-component-metric-catalog`
- Entities: `UsageRecord`, `RecordMetadata`, `Metric`, `MetricKind`, `IdempotencyKey`

### FR: Usage Compensation — Flow

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-compensation-flow`

The system **MUST** accept counter value-reversal records on the **same unified ingestion path** as ordinary usage emission (`POST /usage-collector/v1/records` and the SDK emit operation routed through `cpt-cf-usage-collector-component-ingestion-gateway`) without introducing a dedicated `compensate` REST path, SDK method, or Plugin SPI call per `cpt-cf-usage-collector-adr-usage-compensation` and DESIGN §3.3 "Unified ingestion request shape"; persist accepted compensation records with `entry_type = compensation` (per `cpt-cf-usage-collector-entity-entry-type`), a strictly-negative signed `value`, and a non-empty `corrects_id` referencing the target `entry_type = usage` row, under the same PDP attribution (`cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper) and the same mandatory caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` (`cpt-cf-usage-collector-adr-mandatory-idempotency`) that govern ordinary ingestion; surface the per-record acknowledgement (`outcome="accepted"` / `outcome="duplicate"` / `outcome="rejected"`) in the same `IngestRecordsResponse` shape. Recording a caller-supplied signed-negative `value` is recording, not computing — the system MUST NOT compute refunds, credits, credit-notes, or quota per `cpt-cf-usage-collector-constraint-no-business-logic`; mandatory idempotency prevents double-refund for free.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-compensation`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-fr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `EntryType`

### FR: Usage Compensation — Value Matrix

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-value-matrix`

The system **MUST** enforce the locked four-cell `(MetricKind × EntryType)` value matrix at validation time via `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` before any Plugin SPI persist dispatch: `counter + usage` requires `value >= 0` (unchanged); `counter + compensation` requires `value < 0` (strictly negative — zero is not accepted); `gauge + usage` accepts any signed value (unchanged); `gauge + compensation` is **REJECTED** before persistence (gauges have no `SUM` semantics; the only correction for a gauge is deactivation per `cpt-cf-usage-collector-adr-monotonic-deactivation`). Value-sign violations of the `counter+usage` (`value < 0`) and `counter+compensation` (`value >= 0`) cells surface as the per-record `outcome="rejected"` with `context.reason="kind_invariant"` per `usage-collector-v1.yaml`; the `gauge + compensation` cell surfaces with `context.reason="gauge_compensation_rejected"` (HTTP `422`) per the locked five-code compensation taxonomy in `usage-collector-v1.yaml` and the SDK trait `GaugeCompensationRejected` variant — it is NEVER collapsed into the generic `kind_invariant` code.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-flow-usage-emission-compensation`

**Constraints**: `cpt-cf-usage-collector-fr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `MetricKind`, `EntryType`, `UsageRecord`

### FR: Usage Compensation — L1 corrects_id

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-corrects-id-l1`

The system **MUST** enforce the L1 `corrects_id` referential rule synchronously at ingestion via `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`: (1) when `entry_type = compensation` the request MUST carry a non-empty `corrects_id` (missing `corrects_id` violates the `counter+compensation` matrix-cell shape and surfaces `context.reason="kind_invariant"`); (2) when `entry_type = usage` the request MUST NOT carry a `corrects_id` (presence violates the `*+usage` matrix-cell shape and also surfaces `context.reason="kind_invariant"`); (3) the referenced row MUST exist; (4) the referenced row MUST have `entry_type = usage` (compensating a compensation is a non-goal per `cpt-cf-usage-collector-adr-usage-compensation`); (5) the referenced row MUST share `(tenant_id, metric_gts_id)` with the incoming compensation (cross-tenant or cross-metric compensation is rejected); (6) the referenced row MUST be `status = active`. Failures (3)-(6) surface as `outcome="rejected"` with one of the four granular wire codes from the locked `usage-collector-v1.yaml` Problem.context.reason taxonomy: rule (3) → `corrects_id_not_found` (HTTP `404`); rule (4) → `corrects_id_wrong_entry_type` (HTTP `409`); rule (5) → `corrects_id_wrong_scope` (HTTP `409`); rule (6) → `corrects_id_inactive` (HTTP `409`) — these codes are NEVER collapsed into a single generic `corrects_id_invalid` code (no such code is declared in the OpenAPI taxonomy or in `sdk-trait.md`). The mapping from algorithm outcomes to these wire codes is the contract recorded in the algorithm's "Algorithm outcome → wire `context.reason` mapping" table. The system MUST NOT track L2 per-record remaining amounts, lots, FIFO/LIFO ordering, or non-negative net per `cpt-cf-usage-collector-constraint-no-business-logic`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-flow-usage-emission-compensation`

**Constraints**: `cpt-cf-usage-collector-fr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `EntryType`

### FR: Usage Compensation — Concurrency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-compensation-concurrency`

The system **MUST** reject a compensation whose referenced `usage` row is mid-deactivation via the L1 "referenced record MUST be `active`" check inside `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` — there is no quarantine, no retry queue, no compensating cascade for the rejection, and no additional distributed-coordination machinery is added at the gateway. A compensation referencing a row R that arrives while R is being deactivated is rejected by the same L1 "active" check that handles fully-inactive references; the source gear retries at its own discretion and the mandatory idempotency key makes those retries safe per `cpt-cf-usage-collector-adr-mandatory-idempotency` (the depth-1 cascade itself is owned by `cpt-cf-usage-collector-feature-event-deactivation` and `cpt-cf-usage-collector-adr-monotonic-deactivation`, not by this feature).

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-flow-usage-emission-compensation`

**Constraints**: `cpt-cf-usage-collector-fr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `EntryType`, `DeactivationStatus`

### FR: Usage Compensation — No Business Logic

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-compensation-no-business-logic`

The system **MUST NOT** compute refunds, credits, credit-notes, quota, lot/FIFO-LIFO state, or per-record remaining amounts when recording a compensation. Recording a caller-supplied signed-negative `value` with `entry_type = compensation` is **recording, not computing** — the source gear owns the business decision to give back capacity (capacity refund, partial cancellation, dispute resolution, billing-period correction); the Usage Collector validates the four-cell matrix + the L1 `corrects_id` rule and persists the row as-supplied. The system MUST NOT validate non-negative net at write time and MUST NOT emit a negative-net detection signal per the un-policed-net stance recorded in DESIGN §3.10.3; downstream consumers (billing, quota, FinOps) own any "net can't be negative" policy. Mandatory idempotency prevents double-refund for free per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-compensation`
- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `EntryType`

### NFR: Throughput

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-throughput`

The system **MUST** sustain the target steady-state ingestion throughput floor declared by `cpt-cf-usage-collector-nfr-throughput` end-to-end through `cpt-cf-usage-collector-component-ingestion-gateway` and the Plugin SPI persist capability, with no degradation under continuous load and no throughput regression introduced by per-record validation (attribution + PDP authorization, catalog lookup, kind enforcement, metadata size-cap enforcement) or per-record SPI dispatch on the synchronous write path.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-constraint-nfr-thresholds`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### NFR: Throughput Profile

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-throughput-profile`

The system **MUST** preserve the `cpt-cf-usage-collector-nfr-throughput` floor under the mixed counter/gauge workload profile declared by `cpt-cf-usage-collector-nfr-throughput-profile` so that neither kind starves the other under contention — `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` MUST distinguish the counter and gauge branches deterministically without introducing kind-dependent backpressure asymmetry at the Ingestion Gateway, and the Plugin SPI dispatch path MUST treat both kinds uniformly through the single composite key contract per `cpt-cf-usage-collector-dbtable-usage-records`.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`

**Constraints**: `cpt-cf-usage-collector-constraint-nfr-thresholds`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### NFR: Ingestion Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-ingestion-latency`

The system **MUST** hold the per-record ingestion-latency budget declared by `cpt-cf-usage-collector-nfr-ingestion-latency` end-to-end through `cpt-cf-usage-collector-component-ingestion-gateway`, the per-component `authz_scope` helper invocation against `cpt-cf-usage-collector-contract-authz-resolver`, the metric-lifecycle in-process Metrics Catalog projection, and the Plugin SPI persist capability — never round-tripping the Plugin SPI for the Metric existence-and-kind lookup on the synchronous write path per `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` — and report it through the `usage_collector.ingestion.latency` histogram (declared with `with_unit("s")`) whose bucket layout brackets the published 200 ms p95 budget per DESIGN §3.11.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-constraint-nfr-thresholds`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### NFR: Workload Isolation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-workload-isolation`

The system **MUST** isolate the synchronous ingestion path from the read-side query path and from operator-side Metric and deactivation paths so that sustained read or lifecycle workload does not degrade ingestion latency or throughput beyond the `cpt-cf-usage-collector-nfr-ingestion-latency` and `cpt-cf-usage-collector-nfr-throughput` budgets — `cpt-cf-usage-collector-component-ingestion-gateway` MUST be the sole entry point for the write path, with no shared mutable state or shared backpressure with the read-side or operator-side gateways beyond the single Plugin Host dispatch surface.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-nfr-workload-isolation`

**Touches**:

- Component: `cpt-cf-usage-collector-component-ingestion-gateway`

### NFR: Batch and Report Timing

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-batch-and-report-timing`

The system **MUST** enforce the wire-level per-call batch cap of 100 records on `POST /usage-collector/v1/records` (`IngestRecordsRequest.records` `maxItems: 100`, `minItems: 1` per `usage-collector-v1.yaml`) at `cpt-cf-usage-collector-component-ingestion-gateway` before any per-record processing, reject oversize or empty batches with the request-level structural validation `Problem` envelope (HTTP `400`) per `usage-collector-v1.yaml`, surface per-record outcomes in input order under HTTP `200` (all accepted/duplicated) or HTTP `207 Multi-Status` (mixed or all-rejected — i.e., whenever ≥1 record is rejected, single-record conflict included), and ensure no whole-batch rollback occurs on per-record failure per `cpt-cf-usage-collector-component-ingestion-gateway` per-record acceptance promise.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-nfr-batch-and-report-timing`

**Touches**:

- API: `POST /usage-collector/v1/records`

### NFR: Availability Boundary

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-nfr-availability-boundary`

The system **MUST** keep `POST /usage-collector/v1/records` available whenever the foundation-owned Plugin Host structural readiness fact holds (selector cached AND `ClientHub::try_get_scoped::<dyn UsageCollectorPluginV1>` returns `Some`,) and the `cpt-cf-usage-collector-contract-authz-resolver` client is reachable — surfacing structural plugin-unavailability (`try_get_scoped` returns `None` lifted to a per-call `plugin-unavailable` error; the SPI exposes no `Unready` variant) and Plugin SPI transport / persistence errors (`Timeout`, `BackendError`, `ContractViolation`) as per-record `rejected` outcomes (`context.reason="plugin_readiness"`) with the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` preserved, never as whole-batch rollback, and never inventing storage bindings on failure per `cpt-cf-usage-collector-principle-fail-closed` and the `cpt-cf-usage-collector-component-ingestion-gateway` responsibility boundary.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-nfr-availability-boundary`

**Touches**:

- Component: `cpt-cf-usage-collector-component-ingestion-gateway`

### Principle: Idempotency by Key

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-principle-idempotency-by-key`

The system **MUST** uphold idempotency-by-key as the at-least-once delivery contract: every record carries a caller-supplied `cpt-cf-usage-collector-entity-idempotency-key`, the dedup boundary is the Plugin SPI composite key `(tenant_id, metric_gts_id, idempotency_key)` per `cpt-cf-usage-collector-dbtable-usage-records`, and EXACT-EQUALITY retries sharing the composite (ALL caller canonical fields equal) return the prior record's `id` with `outcome="duplicate"` without a second write so counter totals MUST NOT be inflated by retries — uniformly across counter and gauge kinds. Silent absorb (`Deduplicated` → `duplicate`) is reserved EXCLUSIVELY for exact-equality retries: a same-key submission whose canonical fields differ MUST instead be surfaced as `outcome="rejected"` with `context.reason="idempotency_conflict"` (AlreadyExists/409) carrying the existing record's `id` and MUST NOT be silently dropped, per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-principle-idempotency-by-key`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `IdempotencyKey`

### Principle: Kind Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-principle-kind-enforcement`

The system **MUST** enforce `cpt-cf-usage-collector-entity-metric-kind` invariants at `cpt-cf-usage-collector-component-ingestion-gateway` before any Plugin SPI dispatch — counter records MUST satisfy non-negative `value` per §3.7, gauge records are accepted as-is per DESIGN §3.1 — by consuming the authoritative kind through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` and never re-deriving Metric Kind locally per `cpt-cf-usage-collector-principle-kind-enforcement`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-principle-kind-enforcement`

**Touches**:

- API: `POST /usage-collector/v1/records`
- Component: `cpt-cf-usage-collector-component-metric-catalog`
- Entities: `MetricKind`, `UsageRecord`

### Principle: Fail Closed

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-principle-fail-closed`

The system **MUST** fail closed on every boundary or downstream-resolver or Plugin SPI unavailability: when the inbound `cpt-cf-usage-collector-entity-security-context` is missing at the handler boundary (REST handler did not receive `Extension<SecurityContext>` from ToolKit gateway middleware, or the SDK trait was invoked without a `ctx` argument) the ingestion path MUST return the canonical `Unauthenticated` `Problem` envelope without any PDP call or record dispatch; when the `cpt-cf-usage-collector-contract-authz-resolver` PDP resolver is unreachable or denies, `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (invoked via the per-component `authz_scope` helper inside `cpt-cf-usage-collector-component-ingestion-gateway`) propagates the canonical `PermissionDenied` `Problem` envelope without record dispatch; when the Plugin SPI surfaces transport / readiness / persistence errors the per-record outcome is `rejected` (`context.reason="plugin_readiness"`) with no acknowledged record — no synthesized identity, no cached PDP decision, and no invented storage binding per the `cpt-cf-usage-collector-component-ingestion-gateway` boundary.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-principle-fail-closed`

**Touches**:

- API: `POST /usage-collector/v1/records`
- Component: `cpt-cf-usage-collector-component-ingestion-gateway`

### Principle: Pluggable Storage

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-principle-pluggable-storage`

The system **MUST** dispatch every persistence call through the foundation-owned Plugin Host and the Plugin SPI single-record (Method 1 `persist_usage_record`) or batch (Method 2 `persist_usage_records`) capability — never embedding backend-specific SQL, schema, or client code in `cpt-cf-usage-collector-component-ingestion-gateway`, never opening a parallel storage path, and never inventing a binding when the registry or orchestrator is unreachable — so that the active storage plugin can be swapped via operator configuration without touching the ingestion path per `cpt-cf-usage-collector-principle-pluggable-storage`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-principle-pluggable-storage`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### Constraint: No Business Logic

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-constraint-no-business-logic`

The system **MUST** keep the ingestion path free of billing, pricing, quota enforcement, and per-Metric payload-content interpretation — `cpt-cf-usage-collector-component-ingestion-gateway` MUST NOT interpret `cpt-cf-usage-collector-entity-record-metadata` content, MUST NOT apply any per-tenant or per-Metric accounting transform, and MUST NOT mutate the `value` field beyond kind-invariant rejection — every business rule is owned by source gears and downstream consumers per `cpt-cf-usage-collector-constraint-no-business-logic`.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`

**Constraints**: `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `RecordMetadata`, `UsageRecord`

### Constraint: NFR Thresholds

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-constraint-nfr-thresholds`

The system **MUST** hold the PRD-declared NFR thresholds end-to-end on the ingestion path — `cpt-cf-usage-collector-nfr-throughput`, `cpt-cf-usage-collector-nfr-throughput-profile`, `cpt-cf-usage-collector-nfr-ingestion-latency`, `cpt-cf-usage-collector-nfr-workload-isolation`, `cpt-cf-usage-collector-nfr-batch-and-report-timing`, and `cpt-cf-usage-collector-nfr-availability-boundary` — surfacing them through the `usage_collector.ingestion.requests` counter and the `usage_collector.ingestion.latency` histogram (declared with `with_unit("s")`) as the operator-side instruments per DESIGN §3.11 so each threshold is independently observable per `cpt-cf-usage-collector-constraint-nfr-thresholds`.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-constraint-nfr-thresholds`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### ADR: Caller-supplied Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-adr-caller-supplied-attribution`

The system **MUST** consume `tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, and `source_gear` exclusively from the caller — resolved through the foundation-owned `cpt-cf-usage-collector-entity-security-context` for tenant scope and carried verbatim on the per-record attribution tuple for resource, subject, and source-gear attribution — and MUST NOT synthesize any of these fields server-side, MUST NOT derive them from headers other than the caller-bound credential material, and MUST NOT permit operator overrides on the ingestion path per `cpt-cf-usage-collector-adr-caller-supplied-attribution`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-caller-supplied-attribution`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `SecurityContext`, `ResourceRef`, `SubjectRef`

### ADR: Mandatory Idempotency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-adr-mandatory-idempotency`

The system **MUST** require a caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` on every `cpt-cf-usage-collector-entity-usage-record` payload at the wire level — `idempotency_key` is a NOT NULL column on `cpt-cf-usage-collector-dbtable-usage-records` per §3.7 — reject submissions missing the key with the actionable validation error envelope before any Plugin SPI dispatch, dedup retries under the Plugin SPI composite `(tenant_id, metric_gts_id, idempotency_key)`, and surface duplicate outcomes uniformly across counter and gauge kinds so retries never inflate counter totals or poison gauge point-in-time signals per `cpt-cf-usage-collector-adr-mandatory-idempotency`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-adr-mandatory-idempotency`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `IdempotencyKey`

### Component: Ingestion Gateway

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-component-ingestion-gateway`

The system **MUST** realize `cpt-cf-usage-collector-component-ingestion-gateway` as the sole synchronous write entry point for usage records (REST and SDK, single and batched), owning the ingestion contract end-to-end — SecurityContext acceptance at both entry points (REST handler with `Extension<SecurityContext>` from ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK trait `submit_usage_record(ctx, ...)` / `submit_usage_records(ctx, ...)` with `ctx: &SecurityContext` as the first parameter), per-component PDP enforcement via the `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`), structural attribution-tuple validation, mandatory idempotency-key requirement, kind-dependent invariants resolved against the metric-lifecycle Metrics Catalog projection, configurable `cpt-cf-usage-collector-entity-record-metadata` size-cap enforcement, deterministic per-record acknowledgements — while delegating persistence to `cpt-cf-usage-collector-component-plugin-host` and Metric existence/kind lookup to `cpt-cf-usage-collector-component-metric-catalog`, with no PDP-decision caching, no synthesized identities, no invented storage bindings, and no interpretation of `cpt-cf-usage-collector-entity-record-metadata` content per DESIGN §3.2.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`
- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`
- `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-component-ingestion-gateway`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### Sequence: Emit Usage Record

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-seq-emit-usage`

The system **MUST** implement the `cpt-cf-usage-collector-seq-emit-usage` sequence end-to-end: caller surface (REST handler receiving `Extension<SecurityContext>` from ToolKit gateway middleware, or SDK trait `submit_usage_record(ctx, ...)` / `submit_usage_records(ctx, ...)` with `ctx: &SecurityContext` first) → Ingestion Gateway per-component PDP authorization via the `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver` → Ingestion Gateway dispatch → Metrics Catalog existence/kind lookup → Plugin Host → storage plugin persist under the composite key `(tenant_id, metric_gts_id, idempotency_key)` → per-record acknowledgement, with PDP denial, unknown Metric, kind-invariant violation, oversize metadata, and SPI errors rejecting or marking per-record outcomes without any whole-batch rollback per DESIGN §3.6.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-seq-emit-usage`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### Data: usage_records Table

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-dbtable-usage-records`

The system **MUST** persist `cpt-cf-usage-collector-entity-usage-record` rows in `cpt-cf-usage-collector-dbtable-usage-records` exactly per the DESIGN §3.7 row shape — `id` (PK, plugin-minted on accept), `tenant_id`, `resource_id`, `resource_type`, `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, `status`, and `metadata` NOT NULL; `subject_id` and `subject_type` nullable with `subject_type` not allowed without `subject_id`; UNIQUE on `(tenant_id, metric_gts_id, idempotency_key)` enforcing dedup per `cpt-cf-usage-collector-fr-idempotency`; counter-kind rows MUST have `value >= 0` per `cpt-cf-usage-collector-fr-counter-semantics`; logical reference `metric_gts_id → metric_catalog.gts_id` per the §3.7 referential rule — as the sole writer of that table.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-dbtable-usage-records`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `RecordMetadata`

### Entity: Usage Record

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-usage-record`

The system **MUST** treat `cpt-cf-usage-collector-entity-usage-record` per DESIGN §3.1 as a single attributed measurement of resource consumption carrying `value`, `timestamp`, attribution tuple (`tenant_id`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`), caller-supplied `cpt-cf-usage-collector-entity-idempotency-key`, `status`, and optional `cpt-cf-usage-collector-entity-record-metadata`; the entity MUST be append-only after acceptance except for the `status` transition owned by the deactivation feature, and every field carried on the entity at acceptance time MUST be materialized verbatim on the corresponding `cpt-cf-usage-collector-dbtable-usage-records` row.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`

**Constraints**: `cpt-cf-usage-collector-entity-usage-record`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### Entity: Record Metadata

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-record-metadata`

The system **MUST** treat `cpt-cf-usage-collector-entity-record-metadata` per DESIGN §3.1 as an optional opaque JSON object carried verbatim on a `cpt-cf-usage-collector-entity-usage-record` — never indexed, never aggregated, never interpreted by `cpt-cf-usage-collector-component-ingestion-gateway` or the Plugin Host — persisted byte-for-byte through the Plugin SPI per the `sdk-trait.md` Method 1 invariant and `plugin-spi.md` Method 1 invariant 2, and bounded by the configurable size cap (default 8 KiB) enforced before any SPI dispatch.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-metadata-size-cap-enforcement`

**Constraints**: `cpt-cf-usage-collector-entity-record-metadata`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `RecordMetadata`

### Entity: Resource Ref

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-resource-ref`

The system **MUST** treat `cpt-cf-usage-collector-entity-resource-ref` per DESIGN §3.1 as a caller-supplied composite identifying the attributed resource — mandatory `resource_id` plus mandatory `resource_type` — required on every `cpt-cf-usage-collector-entity-usage-record` payload, materialized on every persisted row as the `resource_id` and `resource_type` columns of `cpt-cf-usage-collector-dbtable-usage-records` (NOT NULL per §3.7), and forwarded as part of the per-record attribution tuple to `cpt-cf-usage-collector-flow-foundation-pdp-authorize`.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-entity-resource-ref`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `ResourceRef`

### Entity: Subject Ref

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-subject-ref`

The system **MUST** treat `cpt-cf-usage-collector-entity-subject-ref` per DESIGN §3.1 as a caller-supplied subject attribution — mandatory `subject_id` plus optional `subject_type` when supplied; omitted entirely for system-level consumption — and per the §3.7 nullable rules MUST NOT accept `subject_type` without `subject_id`, materializing the supplied components verbatim on the persisted row as the nullable `subject_id` and `subject_type` columns of `cpt-cf-usage-collector-dbtable-usage-records`, and forwarding subject attribution as part of the per-record attribution tuple to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` when present.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-entity-subject-ref`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `SubjectRef`

### Entity: Idempotency Key

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-idempotency-key`

The system **MUST** treat `cpt-cf-usage-collector-entity-idempotency-key` per DESIGN §3.1 as a caller-supplied opaque identifier that deduplicates retried submissions uniformly across counter and gauge kinds, require its presence on every `cpt-cf-usage-collector-entity-usage-record` payload per `cpt-cf-usage-collector-adr-mandatory-idempotency`, materialize it on every persisted row as the NOT NULL `idempotency_key` column of `cpt-cf-usage-collector-dbtable-usage-records`, and participate in the Plugin SPI composite UNIQUE key `(tenant_id, metric_gts_id, idempotency_key)` that drives dedup per `cpt-cf-usage-collector-principle-idempotency-by-key`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-idempotency-dedup-dispatch`
- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-entity-idempotency-key`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `IdempotencyKey`

### Entity: Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-metric`

The system **MUST** consume `cpt-cf-usage-collector-entity-metric` per DESIGN §3.1 as a read-only catalog reference on the ingestion path — the ingestion path NEVER mutates Metrics and NEVER triggers projection refresh — resolving Metric existence and the (`cpt-cf-usage-collector-entity-metric-kind`, optional `unit`) shape descriptor for a target `gts_id` exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` against the in-process Metrics Catalog projection owned by `cpt-cf-usage-collector-component-metric-catalog`, with cold-projection states surfaced as `not-found` per `cpt-cf-usage-collector-principle-fail-closed`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-entity-metric`

**Touches**:

- Component: `cpt-cf-usage-collector-component-metric-catalog`
- Entities: `Metric`

### Entity: Metric Kind

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-metric-kind`

The system **MUST** consume `cpt-cf-usage-collector-entity-metric-kind` per DESIGN §3.1 as a closed enumeration of accumulation-semantics classifiers — `counter` (non-negative delta accumulation) and `gauge` (point-in-time, stored as-is) — obtain the authoritative `kind` for a `gts_id` exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`, never re-derive Metric Kind locally, and drive the counter and gauge branches of `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` deterministically from the catalog-returned classifier per `cpt-cf-usage-collector-principle-kind-enforcement`.

**Implements**:

- `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`
- `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup`

**Constraints**: `cpt-cf-usage-collector-entity-metric-kind`

**Touches**:

- Component: `cpt-cf-usage-collector-component-metric-catalog`
- Entities: `MetricKind`

### Entity: Security Context

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-entity-security-context`

The system **MUST** consume `cpt-cf-usage-collector-entity-security-context` per DESIGN §3.1 as the platform-resolved caller-identity envelope (caller principal, caller's tenant scope, auxiliary claims) — never owned, synthesized, or cached by `cpt-cf-usage-collector-component-ingestion-gateway`. The handler MUST accept the `SecurityContext` exclusively at one of the two convention-bound entry points — on REST as `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and on the SDK trait as `ctx: &SecurityContext` passed as the first parameter to `UsageCollectorClientV1::submit_usage_record(ctx, ...)` / `submit_usage_records(ctx, ...)` per `sdk-trait.md` Methods 1 and 2 — and pass it verbatim to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) so PDP authorizes the caller's identity against each per-record attribution tuple (including the caller-supplied `tenant_id` from `UsageRecordInput` per `cpt-cf-usage-collector-adr-caller-supplied-attribution`), and fail closed on missing `SecurityContext` or PDP unavailability per `cpt-cf-usage-collector-principle-fail-closed`. The `cpt-cf-usage-collector-entity-security-context` is the subject of PDP authorization — the persisted `tenant_id` column of `cpt-cf-usage-collector-dbtable-usage-records` is materialized from the caller-supplied `UsageRecordInput.tenant_id` field, not synthesized from the SecurityContext's tenant scope.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-usage-emission-attribution-and-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-entity-security-context`

**Touches**:

- Component: `cpt-cf-usage-collector-component-ingestion-gateway`
- Entities: `SecurityContext`

### API: POST /usage-collector/v1/records

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-usage-emission-api-post-records`

The system **MUST** expose `POST /usage-collector/v1/records` as the sole REST write entry point per `usage-collector-v1.yaml`, with the REST handler receiving `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and delegating to `UsageCollectorClientV1::submit_usage_record` / `submit_usage_records` (`ctx: &SecurityContext` as first parameter per `sdk-trait.md` Methods 1 and 2), accepting an `IngestRecordsRequest` with `records` `minItems: 1` / `maxItems: 100`, returning the `IngestRecordsResponse` with per-record outcomes in input order under HTTP `200` (all accepted/duplicated) or HTTP `207 Multi-Status` (mixed or all-rejected — i.e., whenever ≥1 record is rejected, single-record conflict included), and surfacing deterministic `Problem` envelopes only for whole-request failures (missing `SecurityContext` surfaced as canonical `Unauthenticated`; structural request-body validation) per the yaml's `default` response — per-record errors (PDP `deny`, unknown Metric, kind invariant, metadata size, plugin SPI error) MUST surface as the per-record `outcome="rejected"` with a per-record `error: Problem` carrying the `context.reason` drawn from the yaml's `cpt-cf-usage-collector-nfr-error-experience` taxonomy, never widening the contract beyond what is declared in the yaml.

**Implements**:

- `cpt-cf-usage-collector-flow-usage-emission-emit-record`
- `cpt-cf-usage-collector-flow-usage-emission-emit-records-batch`

**Constraints**: `cpt-cf-usage-collector-fr-ingestion`

**Touches**:

- API: `POST /usage-collector/v1/records`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### §2.3-item → DoD-ID Coverage Matrix

Coverage of every DECOMPOSITION §2.3 catalog item:

| §2.3 Item                                                                                                  | Kind              | DoD ID                                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-fr-ingestion`                                                                      | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion`                                                                                |
| `cpt-cf-usage-collector-fr-idempotency`                                                                    | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-idempotency`                                                                              |
| `cpt-cf-usage-collector-fr-record-metadata`                                                                | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-record-metadata`                                                                          |
| `cpt-cf-usage-collector-fr-counter-semantics`                                                              | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-counter-semantics`                                                                        |
| `cpt-cf-usage-collector-fr-gauge-semantics`                                                                | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-gauge-semantics`                                                                          |
| `cpt-cf-usage-collector-fr-tenant-attribution`                                                             | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-tenant-attribution`                                                                       |
| `cpt-cf-usage-collector-fr-resource-attribution`                                                           | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-resource-attribution`                                                                     |
| `cpt-cf-usage-collector-fr-subject-attribution`                                                            | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-subject-attribution`                                                                      |
| `cpt-cf-usage-collector-fr-ingestion-authorization`                                                        | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion-authorization`                                                                  |
| `cpt-cf-usage-collector-fr-data-quality`                                                                   | FR                | `cpt-cf-usage-collector-dod-usage-emission-fr-data-quality`                                                                             |
| `cpt-cf-usage-collector-nfr-throughput`                                                                    | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-throughput`                                                                              |
| `cpt-cf-usage-collector-nfr-throughput-profile`                                                            | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-throughput-profile`                                                                      |
| `cpt-cf-usage-collector-nfr-ingestion-latency`                                                             | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-ingestion-latency`                                                                       |
| `cpt-cf-usage-collector-nfr-workload-isolation`                                                            | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-workload-isolation`                                                                      |
| `cpt-cf-usage-collector-nfr-batch-and-report-timing`                                                       | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-batch-and-report-timing`                                                                 |
| `cpt-cf-usage-collector-nfr-availability-boundary`                                                         | NFR               | `cpt-cf-usage-collector-dod-usage-emission-nfr-availability-boundary`                                                                   |
| `cpt-cf-usage-collector-principle-idempotency-by-key`                                                      | Principle         | `cpt-cf-usage-collector-dod-usage-emission-principle-idempotency-by-key`                                                                |
| `cpt-cf-usage-collector-principle-kind-enforcement`                                                        | Principle         | `cpt-cf-usage-collector-dod-usage-emission-principle-kind-enforcement`                                                                  |
| `cpt-cf-usage-collector-principle-fail-closed`                                                             | Principle         | `cpt-cf-usage-collector-dod-usage-emission-principle-fail-closed`                                                                       |
| `cpt-cf-usage-collector-principle-pluggable-storage`                                                       | Principle         | `cpt-cf-usage-collector-dod-usage-emission-principle-pluggable-storage`                                                                 |
| `cpt-cf-usage-collector-constraint-no-business-logic`                                                      | Design constraint | `cpt-cf-usage-collector-dod-usage-emission-constraint-no-business-logic`                                                                |
| `cpt-cf-usage-collector-constraint-nfr-thresholds`                                                         | Design constraint | `cpt-cf-usage-collector-dod-usage-emission-constraint-nfr-thresholds`                                                                   |
| `cpt-cf-usage-collector-adr-caller-supplied-attribution`                                                   | ADR               | `cpt-cf-usage-collector-dod-usage-emission-adr-caller-supplied-attribution`                                                             |
| `cpt-cf-usage-collector-adr-mandatory-idempotency`                                                         | ADR               | `cpt-cf-usage-collector-dod-usage-emission-adr-mandatory-idempotency`                                                                   |
| `cpt-cf-usage-collector-component-ingestion-gateway`                                                       | Design component  | `cpt-cf-usage-collector-dod-usage-emission-component-ingestion-gateway`                                                                 |
| `cpt-cf-usage-collector-seq-emit-usage`                                                                    | Sequence          | `cpt-cf-usage-collector-dod-usage-emission-seq-emit-usage`                                                                              |
| `cpt-cf-usage-collector-dbtable-usage-records`                                                             | Data              | `cpt-cf-usage-collector-dod-usage-emission-dbtable-usage-records`                                                                       |
| `cpt-cf-usage-collector-entity-usage-record`                                                               | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-usage-record`                                                                         |
| `cpt-cf-usage-collector-entity-record-metadata`                                                            | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-record-metadata`                                                                      |
| `TenantRef` (carried via `SecurityContext`; materialized as the `tenant_id` column per DECOMPOSITION §2.3) | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-security-context` / `cpt-cf-usage-collector-dod-usage-emission-fr-tenant-attribution` |
| `cpt-cf-usage-collector-entity-resource-ref`                                                               | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-resource-ref`                                                                         |
| `cpt-cf-usage-collector-entity-subject-ref`                                                                | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-subject-ref`                                                                          |
| `cpt-cf-usage-collector-entity-idempotency-key`                                                            | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-idempotency-key`                                                                      |
| `cpt-cf-usage-collector-entity-metric`                                                                     | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-metric`                                                                               |
| `cpt-cf-usage-collector-entity-metric-kind`                                                                | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-metric-kind`                                                                          |
| `cpt-cf-usage-collector-entity-security-context`                                                           | Entity            | `cpt-cf-usage-collector-dod-usage-emission-entity-security-context`                                                                     |
| `POST /usage-collector/v1/records`                                                                         | API               | `cpt-cf-usage-collector-dod-usage-emission-api-post-records`                                                                            |

## 6. Acceptance Criteria

- [ ] `p1` - A well-formed single-record emit by an authorized caller through `POST /usage-collector/v1/records` (one-item `IngestRecordsRequest`) or the SDK single-emit operation persists exactly one durable row in `cpt-cf-usage-collector-dbtable-usage-records` through the Plugin SPI Method 1 single-record persist capability; the persisted row's `tenant_id`, `resource_id`, `resource_type`, optional `subject_id` / `subject_type`, `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, and `metadata` columns are byte-identical to the request payload, and the per-record acknowledgement carries `outcome="accepted"` with the plugin-minted `id` per `usage-collector-v1.yaml` (single-emit success per `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion` and `cpt-cf-usage-collector-dod-usage-emission-api-post-records`).
- [ ] `p1` - A batch `POST /usage-collector/v1/records` carrying N `cpt-cf-usage-collector-entity-usage-record` payloads with 1 ≤ N ≤ 100 (`IngestRecordsRequest.records` `minItems: 1` / `maxItems: 100`) dispatches each eligible-for-persist record through the Plugin SPI Method 2 batch persist capability under the per-record composite key `(tenant_id, metric_gts_id, idempotency_key)` and returns per-record outcomes in input order; a request with N > 100 or with an empty `records` array is rejected at the Ingestion Gateway with the request-level structural validation `Problem` envelope (HTTP `400`) per `usage-collector-v1.yaml` before any per-record processing and no row is written (batch cap and per-record dispatch per `cpt-cf-usage-collector-dod-usage-emission-nfr-batch-and-report-timing` and `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion`).
- [ ] `p1` - A counter-kind emit (resolved through `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup`) whose submitted `value` is below zero is rejected with the actionable validation error envelope (per-record `outcome="rejected"` with `context.reason="kind_invariant"`) before any Plugin SPI dispatch; counter rows persisted by accepted emits satisfy the §3.7 referential invariant `value >= 0` (counter non-negativity per `cpt-cf-usage-collector-dod-usage-emission-fr-counter-semantics`).
- [ ] `p1` - A gauge-kind emit is accepted as a point-in-time value stored as-is — no delta accumulation, no rewriting, no server-side shape rewriting — per DESIGN §3.1; a subsequent gauge emit for the same `(tenant_id, metric_gts_id)` pair replaces the prior value as observed by downstream readers (gauge replacement semantics per `cpt-cf-usage-collector-dod-usage-emission-fr-gauge-semantics`).
- [ ] `p1` - Two emits sharing the same composite `(tenant_id, metric_gts_id, idempotency_key)` within the Plugin SPI's dedup window — including across counter and gauge kinds — result in exactly one persisted row in `cpt-cf-usage-collector-dbtable-usage-records`; the second call returns `outcome="duplicate"` carrying the prior record's `id` without performing a second write, counter totals are not inflated, and the dedup check and the persist are a single SPI capability invocation (Method 1 for single, Method 2 for batch) with no separate non-transactional pre-check (idempotency dedup per `cpt-cf-usage-collector-dod-usage-emission-fr-idempotency` and `cpt-cf-usage-collector-dod-usage-emission-adr-mandatory-idempotency`).
- [ ] `p1` - An emit whose canonical on-the-wire serialized `cpt-cf-usage-collector-entity-record-metadata` exceeds the configured size cap (default 8 KiB per record) is rejected with the per-record `outcome="rejected"` carrying a per-record `error: Problem` (`context.reason="metadata_size"`) with the measured size and the configured cap before any Plugin SPI dispatch; payloads at or below the cap are forwarded unmodified and persisted byte-for-byte through the Plugin SPI with no truncation, rewriting, or content interpretation (metadata cap per `cpt-cf-usage-collector-dod-usage-emission-fr-record-metadata` and `cpt-cf-usage-collector-dod-usage-emission-entity-record-metadata`).
- [ ] `p1` - Every accepted single or batched emit accepts a resolved `cpt-cf-usage-collector-entity-security-context` at the handler boundary — on REST as `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`), on the SDK trait as `ctx: &SecurityContext` first parameter — and dispatches per-attribution-tuple PDP authorization through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`) against the full attribution tuple (`tenant_id` from `UsageRecordInput`, `cpt-cf-usage-collector-entity-resource-ref`, optional `cpt-cf-usage-collector-entity-subject-ref`, `source_gear`, Metric `gts_id`) before any Plugin SPI dispatch; absence of `SecurityContext` at the boundary surfaces the canonical `Unauthenticated` `Problem` envelope per the yaml `default` response, a per-tuple PDP `deny` surfaces the per-record `outcome="rejected"` with `context.reason="authz"` inside `IngestRecordsResponse` (PDP decisions are per-tuple — there is no envelope-level PDP deny aggregation), and no row is written in any of these cases (PDP-gated attribution authorization per `cpt-cf-usage-collector-dod-usage-emission-fr-ingestion-authorization` and `cpt-cf-usage-collector-dod-usage-emission-principle-fail-closed`).
- [ ] `p1` - Every emit resolves Metric existence and `cpt-cf-usage-collector-entity-metric-kind` exclusively through `cpt-cf-usage-collector-algo-metric-lifecycle-catalog-kind-lookup` against the in-process Metrics Catalog projection owned by `cpt-cf-usage-collector-component-metric-catalog` before any Plugin SPI dispatch; an emit whose Metric `gts_id` is absent from the projection — or whose declared kind disagrees with the catalog-resolved kind once kind enforcement runs — surfaces the per-record `outcome="rejected"` with `context.reason="unknown_metric"` for absence or `context.reason="kind_invariant"` for kind mismatch, the ingestion path never falls back to a direct Plugin SPI catalog read on the latency-critical hot path, and the ingestion path never triggers a projection refresh (catalog existence and kind enforcement per `cpt-cf-usage-collector-dod-usage-emission-fr-data-quality`, `cpt-cf-usage-collector-dod-usage-emission-principle-kind-enforcement`, and `cpt-cf-usage-collector-dod-usage-emission-entity-metric`).
- [ ] `p1` - Every accepted emit — single or batched — is persisted through the foundation-owned Plugin Host via the Plugin SPI Method 1 `persist_usage_record` or Method 2 `persist_usage_records` capability under the composite key `(tenant_id, metric_gts_id, idempotency_key)` and the SPI durability ack is required before the per-record `outcome="accepted"` is returned to the caller; Plugin SPI transport / readiness / persistence errors (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) surface as per-record `rejected` outcomes (`context.reason="plugin_readiness"`) with the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` preserved, no whole-batch rollback occurs, and the ingestion path never opens a parallel storage path or invents a binding (at-least-once durability through the Plugin SPI per `cpt-cf-usage-collector-dod-usage-emission-principle-pluggable-storage`, `cpt-cf-usage-collector-dod-usage-emission-nfr-availability-boundary`, and `cpt-cf-usage-collector-dod-usage-emission-seq-emit-usage`).
- [ ] `p1` - A well-formed compensation emit by an authorized source gear — `entry_type=compensation`, `value < 0`, a non-empty `corrects_id` pointing to a `usage` row that exists, has `entry_type=usage`, shares `(tenant_id, metric_gts_id)`, and is `status=active`, plus a mandatory caller-supplied idempotency key — is accepted on the **same unified ingestion path** (`POST /usage-collector/v1/records` or the SDK emit operation; no dedicated `compensate` endpoint, SDK method, or Plugin SPI call exists per `cpt-cf-usage-collector-adr-usage-compensation`) and persists exactly one row with `entry_type=compensation`, the signed-negative `value`, and the `corrects_id` foreign-key-like pointer; the per-record acknowledgement carries `outcome="accepted"` with the plugin-minted `id` (compensation flow success per `cpt-cf-usage-collector-dod-usage-emission-compensation-flow`).
- [ ] `p1` - A compensation emit against a `gauge` Metric is rejected at validation time via `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` with `outcome="rejected"` and `context.reason="gauge_compensation_rejected"` (HTTP `422` per the locked `usage-collector-v1.yaml` Problem.context.reason taxonomy and the SDK trait `GaugeCompensationRejected` variant) before any Plugin SPI dispatch — gauges have no `SUM` semantics, so `gauge + compensation` is the REJECTED cell of the four-cell value matrix and the locked five-code compensation taxonomy carves it out as its own enum (not collapsed into the generic `kind_invariant` code); no row is persisted (value-matrix enforcement per `cpt-cf-usage-collector-dod-usage-emission-value-matrix`).
- [ ] `p1` - A compensation emit whose `value` is greater than or equal to zero is rejected at validation time with `outcome="rejected"` and `context.reason="kind_invariant"` (counter+compensation requires `value < 0`; zero is not accepted as a no-op compensation); a counter+usage emit whose `value` is below zero remains rejected as before with the same `context.reason="kind_invariant"`; the unchanged counter+usage cell and the unchanged gauge+usage cell are both verifiable independently (four-cell value matrix per `cpt-cf-usage-collector-dod-usage-emission-value-matrix`).
- [ ] `p1` - A compensation emit whose `corrects_id` is missing, references a non-existent row, references an `entry_type=compensation` row, references a row from another tenant, references a row from another Metric, or references an `inactive` row is rejected at validation time via `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2` with `outcome="rejected"` and a precise `context.reason` from the locked taxonomy before any Plugin SPI persist dispatch — `kind_invariant` (HTTP `422`) when missing (the `counter+compensation` matrix cell REQUIRES `corrects_id`, so absence is a matrix-cell shape violation; no separate `corrects_id_required` code is defined in `usage-collector-v1.yaml` or `sdk-trait.md`); `corrects_id_not_found` (HTTP `404`) for non-existent references; `corrects_id_wrong_entry_type` (HTTP `409`) for `entry_type=compensation` references; `corrects_id_wrong_scope` (HTTP `409`) for cross-tenant or cross-Metric references; `corrects_id_inactive` (HTTP `409`) for `inactive` references (the L1 "referenced record MUST be `active`" branch also handles concurrent deactivation — a compensation referencing a row R that arrives while R is being deactivated surfaces the same `corrects_id_inactive` code without quarantine or retry queue). A `usage` emit that carries a `corrects_id` is rejected with `context.reason="kind_invariant"` for the symmetric matrix-cell shape violation of the `*+usage` cells (usage rows MUST NOT carry `corrects_id`). The four granular `corrects_id_*` codes MUST NOT be collapsed into a single generic `corrects_id_invalid` code on the wire (L1 `corrects_id` enforcement and concurrency rule per `cpt-cf-usage-collector-dod-usage-emission-corrects-id-l1` and `cpt-cf-usage-collector-dod-usage-emission-compensation-concurrency`).
- [ ] `p1` - A compensation emit missing the mandatory caller-supplied `cpt-cf-usage-collector-entity-idempotency-key` is rejected at the wire level by the same `idempotency_key` NOT NULL requirement that applies to ordinary ingestion per `cpt-cf-usage-collector-adr-mandatory-idempotency`; an EXACT-EQUALITY retry of a previously-accepted compensation (same composite `(tenant_id, metric_gts_id, idempotency_key)` and identical canonical fields including `entry_type`, `value`, `corrects_id`) returns `outcome="duplicate"` carrying the prior compensation row's `id` without performing a second write — no double-refund effect is recorded; a same-key submission whose canonical fields differ surfaces `outcome="rejected"` with `context.reason="idempotency_conflict"` (AlreadyExists/409) carrying the existing record's `id` (idempotency posture for compensation per `cpt-cf-usage-collector-dod-usage-emission-fr-idempotency` and `cpt-cf-usage-collector-dod-usage-emission-compensation-no-business-logic`).
- [ ] `p1` - The Usage Collector MUST NOT compute refunds, credits, credit-notes, quota, lot/FIFO-LIFO state, or per-record remaining amounts for accepted compensations — the persisted row's `value`, `entry_type`, `corrects_id`, and all other caller-supplied canonical fields are byte-identical to the request payload, and no L2 enforcement (non-negative net, negative-net detection / alerting / rejection) is performed by the Usage Collector; downstream consumers own any "net can't be negative" policy per the un-policed-net stance in DESIGN §3.10.3 (no-business-logic posture for compensation per `cpt-cf-usage-collector-dod-usage-emission-compensation-no-business-logic` and `cpt-cf-usage-collector-dod-usage-emission-constraint-no-business-logic`).

## 7. Changelog

- **0.2.1 — 2026-06-02** — Cascaded the ADR-0012 2026-06-02 amendment (simplifications 5 and 6) across the §2 Emit Record / Emit Records Batch / Compensation Emission flows, §3 Catalog Existence and Kind Lookup + Metadata Size-Cap Enforcement algorithms, §5 FR: Record Metadata / FR: Counter Semantics / FR: Gauge Semantics / FR: Data Quality DoDs. Added an explicit closed-shape metadata-key check step on every ingest flow (single, batch, compensation): every key in the candidate `RecordMetadata` MUST be a declared member of the metric's `metadata_fields`, otherwise the record is rejected with the `unknown_metadata_key` error name (`context.reason="unknown_metadata_key"`, `context.key`, `instance_path="/metadata/{key}"`) lifting to AIP-193 `InvalidArgument` / HTTP `400`. Reframed `MetricKind` as derived from the `gts_id` prefix against the two reserved kind base type ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`) across the counter / gauge semantics DoDs and the Catalog Existence and Kind Lookup algorithm output (the lookup now returns the derived `MetricKind` plus the metric's `metadata_fields` declared key set). The size-cap algorithm input is now framed as "key/value map with String values and declared keys"; there is no free-form remainder and no preserved extras. Cites ADR 0012 §Amendment, PRD §5.1/§5.2/§5.7, and domain-model §2.5/§2.6/§2.8.
- **0.2.0 — 2026-06-02** — Aligned the FEATURE with the unified plugin-DB Metric Catalog model per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference` (supersedes ADRs 0007 / 0009 / 0010) and the updated DESIGN (Phase 3). Usage records reference metrics by `gts_id` — the same string used as the catalog identifier (no UUID derivation); the in-plugin reference scheme (column type, index choice) is plugin-author choice per DESIGN §3.2 / §3.7 and explicitly out of FEATURE scope (noted once in §1.4 References and once in the `Catalog Existence and Kind Lookup` algorithm). Rewrote the `cpt-cf-usage-collector-algo-usage-emission-catalog-existence-and-kind-lookup` Output and Step 2 to cite the gateway L1 cache populated from the Metric Catalog (managed via the Plugin SPI, persisted in the active storage plugin's database) per ADR 0012, dropping the obsolete `cpt-cf-usage-collector-adr-gateway-local-metric-catalog` reference. Added ADR 0012 to §1.4 References. No residual mentions of `uuid5`, `parent_type_uuid`, indexable-trait gate, or `abstract` remain in normative content; `metric_type_uuid` was already absent. Preserved the unified ingestion path, kind/entry-type validation matrix, L1 `corrects_id` referential check, dedup composite, and all other behavior.
