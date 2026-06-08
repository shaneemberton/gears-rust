# Usage Collector SDK Trait Reference

<!-- toc -->

- [Overview](#overview)
- [Scope](#scope)
  - [In scope](#in-scope)
  - [Out of scope](#out-of-scope)
- [ToolKit SDK placement](#toolkit-sdk-placement)
  - [Crate layout](#crate-layout)
  - [Trait declaration shape](#trait-declaration-shape)
  - [Two-trait split](#two-trait-split)
- [Domain Model](#domain-model)
  - [Core ingestion and identity types](#core-ingestion-and-identity-types)
  - [Query types and views](#query-types-and-views)
  - [Internal authorization types (not part of the SDK signature)](#internal-authorization-types-not-part-of-the-sdk-signature)
  - [Method-specific output types](#method-specific-output-types)
  - [Cross-entity invariants honored by the SDK trait](#cross-entity-invariants-honored-by-the-sdk-trait)
- [Public SDK Trait](#public-sdk-trait)
- [Method Contracts](#method-contracts)
  - [PDP enforcement and plugin dispatch inside the trait implementation](#pdp-enforcement-and-plugin-dispatch-inside-the-trait-implementation)
  - [No dedicated `compensate` method — compensation rides the emit path](#no-dedicated-compensate-method--compensation-rides-the-emit-path)
  - [Method 1 — Submit single usage record](#method-1--submit-single-usage-record)
  - [Method 2 — Submit batched usage records](#method-2--submit-batched-usage-records)
  - [Method 3 — Aggregated query](#method-3--aggregated-query)
  - [Method 4 — Raw keyset-paginated query](#method-4--raw-keyset-paginated-query)
  - [Method 5 — Deactivate usage event](#method-5--deactivate-usage-event)
  - [Method 6 — Register metric](#method-6--register-metric)
  - [Method 7 — Read metric](#method-7--read-metric)
  - [Method 8 — List metrics](#method-8--list-metrics)
  - [Method 9 — Delete metric](#method-9--delete-metric)
- [Error Taxonomy](#error-taxonomy)
- [Versioning/Compatibility](#versioningcompatibility)
- [Exclusions/Non-goals](#exclusionsnon-goals)
  - [REST-only exclusions](#rest-only-exclusions)
  - [Plugin SPI exclusions](#plugin-spi-exclusions)
  - [Gear non-goals reaffirmed on the SDK trait](#gear-non-goals-reaffirmed-on-the-sdk-trait)
- [Traceability](#traceability)
  - [Trait identifier and consumer contract](#trait-identifier-and-consumer-contract)
  - [Capabilities exposed by the SDK trait](#capabilities-exposed-by-the-sdk-trait)
  - [Domain entities](#domain-entities)
  - [Authorization, fail-closed, and attribution anchors](#authorization-fail-closed-and-attribution-anchors)
  - [Plugin SPI and persistence anchors (exclusions)](#plugin-spi-and-persistence-anchors-exclusions)
  - [Versioning, stability, and quality NFR anchors](#versioning-stability-and-quality-nfr-anchors)
  - [Components allocated to the SDK trait](#components-allocated-to-the-sdk-trait)
- [Open Questions](#open-questions)

<!-- /toc -->

## Overview

The Usage Collector SDK trait is the public, in-process, transport-agnostic
async Rust API for the Usage Collector gear. It exposes the
platform-developer-facing capabilities of the gear — usage record
ingestion, aggregated query, raw cursor-paginated query, and individual
event deactivation — as a single ClientHub-registered async trait. The
trait deliberately omits operator-only catalog and platform-observability
operations, which remain REST-only.

This document is the reference specification for the trait. It captures
the operation set, method contracts (inputs, outputs, error behaviour),
domain types, error taxonomy, ToolKit placement, versioning and stability
policy, and exclusions. The exact Rust signature is owned by the SDK
crate itself; this reference defines what every signature must satisfy.

Sources: phase-01 §"SDK trait surface (DESIGN section 3.3)"
(`cpt-cf-usage-collector-interface-sdk-client`,
`cpt-cf-usage-collector-contract-downstream-usage-reader`); phase-02 §"Public
SDK Surface" and §"SDK Method Candidates".

**Consistency floor (read-after-write rule).** Reads through the SDK trait
inherit the gear-level consistency floor: a record `Acknowledged` by the
ingestion methods (`submit_usage_record`, `submit_usage_records`) is durable
and dedup-visible on the ingestion path, but the same record MAY be invisible
to a subsequent SDK aggregated query (`query_aggregated`), raw query
(`query_raw_keyset`), or catalog read (`read_metric`, `list_metrics`) for an
indeterminate window. The window is driven by the
active plugin's replication topology and the workload-isolation routing it
implements. Source gears MUST NOT design admission control, post-emit
summary, or any same-request outcome flow against the SDK query methods —
they MUST consume the ingestion ack the SDK already returns. Consumers that
need a tighter bound consciously couple themselves to a specific plugin's
published ceiling. Full contract: DESIGN [§3.10.8](./DESIGN.md#3108-consistency-contract)
(`cpt-cf-usage-collector-design-consistency-contract`,
`cpt-cf-usage-collector-adr-consistency-contract`,
`cpt-cf-usage-collector-nfr-query-freshness`).

## Scope

### In scope

The SDK trait realizes the following Usage Collector functional capabilities:

- Ingestion of UsageRecord submissions, including caller-supplied
  idempotency keys (`cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-seq-emit-usage`).
- Aggregated query against accepted UsageRecords, with PDP-narrowed
  scope and a single mandatory Metric filter
  (`cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-seq-query-aggregated`).
- Raw cursor-paginated query against accepted UsageRecords, with PDP
  narrowing and optional Metric, tenant, resource, and subject filters
  (`cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-seq-query-raw`).
- Individual event deactivation, performing a one-way monotonic
  `active -> inactive` status transition
  (`cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-seq-deactivate-event`).
- Metric catalog management (`register_metric`, `read_metric`,
  `list_metrics`, `delete_metric`), realized against the plugin-owned
  `metric_catalog` per `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012): every metric carries a closed `metadata_fields`
  declared-key list (`Vec<String>`; all values typed as `String`)
  and its `kind ∈ {counter, gauge}` is derived from the `gts_id`
  prefix matching one of the two reserved base type ids
  (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`)
  per ADR 0012's 2026-06-02 amendment — `kind` is NOT a separately
  declared field, trait, or catalog column
  (`cpt-cf-usage-collector-fr-metric-registration`,
  `cpt-cf-usage-collector-fr-metric-deletion`,
  `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-seq-register-metric`,
  `cpt-cf-usage-collector-seq-delete-metric`).

Sources: phase-01 §"SDK trait surface (DESIGN section 3.3)" and
§"SDK-Consumed Inputs And Outputs"; phase-02 §"SDK Method Candidates";
ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`).

### Out of scope

The SDK trait does not expose platform health (those endpoints remain
REST-only). Operational telemetry is pushed via OTLP from ToolKit's
global `SdkMeterProvider`; no in-gear HTTP metrics surface exists on
either the SDK trait or the REST API. The SDK trait does not implement
authentication, authorization, storage, cursor token generation, or
aggregation pushdown — authentication is owned by the ToolKit gateway
upstream of the collector, PDP enforcement is allocated to the
per-component `authz_scope` helper inside the trait implementation,
and storage/aggregation pushdown are allocated to the
ClientHub-resolved Plugin SPI. Per
`cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
(ADR 0012), Metric catalog management (`register_metric`, `read_metric`,
`list_metrics`, `delete_metric`) is in scope for the SDK trait and is
realized by the gateway-side `MetricCatalogService` that owns PDP
authorization, `gts_id` kind-prefix validation, and closed-shape
`metadata_fields` validation on top of the plugin-owned catalog. The
SDK dispatches to the plugin SPI for durable persistence per ADR 0012;
referential integrity is enforced by the plugin's
`usage_records.gts_id` `ON DELETE RESTRICT` foreign key.

Sources: phase-01 §"Exclusions"; phase-02 §"REST-Only Exclusions" and
§"Plugin SPI Boundary".

## ToolKit SDK placement

### Crate layout

The `UsageCollectorClientV1` trait lives in `usage-collector-sdk/src/api.rs`.
The companion plugin trait `UsageCollectorPluginV1` lives in
`usage-collector-sdk/src/plugin_api.rs`. Both traits, the GTS spec for
plugin discovery, the domain models, and the public error enum share a
single `usage-collector-sdk` crate — there is no separate `-contracts`
or `-plugin-api` crate. This follows the platform-standard `<gear>` +
`<gear>-sdk` two-crate layout used by every reference gear
(`credstore`, `authn-resolver`, `authz-resolver`).

Required files under `usage-collector-sdk/src/`, all transport-agnostic:

- `lib.rs` — crate root and re-exports.
- `api.rs` — public consumer SDK trait declaration (this document's subject).
- `plugin_api.rs` — public Plugin SPI trait declaration (subject of `plugin-spi.md`).
- `gts.rs` — GTS spec for plugin discovery and binding (reserved; populated by the plugin-registration step per DESIGN §3.12.9).
- `models.rs` — public domain types (see §"Domain Model").
- `error.rs` — public, domain-classified SDK error type (see §"Error
  Taxonomy").

REST DTOs and Axum types must not appear in the SDK crate; the REST
handler in the host `usage-collector` crate converts SDK domain errors
into RFC-9457 `Problem` responses via `IntoResponse`.

The in-process implementation `UsageCollectorLocalClient` lives in
`usage-collector/src/domain/local_client.rs` (inside the host crate)
and is registered un-scoped into ClientHub via
`ctx.client_hub().register::<dyn UsageCollectorClientV1>(...)`, mirroring
the pattern used by `credstore/src/gear.rs:57–58`.

Sources: phase-02 §"ToolKit Conventions" / "Crate placement and layout";
DESIGN §3.12.9 "Cargo crate naming" two-crate layout.

### Trait declaration shape

- The trait is declared `async` (via the `async_trait` pattern), is
  `Send + Sync + 'static`, and is used through ClientHub as a
  trait object.
- The canonical trait name is `UsageCollectorClientV1`, following the
  ToolKit naming convention that places the gear name and capability
  before the `V1` suffix. The `V1` suffix encodes the SDK trait's major
  version and aligns with the gear's major-version stability contract.
- Every method takes `&self` as the receiver and accepts
  `&SecurityContext` as its first explicit parameter; the SDK never
  synthesizes identity or falls back to anonymous access.
- Methods return a `Result` whose `Err` variant is the
  `UsageCollectorError` enum declared in `error.rs` (see §"Error
  Taxonomy"); the `Ok` variant is the method-specific output type
  declared in `models.rs`.
- The trait is registered into ClientHub without scope by the gear's
  `init()`; consumers obtain the client through ClientHub.

Sources: phase-02 §"ToolKit Conventions" / "Trait shape", "ClientHub
registration", "SecurityContext convention", and "Error and Problem
mapping"; phase-01 §"SDK trait surface (DESIGN section 3.3)".

### Two-trait split

The public SDK trait, `UsageCollectorClientV1`, is the consumer-facing
trait. The Usage Collector's Plugin SPI trait, `UsageCollectorPluginV1`,
is described in `plugin-spi.md` and lives in the same
`usage-collector-sdk` crate at `plugin_api.rs`, per the platform-standard
two-trait / single-SDK-crate pattern. Both traits share the same
`models.rs` domain types and `error.rs` error enum; no separate
`-contracts` or `-plugin-api` crate sits between them. The Plugin SPI is
not in scope for this reference.

Sources: phase-02 §"ToolKit Conventions" / "Trait shape" two-trait
pattern bullet; DESIGN §3.12.9 "Cargo crate naming" two-crate layout;
phase-01 §"Plugin Binding And Surface Mapping".

## Domain Model

The SDK trait operates exclusively on the canonical Usage Collector
domain types from `domain-model.md`. All domain types are declared in
`usage-collector-sdk/src/models.rs` and remain transport-agnostic.
Field names are snake_case; struct and enum names are UpperCamelCase.
Identifiers (`tenant_id`, `resource_id`, `subject_id`, `source_gear`,
`gts_id`) are opaque platform identifiers; the Usage Collector neither
parses nor classifies them. All timestamps are UTC instants.

Sources: phase-01 §"Modeling conventions" and §"Domain Entities" /
§"Query Models And Views"; phase-02 §"ToolKit Conventions" / "Domain
types and models".

### Core ingestion and identity types

- `UsageRecord` (`cpt-cf-usage-collector-entity-usage-record`). A
  single attributed measurement of resource consumption with status.
  Fields: `id` (opaque, plugin-minted, present on accepted records),
  `tenant_id` (opaque, required), `resource` (`ResourceRef`,
  required), `subject` (`SubjectRef`, optional), `source_gear`
  (opaque, required), `gts_id` (GTS metric identifier string;
  required; MUST resolve to a row in `metric_catalog` per ADR 0012),
  `value` (signed numeric; permitted sign is determined jointly by the
  referenced metric's derived `kind` (from `gts_id` prefix) and the
  record's `entry_type` per the four-cell validation matrix — see
  §"Method Contracts" Method 1, required), `timestamp` (UTC, required),
  `idempotency_key`
  (`IdempotencyKey`, required), `entry_type` (`EntryType`, required,
  default `usage` on ingestion), `corrects_id` (opaque
  `UsageRecord.id`, conditional — required when
  `entry_type = compensation`, forbidden when `entry_type = usage`),
  `status` (`DeactivationStatus`, required), `metadata`
  (`RecordMetadata` key/value map, optional; every key MUST be a
  member of the referenced metric's `metadata_fields` and every value
  is typed as `String` — closed shape, gateway-validated at L1 per
  ADR 0012; undeclared keys are rejected with `UnknownMetadataKey`
  and never silently preserved). Accepted records are
  immutable except for the one-way `active -> inactive` status
  transition; `entry_type` and `corrects_id` are never mutated after
  acceptance.
- `EntryType` (`cpt-cf-usage-collector-entity-entry-type`). Discriminator
  separating ordinary usage emission from caller-supplied counter
  value-reversal. Values: `usage` (default; ordinary emission;
  `MIN`/`MAX`/`AVG`/`COUNT` operate over this set) and `compensation`
  (append-only signed-negative entry on a `counter` Metric that
  references a `usage` row via `corrects_id` and reduces `SUM(value)`;
  rejected on `gauge` Metrics). Never mutated after acceptance.
  Compensation rides the unified emit path; see §"No dedicated
  `compensate` method — compensation rides the emit path" under
  §"Method Contracts".
- `ResourceRef` (`cpt-cf-usage-collector-entity-resource-ref`).
  Resource attribution composite: `resource_id` (opaque, required) and
  `resource_type` (opaque, required); both must be supplied together.
- `SubjectRef` (`cpt-cf-usage-collector-entity-subject-ref`). Optional
  subject attribution: `subject_id` (opaque, conditional) and
  `subject_type` (opaque, optional). `subject_type` is permitted only
  when `subject_id` is present; subject identifiers are opaque platform
  identifiers and are not derived from the caller's SecurityContext.
- `MetricRecord` (`cpt-cf-usage-collector-entity-metric`).
  Platform-global metric definition, modeled as a GTS _type_ schema
  (id ends `~`) per ADR 0012. Fields: `gts_id` (GTS metric identifier
  string suffixed `~`, deployment-unique; primary key in the
  plugin-owned `metric_catalog` per ADR 0012, required; MUST begin
  with one of the two reserved kind base type id prefixes —
  `gts.cf.core.usage.counter.v1~` or
  `gts.cf.core.usage.gauge.v1~`),
  `metadata_fields` (`Vec<String>`, required; the closed list of
  declared metadata keys for this metric — every key in an ingested
  record's `metadata` map MUST be a member of this list, every value
  is typed as `String`, undeclared keys are rejected),
  `created_at` (UTC timestamp captured by the plugin on accept).
  `MetricRecord` carries NO `kind` column and NO `traits` map —
  `kind` is derived per lookup from `gts_id`'s prefix.
- `MetricKind` (`cpt-cf-usage-collector-entity-metric-kind`).
  Accumulation-semantics classifier **derived** from the metric's
  `gts_id` prefix per ADR 0012's 2026-06-02 amendment; NOT a stored
  column, NOT a declared trait, NOT a separately registered field.
  Values: `Counter` (`gts_id` begins with
  `gts.cf.core.usage.counter.v1~`; non-negative gross usage deltas
  when `entry_type = usage`, strictly-negative compensation deltas
  when `entry_type = compensation`; cumulative per-`(tenant_id, gts_id)`
  total is the signed `SUM` and is NOT monotonically increasing in
  the presence of compensation) and `Gauge` (`gts_id` begins with
  `gts.cf.core.usage.gauge.v1~`; point-in-time, stored as-is; admits
  `entry_type = usage` only — compensation on a gauge metric is
  rejected before persistence per the four-cell value matrix). A
  `gts_id` whose prefix matches neither of the two reserved base
  type ids is rejected at registration with `InvalidKindPrefix`. The
  SDK surfaces `MetricKind` on the `MetricRecord` view through a
  derived accessor (computed from `gts_id`) for ergonomics.
- `IdempotencyKey` (`cpt-cf-usage-collector-entity-idempotency-key`).
  Caller-supplied opaque identifier; required on every ingestion. A
  same-key submission within `(tenant_id, gts_id, idempotency_key)`
  is resolved by exact-equality of the caller-supplied canonical fields
  (per `domain-model.md` §2.6): an exact-equality retry — all canonical
  fields equal — is silently deduplicated by the active plugin, while any
  differing canonical field (including a metadata-only difference) is a
  `Conflict` that is rejected fail-closed and surfaced as
  `UsageCollectorError::IdempotencyConflict`, never silently absorbed.
- `RecordMetadata` (`cpt-cf-usage-collector-entity-record-metadata`).
  Optional key/value map payload. Per ADR 0012 (2026-06-02
  amendment), the metadata model is a **closed shape**: every key
  MUST be a member of the referenced metric's `metadata_fields` list
  (declared at registration; flat `Vec<String>`), every value is
  typed as `String`, and there is no free-form remainder, no open
  extras escape hatch, and no preserved undeclared keys. Undeclared
  keys are rejected at the gateway before plugin dispatch with the
  `UnknownMetadataKey { gts_id, key }` variant (see §"Error
  Taxonomy"). Per-metric queryable dimensions are exactly the keys
  in `metadata_fields` — declared = queryable, with no separate
  indexable-trait gate. Default maximum size is 8 KiB per record
  unless operator configuration overrides it. Plugins do NOT
  re-implement metadata validation; closed-shape validation is the
  gateway's L1 responsibility per ADR 0012.
- `DeactivationStatus`
  (`cpt-cf-usage-collector-entity-deactivation-status`). Values:
  `Active` (default state for newly accepted records) and `Inactive`
  (records that have been deactivated). The only permitted transition
  is `Active -> Inactive`.
- `SecurityContext` (`cpt-cf-usage-collector-entity-security-context`).
  Resolved platform caller identity supplied by the ToolKit gateway
  upstream of the collector (REST surface) or constructed by the
  in-process caller (SDK surface); consumed but not owned by the
  Usage Collector. Carries `principal`, `tenant_scope_claims`,
  `auxiliary_claims`, and `correlation_id` (required for API operations
  and propagated through gateway, PDP-decision logs, gear logs, and
  platform audit trail). The SDK requires a resolved `SecurityContext`
  on every call.

Sources: phase-01 §"Domain Entities".

### Query types and views

- `AggregationQuery`
  (`cpt-cf-usage-collector-entity-aggregation-query`). Fields:
  `time_range` (UTC start/end interval, required, bounded interval),
  `metric_gts_id` (GTS identifier, required, exactly one metric;
  per ADR 0012 the metric is the dimension-resolution anchor, so
  cross-metric aggregation is structurally out of scope and remains a
  non-goal), `aggregation` (enum of `Sum`, `Count`, `Min`, `Max`,
  `Avg`, required), `tenant_id` (opaque, optional), `resource`
  (`ResourceRef`, optional), `subject` (`SubjectRef`, optional),
  `source_gear` (opaque, optional), `group_by` (optional list of
  dimensions; admits the **fixed dimensions** — tenant, resource,
  subject, source gear, authorized time-period groupings — plus
  the **per-metric declared keys** resolved per request from the
  referenced metric's `metadata_fields` list). `$filter` over the
  same dimension set accepts `eq` / `in` operators on `String`-typed
  values; the SDK rejects group-by or filter references to keys not
  in `metadata_fields` (or to undeclared fixed dimensions) before
  plugin dispatch.
- `RawQuery` (`cpt-cf-usage-collector-entity-raw-query`). The SDK
  surface uses the canonical
  `ODataQuery<UsageRecordFilterField>` value defined by
  `toolkit-odata`. Fields:
  - `metric_gts_id` — GTS metric identifier; **REQUIRED** per
    ADR 0012 (rationale: raw queries always operate over a single
    metric and must resolve declared dimensions deterministically;
    carrying the metric on the request lets the gateway resolve the
    per-metric declared keys exactly once before composing
    `filter_ast` against the union of fixed fields and per-metric
    `metadata_fields`). Was optional in earlier drafts of this
    surface.
  - `filter_ast: toolkit_odata::filter::FilterNode<UsageRecordFilterField>`
    — parsed OData `$filter`. The admissible field set is no longer
    a static enum: it is the union of the fixed fields (`tenant`,
    `resource`, `subject`, `source_gear`, `timestamp`, `status`)
    and the per-metric declared dimensions resolved from
    `metric_gts_id` per the rule above. Dimension filters accept
    `eq` / `in` (categorical strings); fixed-field operator
    allowances follow `domain-model.md` §2.10 /
    `out/phase-01-domain-contracts.md` §4.
  - `order: toolkit_odata::ODataOrderBy` — parsed `$orderby`; for raw
    queries it is the canonical `timestamp asc, id asc` order.
  - `page_after: Option<toolkit_odata::Keyset>` — typed `(timestamp,
id)` keyset captured by the gateway from the caller-supplied
    `CursorV1`. `None` on the first call; threaded by the caller
    from `page_info.next_cursor` on subsequent calls (callers see
    only the opaque `CursorV1`, never `page_after`).
  - `limit: u64` — gateway-clamped per-page limit, bounded by the
    REST/SDK contract.
- `UsageRecordFilterField`
  (`cpt-cf-usage-collector-entity-usage-record-filter-field`). The
  filter-admissibility surface for raw and aggregated query paths.
  Per ADR 0012 this is no longer a static Rust enum: it is the
  **union of fixed `UsageRecord` fields** (`tenant`, `resource`,
  `subject`, `source_gear`, `timestamp`, `status`) and the
  **per-metric declared keys resolved per request** from the
  metric referenced by `RawQuery.metric_gts_id` (or
  `AggregationQuery.metric_gts_id`) — specifically, every key in
  the metric's `metadata_fields` list. Filter admissibility is
  therefore declaration-driven, not enum-driven: the SDK rejects
  filters that target keys not in `metadata_fields` (or undeclared
  fixed dimensions) before plugin dispatch and surfaces the
  rejection as `Validation`.
- `Keyset` (`cpt-cf-usage-collector-entity-keyset`). Typed
  `(timestamp, id)` tuple consumed by the toolkit cursor encoder; the
  SDK trait does not surface `Keyset` directly to callers — they
  thread the opaque `CursorV1` returned in `page_info.next_cursor`.
- `AggregationResult`
  (`cpt-cf-usage-collector-entity-aggregation-result`). Server-side
  aggregation output: `metric_gts_id`, `aggregation`, and `buckets`
  (list of buckets). Each bucket carries `dimensions` (object of
  group-by dimension values) and `value` (aggregated numeric value).
- `toolkit_odata::Page<UsageRecord>`
  (`cpt-cf-usage-collector-entity-keyset` is the canonical
  pagination key; `Page<T>` is the canonical envelope from
  `libs/toolkit-odata/src/page.rs`). Keyset-paginated page returned
  by raw query: `items: Vec<UsageRecord>` (each carrying its
  `status`) and `page_info: PageInfo { next_cursor, prev_cursor,
limit }` where `next_cursor` / `prev_cursor` are opaque
  `CursorV1` envelopes minted by the toolkit gateway from the
  plugin-returned last-row `Keyset`.

Cursor envelopes are opaque to SDK callers and are minted, decoded,
and validated by the toolkit gateway (`toolkit_odata::CursorV1` plus
`toolkit_odata::validate_cursor_against`). SDK callers pass
`limit` and the next-page `CursorV1` they read from
`page_info.next_cursor`; they never observe `page_after` or `Keyset`
in raw form. Cursor decode failure, order mismatch, and filter
mismatch are surfaced as canonical Problem responses
(`cursor_decode`, `order_mismatch`, `filter_mismatch`).

Sources: phase-01 §"Query Models And Views";
`out/phase-01-domain-contracts.md` §§1–4; `research-toolkit-alignment.md`
§1 D9 (SDK trait return type) and D10 (gateway-owned cursor
lifecycle).

### Internal authorization types (not part of the SDK signature)

`PdpDecision` (`cpt-cf-usage-collector-entity-pdp-decision`) and
`PdpConstraint` (`cpt-cf-usage-collector-entity-pdp-constraint`) are
authorization-internal types used by the per-component `authz_scope`
helper invoked inside `UsageCollectorClientV1` (ingestion gateway,
query gateway, deactivation handler, and metrics catalog). They are
not declared on the public SDK trait surface
and do not appear in SDK method signatures or return shapes. The SDK
trait callers see only the post-authorization outcome (permit produces
a result; deny produces an `Authorization` error variant).

Sources: phase-01 §"Authorization And Tenancy Facts"; phase-02 §"Public
SDK Surface" and §"Validation And Testing Ideas" (PDP-double seam).

### Method-specific output types

- `UsageRecordAck` — accepted-record acknowledgement returned by both
  ingestion methods. Fields: `id` (plugin-minted `UsageRecord.id`),
  `status` (`DeactivationStatus`, always `Active` on acceptance), and
  `dedup` (`DedupOutcome` indicator).
- `DedupOutcome` — values: `Accepted` (the record was newly persisted),
  `Deduplicated` (a prior record with the same
  `(tenant_id, gts_id, idempotency_key)` was already present
  **and the incoming record's caller-supplied canonical fields are
  exactly equal** — an exact-equality retry — so the acknowledgement
  reports the prior record's `id` as a silent-absorb dedup success), and
  `Conflict` (a prior record with the same dedup key was present **but
  the incoming record's caller-supplied canonical fields differ** — a
  canonical-field mismatch). `DedupOutcome::Accepted` and
  `DedupOutcome::Deduplicated` are reported on the `Ok` arm of
  `UsageRecordAck.dedup`; `DedupOutcome::Conflict` is NOT a successful
  acknowledgement — it is surfaced to the caller as the
  `UsageCollectorError::IdempotencyConflict` error variant (see §"Error
  Taxonomy"), so a divergent same-key re-emission is rejected
  fail-closed and never silently dropped. The compared canonical fields
  are `value`, `timestamp`, `resource_ref`, `subject_ref`,
  `source_gear`, `entry_type`, `corrects_id`, and `metadata`; the
  dedup-key tuple is the match key and the server-owned `id` / `status`
  are excluded, so ALL compared fields equal → `Deduplicated`, and ANY
  compared field differs — including a metadata-only difference or a
  `usage` ↔ `compensation` flip on the same idempotency key → `Conflict`. Because a `Conflict`
  always converts to `UsageCollectorError::IdempotencyConflict`,
  `DedupOutcome::Conflict` is a host→core internal signal only: SDK
  callers never observe it in `UsageRecordAck.dedup`, which only ever
  carries `Accepted` or `Deduplicated`.
- `DeactivationAck` — successful-transition acknowledgement returned by
  Method 5. Fields: `id` (the targeted `UsageRecord.id`, of ANY
  `entry_type`), `status` (`DeactivationStatus`, always `Inactive` after
  a successful transition), and `cascaded_compensation_ids: List<Id>`
  (the depth-1 set of compensation row ids that were active at
  deactivation time and were flipped to inactive alongside the targeted
  row in the same atomic plugin transition; MAY be empty; order is
  unspecified; MUST be present on every successful deactivate
  response). A successful return implies the targeted record was
  `Active` before the call and is now `Inactive`, AND every id in
  `cascaded_compensation_ids` was likewise `Active` before the call and
  is now `Inactive`. The cascade list is non-empty only when the
  targeted row had `entry_type = usage` AND at least one active
  compensation row referenced it via `corrects_id`; deactivating an
  `entry_type = compensation` row never cascades (the list is empty by
  construction per `cpt-cf-usage-collector-adr-usage-compensation`
  non-goals). The rejection cases (already-inactive record, unknown
  record) are surfaced through error variants of
  `UsageCollectorError` (see §"Error Taxonomy"), per `domain-model.md`
  §2.10 "A second deactivation request for an inactive record is
  rejected with an actionable error" and DESIGN.md
  `cpt-cf-usage-collector-principle-monotonic-deactivation` "rejects
  deactivation requests against already-inactive records".

These output types are declared in `usage-collector-sdk/src/models.rs`
so that callers can pattern-match results without parsing error
shapes. They are SDK-local synthesis types derived from the surface
mapping facts in phase-01 §"Plugin Binding And Surface Mapping" / "SDK
trait" and the dedup / deactivation outcome facts in phase-01
§"Ingestion" and §"Event deactivation"; phase-02 §"SDK Method Inputs
And Outputs" and §"Open Questions (annotated)" OQ-3.

Catalog-operation input/output types (per ADR 0012):

- `MetricGtsId` — opaque GTS metric identifier string (suffixed `~`)
  for a registered metric. Declared in
  `usage-collector-sdk/src/models.rs` alongside the other identity
  types; this is the catalog primary key and the FK column on
  `usage_records` per ADR 0012.
- `RegisterMetricInput` — caller-supplied metric registration
  payload (mirrors the plugin SPI per `plugin-spi.md` §"Method 6"):
  `gts_id` (GTS metric identifier, suffixed `~`, required;
  deployment-unique; MUST begin with one of the two reserved kind
  base type id prefixes — `gts.cf.core.usage.counter.v1~` or
  `gts.cf.core.usage.gauge.v1~` — per ADR 0012),
  `metadata_fields` (`Vec<String>`, required; flat closed list of
  declared metadata keys for the metric — unique non-empty strings;
  all corresponding values are typed as `String` at ingest;
  undeclared keys at record ingest are rejected with
  `UnknownMetadataKey`). Gateway-validated structurally (PDP
  authorization; `gts_id` kind-prefix check against the two
  reserved base type ids; `metadata_fields` well-formedness — unique
  non-empty strings) before the plugin SPI dispatch — see
  `cpt-cf-usage-collector-component-metric-catalog`. The payload
  carries NO `kind` field and NO `traits` map; `kind` is derived
  from `gts_id` and is therefore not a registration input.
- `ListMetricsFilter` — caller-supplied filter for `list_metrics`
  (mirrors the plugin SPI per `plugin-spi.md` §"Method 8"): `kind`
  (`Option<MetricKind>` — derived from each candidate row's `gts_id`
  prefix per ADR 0012; not a dedicated catalog column, not stored
  on the row), plus the keyset-pagination fields
  (`page_after: Option<Keyset>` over `(created_at, gts_id)`,
  `limit: u64`).
- `PageParams` — keyset-pagination input for `list_metrics`:
  opaque `cursor` (optional, returned by the previous page) and
  `limit` (gateway-clamped per the REST/SDK contract). Cursor
  envelopes are minted and decoded by the gateway; SDK callers see
  only the opaque `cursor` string they thread from the previous
  page. Internally composes into the `ListMetricsFilter` keyset for
  plugin dispatch.
- `Page<T>` — the platform `toolkit_odata::Page<T>` envelope reused
  for catalog list responses: `items: Vec<T>` plus a `page_info`
  block carrying `next_cursor`, `prev_cursor`, and `limit`.

### Cross-entity invariants honored by the SDK trait

- Every accepted record references a registered metric via its
  `gts_id` per ADR 0012. Records referencing an unknown `gts_id` are
  rejected as `UnknownMetric` before persistence. Catalog-row
  referential integrity is enforced natively by the plugin via the
  `usage_records.gts_id` `ON DELETE RESTRICT` foreign key per
  ADR 0012.
- Every read and write operation requires a resolved `SecurityContext`
  (resolved by the ToolKit gateway upstream of the collector on the
  REST surface, or supplied by the in-process caller on the SDK
  surface) and a positive PDP decision. The SDK fails closed on
  missing/invalid `SecurityContext`, PDP failure, validation failure,
  plugin-readiness failure, or storage failure.
- The `metadata` field is closed-shape per ADR 0012 (2026-06-02
  amendment): every key MUST be a member of the referenced metric's
  declared `metadata_fields` list, every value is typed as `String`,
  and the gateway L1 rejects undeclared keys with
  `UnknownMetadataKey { gts_id, key }` before plugin dispatch. There
  is no preserved free-form remainder. Per-metric queryable
  dimensions are exactly the keys in `metadata_fields` (declared =
  queryable) and are queryable on the raw and aggregated query
  surfaces.
- Pricing, rating, billing, invoice generation, and quota decisions are
  out of scope for every SDK operation.

Sources: phase-01 §"Cross-Entity Invariants" and §"Authorization And
Tenancy Facts"; phase-02 §"Versioning, Deprecation, And Non-Goals".

## Public SDK Trait

The Usage Collector exposes one public SDK trait,
`UsageCollectorClientV1`. The trait is async, `Send + Sync + 'static`,
declared in `usage-collector-sdk/src/api.rs`, and registered into
ClientHub without scope by the Usage Collector gear's `init()`.

The trait carries nine methods, one per SDK-exposed capability (ADR 0012 removed `read_metric_chain`):

| Method (logical)             | Realizes                                                                                                   | Inputs (beyond `&SecurityContext`)                                                                                                                                                                                                                                                                        | Output (Ok variant)                                                                                                                                                                                                                                                                                                                                                             |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Submit single usage record   | `fr-ingestion`, `fr-idempotency`, `fr-usage-compensation`, `seq-emit-usage`                                | One `UsageRecordSubmission` value (per-record fields described in §"Method Contracts", including optional `entry_type` and `corrects_id` and signed `value`).                                                                                                                                             | `UsageRecordAck` (accepted record `id`, `DeactivationStatus`, `DedupOutcome`).                                                                                                                                                                                                                                                                                                  |
| Submit batched usage records | `fr-ingestion`, `fr-idempotency`, `fr-usage-compensation`, `nfr-throughput`, `nfr-batch-and-report-timing` | Non-empty list of `UsageRecordSubmission` (each carrying optional `entry_type` and `corrects_id`).                                                                                                                                                                                                        | List of per-record `Result` carrying `UsageRecordAck` aligned with the input order.                                                                                                                                                                                                                                                                                             |
| Aggregated query             | `fr-query-aggregation`, `seq-query-aggregated`                                                             | One `AggregationQuery`.                                                                                                                                                                                                                                                                                   | `AggregationResult`.                                                                                                                                                                                                                                                                                                                                                            |
| Raw keyset-paginated query   | `fr-query-raw`, `seq-query-raw`                                                                            | One `ODataQuery<UsageRecordFilterField>` (parsed `filter_ast`, `order`, optional `page_after`, `limit`).                                                                                                                                                                                                  | `toolkit_odata::Page<UsageRecord>` (`items` + `page_info` with opaque `CursorV1`).                                                                                                                                                                                                                                                                                               |
| Deactivate usage event       | `fr-event-deactivation`, `seq-deactivate-event`                                                            | The target `UsageRecord.id` (any `entry_type`).                                                                                                                                                                                                                                                           | `DeactivationAck` (targeted `id`, resulting `DeactivationStatus` always `Inactive` on success, and `cascaded_compensation_ids: List<Id>` carrying the depth-1 cascade of compensation rows flipped alongside a deactivated `usage` row — empty for a `compensation` primary). Rejections for already-inactive or unknown records are surfaced as error variants.                |
| Register metric              | `fr-metric-registration`, `seq-register-metric`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`    | One `RegisterMetricInput` (`gts_id`, `metadata_fields: Vec<String>`). Validated structurally by the trait implementation (PDP authorization; `gts_id` kind-prefix check against the two reserved base type ids; `metadata_fields` well-formedness — unique non-empty strings) before plugin SPI dispatch. | `MetricRecord` — the durable catalog row produced by the plugin's `register_metric` SPI call against the plugin-owned `metric_catalog`, carrying the registered `gts_id` and `metadata_fields` per ADR 0012. A `gts_id` whose prefix does not match one of the two reserved kind base type ids surfaces `InvalidKindPrefix`. Duplicate `gts_id` surfaces `MetricAlreadyExists`. |
| Read metric                  | `fr-metric-existence-and-kind`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`                     | `gts_id: GtsId`.                                                                                                                                                                                                                                                                                          | `MetricRecord` carrying `gts_id`, `metadata_fields`, and `created_at` (and a derived `kind` accessor computed from `gts_id`). Missing identifier surfaces `MetricNotFound`.                                                                                                                                                                                                     |
| List metrics                 | `fr-metric-existence-and-kind`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`                     | `ListMetricsFilter` (`kind`) plus `paging: PageParams`.                                                                                                                                                                                                                                                   | `Page<MetricRecord>` — keyset-paginated list of registered metrics, each row including `gts_id`, `metadata_fields`, and `created_at` (kind is derived from `gts_id` per ADR 0012).                                                                                                                                                                                              |
| Delete metric                | `fr-metric-deletion`, `seq-delete-metric`, `adr-0012-unified-plugin-catalog-and-gts-id-reference`          | `gts_id: GtsId`.                                                                                                                                                                                                                                                                                          | `()`. Rejections: `MetricNotFound`, and `MetricReferenced { gts_id, sample_ref_count }` when the plugin's `ON DELETE RESTRICT` FK rejects the delete (lifts to HTTP 409 on the REST surface). Callers MUST expect 409 on referenced metrics.                                                                                                                                    |

All methods return a `Result` over the listed Ok variant and
`UsageCollectorError` (see §"Error Taxonomy"). The submission input
type, `UsageRecordSubmission`, is declared in `models.rs` and carries
the caller-supplied per-record fields described in §"Method Contracts"
(it is distinct from the persisted `UsageRecord`, which carries the
plugin-minted `id` and the accepted `status`).

Sources: phase-01 §"SDK trait surface (DESIGN section 3.3)",
§"SDK-Consumed Inputs And Outputs", §"Plugin Binding And Surface
Mapping"; phase-02 §"Public SDK Surface", §"SDK Method Candidates",
§"ToolKit Conventions".

Note on batched ingestion: phase-02 §"SDK Method Candidates" records
the single-vs-batched shape as a Phase 3 decision (gap G-2). This
reference adopts a two-method form (single and batched) because the
Plugin SPI accepts single and batched records (phase-02 §"Plugin SPI
Boundary"), and the ingestion throughput and batch-and-report timing
NFRs (`cpt-cf-usage-collector-nfr-throughput`,
`cpt-cf-usage-collector-nfr-batch-and-report-timing`) require batched
submissions at the gateway. The single-record method is retained as
the ergonomic case and to keep per-call latency budgets tractable.

Note on `SecurityContext` parameter placement: phase-02 §"ToolKit
Conventions" / "SecurityContext convention" requires `&SecurityContext`
as the first parameter of every SDK method; that convention is the
canonical realization of the cross-entity invariant in phase-01
§"SecurityContext" requiring a resolved context on every operation.

## Method Contracts

Each method contract below lists the realized FR/sequence identifiers,
the required `SecurityContext` invariant, additional structural inputs,
the success output, and the error categories the method may surface.
Concrete error variant names are defined in §"Error Taxonomy".

### PDP enforcement and plugin dispatch inside the trait implementation

The `UsageCollectorClientV1` trait implementation
(`UsageCollectorClientV1` realized by the in-process
`usage-collector` gear crate) is the canonical site for PDP
enforcement, Metric-existence validation, and Plugin SPI dispatch
for every method below. This is the D11 decision from
`research-toolkit-alignment.md` and is anchored at the per-domain
components (`cpt-cf-usage-collector-component-ingestion-gateway`,
`cpt-cf-usage-collector-component-query-gateway`,
`cpt-cf-usage-collector-component-deactivation-handler`,
`cpt-cf-usage-collector-component-metric-catalog`) — each realized
as a service-layer component inside the trait implementation that
calls the shared `authz_scope` helper (a thin wrapper over
`PolicyEnforcer::access_scope_with`, matching the
`account-management/src/domain/authz.rs` pattern), not as Tower /
`OperationBuilder` middleware.

Concretely:

- `OperationBuilder::authenticated()` performs bearer-auth
  resolution and injects a `SecurityContext` extractor. Nothing
  beyond that runs at the framework layer.
- The REST handlers in the gear crate are thin pass-throughs that
  map REST DTO → domain, call
  `UsageCollectorClientV1::<op>(ctx, domain_query)`, map domain →
  REST DTO, and return. They do NOT perform PDP enforcement,
  Metric-existence validation, or plugin dispatch themselves.
- Inside each `UsageCollectorClientV1::<op>` method, the trait
  implementation performs (in order):
  1. PDP call against the resolved `SecurityContext` for the
     operation under attempt; denial yields the `Authorization`
     error variant.
  2. `PdpConstraint` composition against the caller-supplied filters
     (intersection — user filters can only narrow PDP-authorized
     scope).
  3. Metric-existence validation against the in-process Metrics
     Catalog projection (`cpt-cf-usage-collector-component-metric-catalog`)
     where the operation references a `metric_gts_id`; an
     unregistered identifier yields the `UnknownMetric` error
     variant before plugin dispatch.
  4. Plugin SPI dispatch (`UsageCollectorPluginV1::<spi_method>`)
     for persistence / aggregation / raw-page / deactivation /
     catalog operations as appropriate.
  5. Domain-level translation of `UsageCollectorPluginError` into
     `UsageCollectorError` per §"Error Taxonomy".

This composition keeps in-process SDK callers and out-of-process
REST callers traversing the same authorization, validation, and
plugin-dispatch path — there is no REST-only auth code, no
duplicate Metric-existence check at the handler layer, and no
PDP-double seam between the SDK and the REST surface.

Sources: `research-toolkit-alignment.md` §1 D11 (PDP enforcement
placement); DESIGN §3.2 Query Gateway Responsibility scope; §3.9.6
Authorization Architecture; phase-02 §"Public SDK Surface" and
§"Validation And Testing Ideas" (PDP-double seam).

### No dedicated `compensate` method — compensation rides the emit path

There is NO dedicated `compensate` operation on the
`UsageCollectorClientV1` SDK trait. Compensation rides the unified emit
path — Method 1 `submit_usage_record` and Method 2 `submit_usage_records`
— with `entry_type = "compensation"`, `corrects_id` set to the
referenced `usage` row's `UsageRecord.id`, and a strictly-negative
`value` (counters only — `gauge + compensation` is rejected by the
four-cell value matrix). Rationale: consistent PDP attribution on every
ingestion call and mandatory idempotency-key handling on every
ingestion call — one ingestion path, one set of guarantees, and no
second seam where authorization or idempotency could drift. This locks
the `api_shape = single-path` decision recorded in
`cpt-cf-usage-collector-adr-usage-compensation`,
`cpt-cf-usage-collector-fr-usage-compensation`, and
`cpt-cf-usage-collector-flow-usage-emission-compensation` (inlined in
`features/usage-emission.md`); the equivalent posture is mirrored on
the REST surface (no dedicated `compensate` endpoint) and on the
Plugin SPI (no dedicated `compensate` SPI call).

Sources: `cpt-cf-usage-collector-adr-usage-compensation`;
`cpt-cf-usage-collector-fr-usage-compensation`; Phase 5 handoff
§"Locked invariants preserved by Phase 5" (`api_shape` bullet);
Phase 6 handoff §"Locked Decisions Carried Forward by Phase 6"
(`api_shape = single ingestion path` bullet) and §"Compensation Flow
Text".

### Method 1 — Submit single usage record

- Identifier: `submit_usage_record`.
- Realizes: `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-seq-emit-usage`.
- SecurityContext: required and passed (as the leading `&SecurityContext`
  argument) to the per-component `authz_scope` helper invoked by
  `cpt-cf-usage-collector-component-ingestion-gateway` for ingestion
  authorization against the attribution tuple
  `(tenant, resource, source_gear, Metric)` and additionally
  `subject` when present.
- Unified ingestion path — single emit operation: this Method is the SDK
  surface for BOTH ordinary usage emission AND the counter
  value-reversal flow described as
  `cpt-cf-usage-collector-flow-usage-emission-compensation` in
  `features/usage-emission.md`. There is NO dedicated `compensate` SDK
  method on `UsageCollectorClientV1`; an explicit note appears at the
  start of §"Method Contracts" explaining the rationale (consistent PDP
  attribution + mandatory idempotency-key handling on every ingestion).
- Structural inputs (encapsulated in a `UsageRecordSubmission`,
  named parameters, order-insensitive):
  - `tenant_id` — opaque tenant identifier; required.
  - `resource` — `ResourceRef`; required.
  - `subject` — `SubjectRef`; optional.
  - `source_gear` — opaque source-gear identifier; required.
  - `gts_id` — GTS metric identifier string (suffixed `~`); required.
    MUST resolve to a row in `metric_catalog` per ADR 0012. This is
    the same value persisted by the plugin as the FK column on
    `usage_records`; no UUID derivation is performed by the trait or
    by the plugin.
  - `value` — signed `Number`; required. Permitted sign is determined
    jointly by the referenced metric's derived `kind` (computed from
    `gts_id`'s prefix per ADR 0012) and the submission's `entry_type`
    per the four-cell value matrix below. The SDK MUST accept
    strictly-negative numbers (the trait MUST NOT pre-clamp,
    pre-reject, or sign-flip `value`).
  - `timestamp` — UTC `Timestamp`; required.
  - `idempotency_key` — `IdempotencyKey`; required.
  - `entry_type` — `EntryType` (`cpt-cf-usage-collector-entity-entry-type`);
    optional, default `"usage"`; permitted values `"usage"` and
    `"compensation"`.
  - `corrects_id` — opaque `UsageRecord.id`; optional. MUST be set when
    `entry_type = "compensation"`; MUST be unset when
    `entry_type = "usage"`.
  - `metadata` — `RecordMetadata` (key/value map; string-typed values);
    optional. Validated at the gateway L1 against the referenced
    metric's `metadata_fields` list per ADR 0012 (closed shape, keyed
    by `gts_id`): every key MUST be a member of `metadata_fields`,
    every value is treated as `String`, and undeclared keys surface
    as `UnknownMetadataKey { gts_id, key }` (see §"Error Taxonomy").
    Plugins do NOT re-implement metadata validation.
- Four-cell value matrix (informational; server-enforced — the SDK MUST
  document the matrix and MUST NOT re-validate it locally):

  | `MetricKind` | `entry_type`   | Allowed `value`      | Outcome on violation              |
  | ------------ | -------------- | -------------------- | --------------------------------- |
  | `counter`    | `usage`        | `value >= 0`         | `Validation` (existing)           |
  | `counter`    | `compensation` | `value < 0` (strict) | `Validation` (existing)           |
  | `gauge`      | `usage`        | any signed value     | n/a                               |
  | `gauge`      | `compensation` | (rejected)           | `GaugeCompensationRejected` (NEW) |

- L1 `corrects_id` rule (informational; server-enforced — the SDK MUST
  document the rule and MUST NOT re-validate it locally). When
  `corrects_id` is present on the emit call, the server enforces the
  following preconditions on the referenced row R:
  1. R MUST exist. Violation surfaces the `CorrectsIdNotFound` error
     variant.
  2. R.`entry_type` MUST equal `usage`. Violation surfaces the
     `CorrectsIdWrongEntryType` error variant (compensating a
     compensation is a non-goal).
  3. R.`tenant_id` MUST equal the caller's `tenant_id` AND
     R.`gts_id` MUST equal the caller's `gts_id` per ADR 0012.
     Violation surfaces the `CorrectsIdWrongScope` error variant.
  4. R MUST be active (`status = Active`, not deactivated). Violation
     surfaces the `CorrectsIdInactive` error variant; this rule also
     realizes the concurrency guard against a compensation arriving
     mid-deactivation per
     `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`.

  There is NO L2 remaining-amount check at the SDK layer; per-record
  remaining-amount tracking is an explicit non-goal of the gear.

- Validation behaviour (executed in order before plugin dispatch):
  1. Missing or invalid `SecurityContext` (authentication is owned by
     the ToolKit gateway upstream of the collector) yields the
     `Authentication` error variant and no record is persisted.
  2. PDP denial yields the `Authorization` error variant and no record
     is persisted.
  3. Structural attribution validation (required fields present;
     `subject_id` present when `subject` is supplied; `subject_type`
     only with `subject_id`) yields the `Validation` error variant on
     failure.
  4. Missing `idempotency_key` yields the `Validation` error variant
     (mandatory-idempotency invariant).
  5. Oversized `metadata` (default cap 8 KiB per record, operator
     configurable) yields the `Validation` error variant. Any
     metadata key not present in the referenced metric's declared
     `metadata_fields` yields the
     `UnknownMetadataKey { gts_id, key }` variant per ADR 0012
     (closed shape, keyed by `gts_id`); there is no preserved
     free-form remainder.
  6. Unknown `gts_id` (no row in `metric_catalog`) yields the
     `UnknownMetric` error variant per ADR 0012.
  7. Per the four-cell value matrix: `counter + usage` with `value < 0`,
     and `counter + compensation` with `value >= 0`, each yield the
     `Validation` error variant. `gauge + compensation` (any value)
     yields the `GaugeCompensationRejected` error variant. `gauge +
usage` passes through unchanged.
  8. Per the L1 `corrects_id` rule above: the four `corrects_id_*`
     preconditions surface, respectively, `CorrectsIdNotFound`,
     `CorrectsIdWrongEntryType`, `CorrectsIdWrongScope`, and
     `CorrectsIdInactive`. Submitting `corrects_id` while
     `entry_type = "usage"` (or omitting `corrects_id` while
     `entry_type = "compensation"`) yields the `Validation` error
     variant.
  9. Structural plugin unavailability (the host's selector cache is
     empty OR `ClientHub::try_get_scoped` returns `None`) yields the
     `PluginUnavailable` error variant;
     SPI timeouts yield the `PluginTimeout` error variant; other plugin
     errors yield the `PluginFailure` error variant.
  10. A same-key resubmission (same
      `(tenant_id, gts_id, idempotency_key)`) whose caller-supplied
      canonical fields (`value`, `timestamp`, `resource_ref`,
      `subject_ref`, `source_gear`, `entry_type`, `corrects_id`,
      `metadata`) differ from the stored record yields the
      `IdempotencyConflict` error variant (`DedupOutcome::Conflict`)
      and no second record is persisted; an exact-equality retry
      instead yields `DedupOutcome::Deduplicated` on the `Ok` arm.
- SDK enforcement posture: the SDK MUST NOT validate net non-negativity
  locally; per-tenant per-Metric net is owned by the server-side
  un-policed-net posture (DESIGN §3.10.3) and the SDK only relays the
  resulting state through `AggregationResult`. The SDK MUST surface the
  server's validation error codes faithfully and MUST NOT collapse the
  five new error variants into a single generic `Validation` variant.
- Declared error variants for this method (subset, full taxonomy below):
  `Authentication`, `Authorization`, `Validation`, `UnknownMetric`,
  `UnknownMetadataKey`, `GaugeCompensationRejected`,
  `CorrectsIdNotFound`, `CorrectsIdWrongEntryType`,
  `CorrectsIdWrongScope`, `CorrectsIdInactive`,
  `IdempotencyConflict`, `PluginUnavailable`, `PluginTimeout`,
  `PluginFailure`.
- Success output: `UsageRecordAck` carrying the plugin-minted record
  `id`, the accepted `DeactivationStatus` (always `Active` on
  acceptance), and the `DedupOutcome` indicator. The acknowledgement
  shape is identical for `entry_type = "usage"` and
  `entry_type = "compensation"` submissions — there is no
  compensation-specific ack variant. When the plugin silently absorbs
  an exact-equality retry, the acknowledgement reports the prior
  accepted record's `id` and `DedupOutcome::Deduplicated`; the
  acknowledgement never raises an exact-equality duplicate as an error.
  A same-key submission with any differing canonical field is NOT an
  acknowledgement — it surfaces as the `IdempotencyConflict` error
  variant (`context.reason = idempotency_conflict`, AIP-193
  AlreadyExists / 409), distinct from the keyless `idempotency`
  rejection.
- Latency budget: total p95 200 ms per the ingestion latency budget
  (`cpt-cf-usage-collector-nfr-ingestion-latency`); the budget is the
  same for both `entry_type` values.

Sources: phase-01 §"Ingestion (covers
`cpt-cf-usage-collector-fr-ingestion`,
`cpt-cf-usage-collector-fr-idempotency`)", §"Sequences relevant to SDK
methods" emit row, §"Cross-Entity Invariants"; phase-02 §"SDK Method
Inputs And Outputs" / "Ingestion", §"Errors And Problem Mapping";
Phase 2 handoff §"Finalized §2.1 `UsageRecord` field additions" and
§"Four-cell validation matrix (verbatim, render in any artifact that
needs it)"; Phase 6 handoff §"Compensation Flow Text (verbatim
handoff)" and §"L1 `corrects_id` Rule Text (verbatim, for downstream
SPI / SDK / OpenAPI quoting)";
`cpt-cf-usage-collector-adr-usage-compensation`;
`cpt-cf-usage-collector-fr-usage-compensation`.

### Method 2 — Submit batched usage records

- Identifier: `submit_usage_records`.
- Realizes: `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`,
  `cpt-cf-usage-collector-seq-emit-usage`.
- SecurityContext: same as Method 1; PDP authorization is performed
  per record against the full attribution tuple.
- Structural inputs: a non-empty list of `UsageRecordSubmission`
  values. Each submission carries the same fields as in Method 1,
  including the optional `entry_type` (default `"usage"`) and the
  optional `corrects_id` (required when `entry_type = "compensation"`,
  forbidden when `entry_type = "usage"`). A single batched call MAY
  mix `usage` and `compensation` submissions in arbitrary order;
  per-record `entry_type` is independent across the list.
- Validation behaviour: each record is validated independently using
  the same rules as Method 1, including the four-cell value matrix and
  the L1 `corrects_id` rule. Per-record outcomes are reported in the
  return list in input order. The call as a whole is rejected only
  when the `SecurityContext` is missing or the input list is empty;
  each yields the `Validation` error variant on the call. Per-record
  validation failures, PDP denials, unknown Metrics, gauge-compensation
  rejections (the `GaugeCompensationRejected` error variant), invalid
  `corrects_id` references (the four `CorrectsId*` error variants),
  same-key canonical-field-mismatch conflicts (the
  `IdempotencyConflict` error variant / `DedupOutcome::Conflict`, as in
  Method 1), and plugin errors are reported as per-record `Err`-shaped
  entries within the result list — never as a whole-call rejection —
  while the call returns `Ok` on the list as a whole; this aligns with
  the deterministic per-record acceptance acknowledgement promise of the
  Ingestion Gateway.
- Success output: a list of per-record results, each carrying either
  a `UsageRecordAck` or a `UsageCollectorError`, in the same length
  and order as the input list.
- Latency and throughput: bounded by
  `cpt-cf-usage-collector-nfr-ingestion-latency` per record and by
  `cpt-cf-usage-collector-nfr-throughput` and
  `cpt-cf-usage-collector-nfr-batch-and-report-timing` at the batch.

Sources: phase-01 §"Ingestion" and §"Sequences relevant to SDK
methods"; phase-02 §"SDK Method Candidates" candidate 2, §"SDK Method
Inputs And Outputs" / "Ingestion".

### Method 3 — Aggregated query

- Identifier: `query_usage_aggregated`.
- Realizes: `cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-seq-query-aggregated`.
- SecurityContext: required; passed (as the leading `&SecurityContext`
  argument) to the per-component `authz_scope` helper invoked by
  `cpt-cf-usage-collector-component-query-gateway` for read
  authorization; PDP-returned constraints are intersected with the
  caller-supplied filters before plugin dispatch.
- Structural inputs: one `AggregationQuery` value. `time_range`,
  `metric_gts_id`, and `aggregation` are required; `tenant_id`,
  `resource`, `subject`, `source_gear`, and `group_by` are optional.
  Per ADR 0012, `group_by` admits the fixed dimensions (tenant,
  resource, subject, source gear, authorized time-period
  groupings) **plus the per-metric declared keys** resolved per
  request from the metric referenced by `metric_gts_id` — every
  key in the metric's `metadata_fields` list. `$filter` over the
  same dimension set accepts `eq` / `in` on `String`-typed values;
  the gateway resolves the per-metric declared keys once before
  composing PDP constraints and dispatching to the plugin.
  Cross-metric aggregation remains a structural non-goal — exactly
  one `metric_gts_id` per query.
  User-supplied filters can only narrow the PDP-authorized scope.
- Validation behaviour:
  1. Missing/invalid `SecurityContext` (authentication owned by the
     ToolKit gateway upstream) yields the `Authentication` error
     variant; PDP denial yields the `Authorization` error variant.
  2. Missing required `time_range`, missing or duplicated
     `metric_gts_id`, or unsupported `aggregation` yields the
     `Validation` error variant. A `group_by` or `$filter` reference
     to a key not in `metadata_fields` (or to an undeclared fixed
     dimension) also yields `Validation` (declared keys are
     resolved per request from the metric's `metadata_fields` list
     per ADR 0012).
  3. Unregistered `metric_gts_id` yields the `UnknownMetric` error
     variant and is rejected before plugin dispatch.
  4. Plugin-side failures map to `PluginUnavailable`, `PluginTimeout`, or
     `PluginFailure` as in Method 1.
- Success output: `AggregationResult` (`metric_gts_id`, `aggregation`,
  list of buckets). An empty result set inside the authorized scope
  returns an `AggregationResult` with an empty `buckets` list and is
  not an error.
- Latency budget: total p95 500 ms for a 30-day single-tenant
  aggregated query per `cpt-cf-usage-collector-nfr-query-latency`. The
  PRD NFR is authoritative for memory bounds; the SDK trait does not
  enforce numeric caps on result size at the SDK boundary, leaving
  them to the REST/OpenAPI contract and to the plugin.

Sources: phase-01 §"Aggregated query (covers
`cpt-cf-usage-collector-fr-query-aggregation`)", §"AggregationQuery",
§"AggregationResult", §"Sequences relevant to SDK methods" aggregated
row; phase-02 §"SDK Method Inputs And Outputs" / "Aggregated query".

### Method 4 — Raw keyset-paginated query

- Identifier: `query_usage_raw`.
- Realizes: `cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-seq-query-raw`.
- SecurityContext: required; the trait implementation invokes the
  per-component `authz_scope` helper inside
  `cpt-cf-usage-collector-component-query-gateway` (passing
  `&SecurityContext` first) for read authorization and intersects
  PDP-returned constraints with the caller-supplied filters before
  plugin dispatch (D11 — see §"PDP enforcement and plugin dispatch
  inside the trait implementation").
- Canonical Rust signature:

  ```rust
  /// Performs PDP enforcement, Metric-existence validation, PDP-constraint
  /// composition against the filter AST, and plugin dispatch — all inside
  /// this implementation (D11). Cursor handling is invisible to SDK
  /// callers: they pass `page_after` / `limit` (via `ODataQuery`) and
  /// read `page_info.next_cursor` from the returned `Page<UsageRecord>`.
  async fn query_usage_raw(
      &self,
      ctx: &SecurityContext,
      query: ODataQuery<UsageRecordFilterField>,
  ) -> Result<toolkit_odata::Page<UsageRecord>, UsageCollectorError>;
  ```

  Where `ODataQuery<UsageRecordFilterField>` is the domain-side
  struct exposing the parsed inputs (see §"Query types and views"):

  ```rust
  pub struct ODataQuery<F: toolkit_odata::filter::FilterField> {
      pub filter_ast: toolkit_odata::filter::FilterNode<F>,
      pub order:      toolkit_odata::ODataOrderBy,
      pub page_after: Option<toolkit_odata::Keyset>,
      pub limit:      u64,
  }
  ```

- Structural inputs: one `ODataQuery<UsageRecordFilterField>` value
  carrying the **required** `metric_gts_id` (see §"Query types and
  views" — promoted to REQUIRED per ADR 0012 so the gateway can
  resolve the metric's declared keys before admitting
  `filter_ast`), the parsed `filter_ast` (already PDP-constrained
  and admitted against fixed fields + per-metric `metadata_fields`
  on entry to the plugin dispatch step), the canonical `timestamp asc,
id asc` `order`, an optional `page_after` keyset (gateway-decoded
  from the caller-supplied `CursorV1`), and a bounded `limit`.
- Validation behaviour (executed by the trait implementation, in
  order, before plugin dispatch):
  1. Missing or invalid `SecurityContext` (authentication is owned by
     the ToolKit gateway upstream of the collector) yields the
     `Authentication` error variant.
  2. PDP denial yields the `Authorization` error variant. PDP
     constraints are composed against `filter_ast` so the plugin
     receives a filter AST that is authoritatively narrowed.
  3. `Validation` is surfaced for structural failures the SDK can
     detect at this boundary — non-positive `limit`, a missing
     `metric_gts_id` (REQUIRED per ADR 0012), a `filter_ast` that
     references a field outside the admissible set (fixed
     `UsageRecord` fields plus per-metric declared keys resolved
     from `metric_gts_id`'s `metadata_fields`), an operator outside
     the per-field allowance (dimension filters accept `eq` / `in`
     only over `String`-typed values), or an `order` other than the
     canonical raw-query order.
  4. Unregistered `metric_gts_id` (either the request-level value or
     a reference inside `filter_ast`) yields the `UnknownMetric`
     error variant and is rejected before plugin dispatch (D11 —
     Metric-existence validation inside the trait implementation).
  5. Plugin-side failures map to `PluginUnavailable`, `PluginTimeout`,
     or `PluginFailure`.
- Success output: `toolkit_odata::Page<UsageRecord>` with `items`
  (list of `UsageRecord`, each carrying its `status`) and
  `page_info: PageInfo { next_cursor, prev_cursor, limit }` whose
  `next_cursor` is the opaque `CursorV1` minted by the gateway from
  the plugin-returned last-row `Keyset`. An empty match within the
  authorized scope returns an empty `items` list with no
  `next_cursor` and is not an error.
- Pagination behaviour: cursor envelopes are gateway-minted and
  opaque to SDK callers. Callers pass `page_after` / `limit`
  implicitly by threading the `CursorV1` they read from
  `page_info.next_cursor` into the next call; they never observe
  `page_after` or `Keyset` directly. Cursor decode failure, order
  mismatch, and filter mismatch are surfaced as canonical Problem
  responses on the REST surface (`cursor_decode`, `order_mismatch`,
  `filter_mismatch`) and as `Validation` on the SDK surface — no
  separate cursor-validity error variant exists.

Sources: phase-01 §"Raw query (covers
`cpt-cf-usage-collector-fr-query-raw`)", §"RawQuery",
§"Sequences relevant to SDK methods" raw row; phase-02 §"SDK Method
Inputs And Outputs" / "Raw query"; `out/phase-01-domain-contracts.md`
§3–§4; `research-toolkit-alignment.md` §1 D9 (SDK trait return type),
D10 (gateway-owned cursor lifecycle), D11 (PDP placement).

### Method 5 — Deactivate usage event

- Identifier: `deactivate_usage_record`.
- Realizes: `cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-seq-deactivate-event`.
- SecurityContext: required; passed (as the leading `&SecurityContext`
  argument) to the per-component `authz_scope` helper invoked by
  `cpt-cf-usage-collector-component-deactivation-handler` for
  operator authorization.
- Structural inputs: the target `UsageRecord.id`. Deactivation applies
  to a row of ANY `entry_type` value (`usage` or `compensation`); the
  request parameters are unchanged by the compensation primitive.
- Validation behaviour:
  1. Missing/invalid `SecurityContext` (authentication owned by the
     ToolKit gateway upstream) yields the `Authentication` error
     variant; PDP denial yields the `Authorization` error variant.
  2. Plugin-side failures map to `PluginUnavailable`, `PluginTimeout`, or
     `PluginFailure`.
- Rejection behaviour (in addition to missing/invalid SecurityContext,
  PDP denial, and validation):
  - An already-`Inactive` target record yields the `AlreadyInactive`
    error variant; no state change occurs and no other field is
    mutated. This realizes the monotonicity invariant on the SDK
    boundary.
  - An unknown `UsageRecord.id` yields the `NotFound` error variant.
- Success output: `DeactivationAck` carrying:
  - `id` — the targeted `UsageRecord.id` (the explicitly-deactivated
    row, regardless of its `entry_type`).
  - `status` — the resulting `DeactivationStatus` (always `Inactive`
    after a successful transition).
  - `cascaded_compensation_ids` — `List<Id>`. The set of compensation
    record ids that were active at deactivation time and were flipped
    to inactive as a depth-1 cascade alongside the targeted row. MAY
    be empty. Order is unspecified. MUST be present (possibly empty)
    on every successful deactivate response. The list is non-empty
    only when the targeted row has `entry_type = "usage"` AND at
    least one active compensation row referenced it via
    `corrects_id`; deactivating a row with
    `entry_type = "compensation"` is a single-row, no-cascade
    operation (the list is empty by construction per
    `cpt-cf-usage-collector-adr-usage-compensation` non-goals).

  A successful return implies the targeted row was `Active` before the
  call AND every id in `cascaded_compensation_ids` was likewise `Active`
  before the call and is now `Inactive`. The cascade is atomic at the
  plugin boundary per
  `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`;
  partial cascade is structurally impossible.

- Monotonicity: the only permitted transition is `Active -> Inactive`;
  no reactivation exists; deactivation is atomic at the plugin
  boundary; no other field of any flipped row is mutated. The one-way
  latch applies uniformly to the primary row and to every
  cascade-flipped compensation row.

Sources: phase-01 §"DeactivationStatus", §"Event deactivation",
§"Sequences relevant to SDK methods" deactivate row; phase-02 §"SDK
Method Inputs And Outputs" / "Deactivation", §"Plugin SPI Boundary"
atomic-transition bullet; Phase 5 handoff §"Finalized cascade algorithm
text", §"Cascade response shape", and §"Forward references" Phase 8
bullet; `cpt-cf-usage-collector-adr-usage-compensation`;
`cpt-cf-usage-collector-fr-usage-compensation`;
`cpt-cf-usage-collector-entity-entry-type`.

### Method 6 — Register metric

- Identifier: `register_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-registration`,
  `cpt-cf-usage-collector-seq-register-metric`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012).
- SecurityContext: required; passed (as the leading `&SecurityContext`
  argument) to the per-component `authz_scope` helper invoked by
  `cpt-cf-usage-collector-component-metric-catalog` for operator
  authorization. The trait implementation performs the same PDP call
  reached by the REST handler — there is one authorization site, not
  two.
- Canonical Rust signature:

  ```rust
  async fn register_metric(
      &self,
      ctx: &SecurityContext,
      input: RegisterMetricInput,
  ) -> Result<MetricRecord, UsageCollectorError>;
  ```

- Structural inputs:
  `RegisterMetricInput { gts_id, metadata_fields }`.
  `gts_id` is the GTS metric identifier (suffixed `~`,
  deployment-unique) and serves as the catalog primary key per
  ADR 0012; it MUST begin with one of the two reserved kind base
  type id prefixes — `gts.cf.core.usage.counter.v1~` or
  `gts.cf.core.usage.gauge.v1~` — from which `kind ∈ {counter, gauge}`
  is derived per lookup. `metadata_fields` is a `Vec<String>` — the
  closed list of declared metadata keys for this metric (unique
  non-empty strings; all corresponding values are typed as `String`
  end-to-end). The payload carries NO `kind` field and NO `traits`
  map.
- Caller / plugin validation split: the gateway-side trait
  implementation performs `gts_id` kind-prefix validation against
  the two reserved base type ids and `metadata_fields`
  well-formedness (unique non-empty strings) BEFORE dispatching to
  the plugin SPI's `register_metric` (see `plugin-spi.md`
  §"Method 6"). The plugin enforces only structural persistence
  constraints (`gts_id` uniqueness, atomic insert).
- Persistence target: the plugin-owned `metric_catalog` table per
  ADR 0012. The catalog PK is `gts_id`; no UUID derivation is
  performed by the gateway or the plugin. The gateway L1 catalog
  cache invalidates the entry for the new row synchronously on `Ok`
  per ADR 0012.
- Validation behaviour (executed in order):
  1. Missing/invalid `SecurityContext` yields `Authentication`; PDP
     denial yields `Authorization`.
  2. Malformed `gts_id` (not suffixed `~`, not deployment-unique
     shape) or malformed `metadata_fields` (duplicate keys, empty
     strings, non-string entries) yields `Validation`.
  3. A `gts_id` whose prefix matches neither
     `gts.cf.core.usage.counter.v1~` nor
     `gts.cf.core.usage.gauge.v1~` yields the
     `InvalidKindPrefix { gts_id }` variant.
  4. Collision with a previously registered metric (plugin UNIQUE
     violation on `gts_id`) whose payload differs from the stored
     row yields `MetricAlreadyExists`. An identical payload
     resubmission MUST be idempotent per the SPI contract and
     returns the stored `MetricRecord` on `Ok`.
- Success output: `MetricRecord` carrying the durably stored row
  (`gts_id`, `metadata_fields`, `created_at`).
- Idempotency: registration is idempotent on byte-equal payloads;
  any payload divergence on a colliding `gts_id` is reported as
  `MetricAlreadyExists` rather than silently absorbed.

Sources: ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`);
phase-01 §"Metric lifecycle"; DESIGN §3.6 Register Metric; DESIGN
§3.7 Table `metric_catalog`; `plugin-spi.md` §"Method 6 — Register
metric".

### Method 7 — Read metric

- Identifier: `read_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012).
- SecurityContext: required; passed to `authz_scope` for read
  authorization. Metrics are platform-global, so PDP narrowing is
  applied at the read-permission granularity, not per-tenant.
- Canonical Rust signature:

  ```rust
  async fn read_metric(
      &self,
      ctx: &SecurityContext,
      gts_id: GtsId,
  ) -> Result<MetricRecord, UsageCollectorError>;
  ```

- Behaviour: returns the catalog row including `gts_id`,
  `metadata_fields`, and `created_at` (with `kind` exposed via a
  derived accessor computed from `gts_id`). The trait implementation
  reads from the gateway L1 catalog cache when warm and falls back
  to the plugin SPI's `read_metric` on a miss (which returns
  `Option<CatalogRow>` per `plugin-spi.md` §"Method 7"); a missing
  identifier yields `MetricNotFound`.
- Success output: `MetricRecord`.
- Idempotency: pure read; no observable effect on repeat calls.

Sources: ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`);
phase-01 §"Metric lifecycle"; DESIGN §3.6 Get Metric;
`plugin-spi.md` §"Method 7 — Read metric".

### Method 8 — List metrics

- Identifier: `list_metrics`.
- Realizes: `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012).
- SecurityContext: required; passed to `authz_scope` for read
  authorization.
- Canonical Rust signature:

  ```rust
  async fn list_metrics(
      &self,
      ctx: &SecurityContext,
      filter: ListMetricsFilter,
      paging: PageParams,
  ) -> Result<Page<MetricRecord>, UsageCollectorError>;
  ```

- Structural inputs: `ListMetricsFilter { kind }` (see
  §"Method-specific output types") plus `PageParams { cursor, limit }`.
  The `cursor` envelope is opaque to callers (minted and decoded by
  the gateway via `toolkit_odata::CursorV1`).
- Behaviour: returns the registered metric catalog per ADR 0012 via
  the plugin SPI's `list_metrics` (`plugin-spi.md` §"Method 8"). Each
  row includes `gts_id`, `metadata_fields`, and `created_at`; the
  derived `kind` accessor on `MetricRecord` lets consumers inspect
  `kind ∈ {counter, gauge}` without an extra round-trip and without
  a stored column.
- Success output: `Page<MetricRecord>` with `items` and `page_info`
  carrying `next_cursor`, `prev_cursor`, and `limit`.
- Latency: bounded by the plugin's `list_metrics` per-call timeout.

Sources: ADR 0012; phase-01 §"Metric lifecycle"; DESIGN §3.6 List
Metrics; `plugin-spi.md` §"Method 8 — List metrics".

### Method 9 — Delete metric

- Identifier: `delete_metric`.
- Realizes: `cpt-cf-usage-collector-fr-metric-deletion`,
  `cpt-cf-usage-collector-seq-delete-metric`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012).
- SecurityContext: required; passed (as the leading `&SecurityContext`
  argument) to `authz_scope` inside
  `cpt-cf-usage-collector-component-metric-catalog`.
- Canonical Rust signature:

  ```rust
  async fn delete_metric(
      &self,
      ctx: &SecurityContext,
      gts_id: GtsId,
  ) -> Result<(), UsageCollectorError>;
  ```

- Delete protocol (executed in order by the trait implementation per
  ADR 0012):
  1. PDP authorize. Denial yields `Authorization`.
  2. Dispatch to the plugin SPI's `delete_metric`
     (`plugin-spi.md` §"Method 9"). The plugin attempts the row
     delete inside a single backend transaction; the
     `usage_records.gts_id` `ON DELETE RESTRICT` foreign key fires
     natively on a referenced metric per ADR 0012.
  3. Translate plugin outcomes per the dispatch boundary:
     `MetricNotFound { gts_id }` from the plugin lifts to
     `UsageCollectorError::MetricNotFound`;
     `MetricReferenced { gts_id, sample_ref_count }` from the plugin
     lifts to `UsageCollectorError::MetricReferenced` and is
     surfaced to REST callers as HTTP 409 (callers MUST expect 409
     on referenced metrics). On `Ok`, the gateway L1 catalog cache
     evicts the deleted row per ADR 0012.
- Success output: `()`.
- Idempotency: a second `delete_metric` call for the same `gts_id`
  yields `MetricNotFound` (the plugin row is gone after the first
  successful delete). A delete against a referenced metric never
  silently succeeds — it always surfaces `MetricReferenced`.

Sources: ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`);
phase-01 §"Metric lifecycle"; DESIGN §3.6 Delete Metric; DESIGN
§3.7 referential delete semantics; `plugin-spi.md` §"Method 9 —
Delete metric".

> **Removed in ADR 0012.** A prior `read_metric_chain` method
> (Method 10) walked `parent_type_uuid` from a target metric row up
> to the boot-seeded platform base. ADR 0012 flattens the metric
> type model (no `parent_type_uuid`, no ancestor walk); the method
> and its plugin SPI counterpart are removed from this trait.

## Error Taxonomy

All SDK trait methods return `Result<…, UsageCollectorError>`.
`UsageCollectorError` is declared in `usage-collector-sdk/src/error.rs`
as a flat `thiserror::Error` enum and is the public error envelope for
the SDK surface. The SDK crate **does NOT depend on
`toolkit-canonical-errors`**; consumers pattern-match variants of
`UsageCollectorError` directly. This mirrors the platform standard set
by `account-management-sdk`, `credstore-sdk`, `authn-resolver-sdk`, and
`authz-resolver-sdk`.

The host crate (`usage-collector`) provides
`From<UsageCollectorError> for toolkit_canonical_errors::CanonicalError`
in `usage-collector/src/infra/sdk_error_mapping.rs`; REST handlers in
`usage-collector/src/api/rest/handlers.rs` return
`Result<DtoT, UsageCollectorError>` and the canonical RFC-9457
`Problem` envelope is produced by `CanonicalError`'s built-in
`IntoResponse` impl. The `?` operator plus the `From` impl plus
`IntoResponse` drive the wire envelope — handlers do not call
`.map_err(|e| problem(...))` per-route. The SDK never returns a
`ProblemResponse` directly. The AIP-193 mapping (variant → category →
HTTP status) is documented in DESIGN.md §3.3 Error Envelopes.

Variant catalog:

- `Authentication` — Missing, expired, or otherwise unresolved
  `SecurityContext`. Surfaces the fail-closed posture against missing
  credentials.
- `Authorization` — PDP denial on the requested operation. Reported
  uniformly for read and write denials; the SDK does not surface the
  PDP reason payload.
- `Validation` — Structural or semantic validation failure: missing
  required field, malformed identifier, malformed `time_range`,
  oversized `metadata`, missing `idempotency_key`, non-positive
  raw-query `limit`, missing `metric_gts_id` on a raw query
  (REQUIRED per ADR 0012), a `filter_ast` referencing a field
  outside the admissible set (fixed `UsageRecord` fields plus
  per-metric declared keys resolved per request from
  `metric_gts_id`'s `metadata_fields` list) or an operator outside
  the per-field allowance (dimension filters accept `eq` / `in`
  over `String`-typed values), an `order` other than the canonical
  raw-query order, a gateway-rejected cursor (cursor-decode /
  order-mismatch / filter-mismatch), `counter + usage` with
  `value < 0`, `counter + compensation` with `value >= 0`,
  `entry_type = compensation` without `corrects_id`, or
  `entry_type = usage` with `corrects_id` present.
- `UnknownMetric` — Ingestion or aggregated query referenced a
  `gts_id` that does not exist in `metric_catalog` per ADR 0012.
  Distinct from `MetricNotFound` (which is raised by the
  catalog-admin methods 7 / 9 on a missing target row);
  `UnknownMetric` is reserved for the ingestion / query references
  that fail metric-existence validation inside the trait
  implementation before plugin dispatch.
- `UnknownMetadataKey` — Ingestion supplied a `metadata` map carrying
  a key that is not a member of the referenced metric's declared
  `metadata_fields` list per ADR 0012 (closed shape, keyed by
  `gts_id`). Carries the structured fields `{ gts_id, key }`:
  `gts_id` identifies the metric whose closed-shape contract
  rejected the key; `key` is the offending undeclared key name.
  Distinct from `Validation` because the failure originates at the
  gateway's L1 closed-shape check (not from a structural request
  error) and from `UnknownMetric` because the metric is known but
  the caller supplied a key outside its declared shape. Plugins do
  NOT re-implement metadata validation. Lifts to AIP-193
  `InvalidArgument` (HTTP 400) on the wire, surfaced as
  `Problem.context.reason="unknown_metadata_key"` with `gts_id` and
  `key` carried in `context`.
- `InvalidKindPrefix` — Catalog `register_metric` was called with a
  `gts_id` whose prefix matches neither of the two reserved kind
  base type ids (`gts.cf.core.usage.counter.v1~` and
  `gts.cf.core.usage.gauge.v1~`) per ADR 0012's 2026-06-02
  amendment. Carries the structured field `{ gts_id }`. Distinct
  from `Validation` because the request is structurally well-formed
  (the `gts_id` is a well-formed GTS identifier) — the rejection is
  the kind-prefix invariant check, not a missing or malformed-input
  error. Lifts to AIP-193 `InvalidArgument` (HTTP 400) on the wire,
  surfaced as `Problem.context.reason="invalid_kind_prefix"` with
  `gts_id` carried in `context`.
- `GaugeCompensationRejected` — Ingestion submitted a record with
  `entry_type = compensation` against a Metric whose `kind` is `gauge`;
  the four-cell value matrix forbids compensation on gauge Metrics
  (gauges already express down-movement directly). Distinct from
  `Validation` because the request is structurally well-formed and the
  rejection is the gauge-specific cell of the value matrix per
  `cpt-cf-usage-collector-entity-entry-type` and
  `cpt-cf-usage-collector-algo-usage-emission-kind-enforcement-on-ingest-v2`.
  Lifts to AIP-193 `FailedPrecondition` (HTTP 422) on the wire — the
  request is well-formed but violates a Metric-kind precondition.
  Surfaces on the wire as `Problem.context.reason="gauge_compensation_rejected"` per `usage-collector-v1.yaml`.
- `CorrectsIdNotFound` — Ingestion submitted a record carrying
  `corrects_id` that references a `UsageRecord.id` not present in the
  caller-visible store (or visible-but-out-of-tenant-scope behind the
  PDP narrowing). Distinct from `NotFound` (which is reserved for the
  deactivation method's missing-target case) and from
  `CorrectsIdWrongScope` (which is raised when the row exists but is
  outside `(tenant_id, gts_id)` scope per ADR 0012). Lifts to AIP-193
  `NotFound` (HTTP 404) on the wire.
  Surfaces on the wire as `Problem.context.reason="corrects_id_not_found"` per `usage-collector-v1.yaml`.
- `CorrectsIdWrongEntryType` — Ingestion submitted a record carrying
  `corrects_id` that references a row whose `entry_type` is not `usage`
  (typically `compensation`); compensating a compensation is a non-goal
  per `cpt-cf-usage-collector-adr-usage-compensation`. Lifts to AIP-193
  `FailedPrecondition` (HTTP 409) on the wire.
  Surfaces on the wire as `Problem.context.reason="corrects_id_wrong_entry_type"` per `usage-collector-v1.yaml`.
- `CorrectsIdWrongScope` — Ingestion submitted a record carrying
  `corrects_id` that references a row whose `(tenant_id, gts_id)`
  does not match the incoming compensation's `(tenant_id, gts_id)`
  per ADR 0012. Cross-tenant or cross-Metric compensation is
  rejected. Lifts to AIP-193 `FailedPrecondition` (HTTP 409) on the
  wire.
  Surfaces on the wire as `Problem.context.reason="corrects_id_wrong_scope"` per `usage-collector-v1.yaml`.
- `CorrectsIdInactive` — Ingestion submitted a record carrying
  `corrects_id` that references a row whose `status` is `Inactive`
  (already deactivated, including a row that is concurrently in the
  process of being deactivated — the L1 "active" check serialises
  against the cascade transition per
  `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`).
  Distinct from `AlreadyInactive` (which is the deactivation method's
  rejection for a re-deactivation attempt). Lifts to AIP-193
  `FailedPrecondition` (HTTP 409) on the wire.
  Surfaces on the wire as `Problem.context.reason="corrects_id_inactive"` per `usage-collector-v1.yaml`.
- `NotFound` — Deactivation referenced a `UsageRecord.id` that does
  not exist.
- `AlreadyInactive` — Deactivation referenced a record whose `status`
  is already `Inactive`; rejecting the second deactivation realizes the
  monotonic-deactivation invariant on the SDK boundary
  (`cpt-cf-usage-collector-principle-monotonic-deactivation`).
- `MetricAlreadyExists` — Catalog `register_metric` was called with
  a `gts_id` already present in `metric_catalog` and whose request
  payload differs from the stored row (plugin UNIQUE violation
  surfaced as `MetricAlreadyExists { gts_id }` per ADR 0012 and
  `plugin-spi.md` §"Method 6"). An identical-payload resubmission
  is idempotent and returns the stored row on `Ok` — it does NOT
  raise this variant. Distinct from `Validation` because the
  duplicate is a domain-state conflict per
  `cpt-cf-usage-collector-seq-register-metric`, not a structural
  request error; lifts to AIP-193 `AlreadyExists` (HTTP 409) per
  DESIGN.md §3.3, surfaced on the wire as
  `Problem.context.reason="metric_already_exists"`. Name aligns with
  the plugin SPI variant `MetricAlreadyExists`.
- `IdempotencyConflict` — Ingestion submitted a record whose
  `(tenant_id, gts_id, idempotency_key)` collides with a stored
  record **but** whose caller-supplied canonical fields (`value`,
  `timestamp`, `resource_ref`, `subject_ref`, `source_gear`,
  `entry_type`, `corrects_id`, `metadata`) differ from that stored record (the
  `DedupOutcome::Conflict` outcome from the Plugin SPI; see
  §"Method-specific output types"). The second write is rejected
  fail-closed and never silently dropped per
  `cpt-cf-usage-collector-adr-mandatory-idempotency`. Distinct from
  `Validation` because the request is structurally well-formed (it
  carries a valid idempotency key) — this is a domain-state conflict,
  not a missing-key or malformed-input error. Distinct, too, from the
  keyless `idempotency` rejection: a record submitted WITHOUT the
  mandatory idempotency key is a `Validation` error lifting to AIP-193
  `InvalidArgument` (HTTP 400, `context.reason="idempotency"`), whereas
  `IdempotencyConflict` is a same-key divergent-content collision
  lifting to AIP-193 `AlreadyExists` (HTTP 409), surfaced on the wire as
  `Problem.context.reason="idempotency_conflict"` per DESIGN.md §3.3. An
  exact-equality retry is NOT this error — it is a deduplicated success
  (`DedupOutcome::Deduplicated` on the `Ok` arm).
- `MetricNotFound` — Catalog `read_metric` / `list_metrics` /
  `delete_metric` referenced a `gts_id` that does not exist in
  `metric_catalog` per ADR 0012. Carries the structured field
  `{ gts_id }`. Distinct from `NotFound` (which is reserved for
  `UsageRecord.id` misses on deactivation) and from `UnknownMetric`
  (which is raised when an ingestion or query references an
  unregistered metric). Lifts to AIP-193 `NotFound` (HTTP 404) per
  DESIGN.md §3.3, surfaced on the wire as the canonical `not_found`
  category with `context.resource_type="metric"` /
  `context.resource_name=<gts_id>`. Name aligns with the plugin SPI
  variant `MetricNotFound { gts_id }`.
- `MetricReferenced` — Catalog `delete_metric` was rejected by the
  plugin's `usage_records.gts_id` `ON DELETE RESTRICT` foreign key
  per ADR 0012. Carries the structured fields
  `{ gts_id, sample_ref_count }`: `sample_ref_count` is a bounded
  sample sufficient to surface "this metric still has rows" without
  scanning the entire table (the exact bound is plugin-tunable but
  MUST be at least `1`). Callers MUST expect HTTP 409 on referenced
  metrics per the REST contract. Lifts to AIP-193
  `FailedPrecondition` (HTTP 409) per DESIGN.md §3.3, surfaced on
  the wire as `Problem.context.reason="metric_referenced"` with
  `context.sample_ref_count`. Name aligns with the plugin SPI
  variant `MetricReferenced { gts_id, sample_ref_count }`.

> **Removed in ADR 0012.** Prior `DeclaredMetricImmutable` variant
> (raised against the gateway-local-from-config catalog) is no
> longer reachable: ADR 0012 retired the local-from-config catalog,
> leaving the plugin-DB catalog as the sole metric catalog. The
> equivalent rejection no longer exists. Where the variant was
> raised, the request now succeeds, surfaces `MetricAlreadyExists`,
> or surfaces `MetricNotFound` per the unified semantics.

- `PluginUnavailable` — Structural condition: the host had no scoped
  `dyn UsageCollectorPluginV1` client under
  `ClientScope::gts_id(&instance_id)` (the selector cache was empty OR
  `ClientHub::try_get_scoped` returned `None`) at the time of the call.
  Lifted by the host service from the structural fact; the SPI itself
  exposes no `Unready` variant and no `ready()` probe.
- `PluginTimeout` — The Plugin SPI call exceeded its declared per-call
  timeout.
- `PluginFailure` — The active storage plugin reported a classified
  error other than a timeout; structural unavailability is reported
  separately as `PluginUnavailable`.
- `ServiceUnavailable` — Non-plugin transient infrastructure failure;
  carries an optional `retry_after_seconds` hint exposed via
  `context.retry_after_seconds` per the canonical envelope. Wire-header
  treatment (whether a corresponding `Retry-After` HTTP header is set)
  is owned by `toolkit-canonical-errors`' `IntoResponse` and is not
  asserted by this gear's contract.
- `Internal` — Unclassified failure. The `detail` field **MUST** be
  DSN-free and pre-redacted at the construction site; no internal
  storage paths, credentials, or stack traces are surfaced through
  this variant.

**Group-membership helpers** (mirroring the platform standard set by
`account-management-sdk`): `UsageCollectorError` exposes category
predicates as part of its public surface so consumers can do
category-level handling without enumerating every variant.

- `is_not_found()` — `NotFound`, `UnknownMetric`,
  `MetricNotFound`, `CorrectsIdNotFound`.
- `is_unavailable()` — `PluginUnavailable`, `ServiceUnavailable`.
- `is_retryable()` — `PluginTimeout`, `PluginUnavailable`,
  `ServiceUnavailable`.
- `is_validation_error()` — `Validation`, `UnknownMetadataKey`,
  `InvalidKindPrefix`.
- `is_precondition_failed()` — `AlreadyInactive`,
  `MetricReferenced`, `GaugeCompensationRejected`,
  `CorrectsIdWrongEntryType`, `CorrectsIdWrongScope`,
  `CorrectsIdInactive`. Deactivation against an already-inactive record
  violates the monotonic-deactivation precondition on the SDK boundary
  (`cpt-cf-usage-collector-principle-monotonic-deactivation`);
  `MetricReferenced` is the referential-delete rejection enforced
  by the plugin's `usage_records.gts_id` `ON DELETE RESTRICT` FK per
  ADR 0012; the four compensation-precondition variants violate the
  compensation preconditions locked by
  `cpt-cf-usage-collector-adr-usage-compensation` and
  `cpt-cf-usage-collector-fr-usage-compensation`.
  All lift to AIP-193 `FailedPrecondition` per DESIGN.md §3.3
  (HTTP 400 for the deactivation case, HTTP 409 for the catalog,
  `MetricReferenced`, and `corrects_id` cases, HTTP 422 for the
  `gauge + compensation` cell, mirroring the canonical mapping for
  state conflicts).
- `is_already_exists()` — Returns true for `MetricAlreadyExists`
  (raised by the catalog write path when `register_metric` collides
  on `gts_id` with a divergent payload per ADR 0012) and
  `IdempotencyConflict` (raised by the ingestion path on a same-key
  submission whose canonical fields differ from the stored record);
  reserved for already-exists state conflicts and lifts to AIP-193
  `AlreadyExists` (HTTP 409) per DESIGN.md §3.3.
- `is_permission_denied()` — `Authorization`.

Variants without dedicated predicates (`Authentication`,
`PluginFailure`, `Internal`) are pattern-matched directly because they
are either terminal classifications or carry caller-specific payload.

Adding a new variant means extending the relevant helper in one place
rather than patching every call site.

Behavioural notes:

- Deactivation success returns `DeactivationAck` on the `Ok` variant;
  the rejection cases for an already-inactive record (`AlreadyInactive`)
  or an unknown record (`NotFound`) are surfaced as error variants per
  domain-model.md §2.9 and DESIGN.md
  `cpt-cf-usage-collector-principle-monotonic-deactivation`.
- Exact-equality duplicate ingestion submissions are reported through
  the `UsageRecordAck.dedup` indicator (`DedupOutcome::Deduplicated`) on
  the `Ok` variant rather than as errors. A same-key submission whose
  canonical fields differ from the stored record (`DedupOutcome::Conflict`)
  is NOT reported on the `Ok` arm — it surfaces as the
  `IdempotencyConflict` error variant (`context.reason =
idempotency_conflict`, AIP-193 AlreadyExists / 409), distinct from the
  keyless `idempotency` (`Validation`) rejection.
- Variant naming is canonical for this reference; the SDK crate may
  add per-variant context fields (such as a stable error code or a
  PDP reason category) as long as the public taxonomy preserves the
  domain classification above.

Sources: phase-01 §"Ingestion", §"Aggregated query", §"Raw query",
§"Event deactivation", §"Cross-Entity Invariants"; phase-02 §"Errors
And Problem Mapping", §"SDK Method Inputs And Outputs", §"ToolKit
Conventions" / "Error and Problem mapping".

## Versioning/Compatibility

- The SDK trait is one of three independently versioned public
  surfaces (REST API, SDK trait, Plugin SPI). Each surface evolves
  under a major-version stability contract
  (`cpt-cf-usage-collector-adr-contract-stability`,
  `cpt-cf-usage-collector-principle-contract-stability`,
  `cpt-cf-usage-collector-nfr-plugin-contract-stability`).
- The SDK trait's major version is encoded in the trait name suffix
  `V1`. A new major version (`V2`, and so on) is required for any
  breaking change.
- Within a major version only additive changes are permitted: new
  optional methods, new optional fields on input types, and new
  non-required variants on output enums. Removing methods, removing
  or renaming fields, narrowing accepted values, changing semantics,
  or introducing a new required input is a breaking change and
  requires a new major version.
- Deprecation flow: an SDK trait method or field scheduled for
  removal in the next major release must be marked `deprecated` in
  the SDK trait rustdoc at least one minor release before the major
  bump.
- At most one prior major version is supported concurrently per
  surface. The Plugin SPI carries the same posture, so a Usage
  Collector gear instance may serve callers of the current SDK
  major and the immediately prior SDK major in parallel during a
  deprecation window.
- Rust trait compatibility tests gate every PR against the prior
  major per the Contract test category
  (`cpt-cf-usage-collector-nfr-plugin-contract-stability`).
- The SDK trait declares per-call timeouts; the timeout values
  themselves are documented in the SDK rustdoc and bounded by the
  per-operation latency budgets (200 ms for ingestion p95, 500 ms
  for the 30-day single-tenant aggregated query p95). A change to a
  per-call timeout value is an additive observation, not a breaking
  change to the trait shape.

Sources: phase-02 §"Versioning, Deprecation, And Non-Goals" /
"Versioning and deprecation", §"SDK Method Inputs And Outputs"
latency bullets; phase-01 §"Cross-Entity Invariants" major-version
bullet.

## Exclusions/Non-goals

### REST-only exclusions

The SDK trait does not expose the following operations; consumers
must use the Usage Collector REST API:

- Platform liveness and readiness probes (handled by the ToolKit host above the gear boundary — the collector does not expose gear-local health endpoints).
- Operational telemetry; instruments are pushed via OTLP from
  ToolKit's global `SdkMeterProvider` (no in-gear HTTP metrics
  endpoint is provided).
- The REST-side error wire envelope (RFC-9457 `Problem`); SDK errors
  are domain-classified and the REST handler performs the conversion.
- CORS, TLS termination, and output encoding; these are platform API
  gateway responsibilities, not SDK or gear responsibilities.

Note: per ADR 0012, metric catalog operations (registration, read,
list, delete) are NOT REST-only — they are exposed on both the SDK
trait (Methods 6–9) and the REST API and converge on the same
gateway-side `MetricCatalogService` over the plugin-owned
`metric_catalog`. The gateway owns PDP authorization, `gts_id`
kind-prefix validation (against the two reserved base type ids
per ADR 0012's 2026-06-02 amendment), and closed-shape
`metadata_fields` validation; the plugin owns durable storage and
the `usage_records.gts_id` `ON DELETE RESTRICT` foreign-key
enforcement. Prior drafts of this section listed those operations
as REST-only; that exclusion has been lifted.

Sources: phase-01 §"Exclusions"; phase-02 §"REST-Only Exclusions";
ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`).

### Plugin SPI exclusions

The following are Plugin SPI responsibilities and must not appear on
the SDK trait:

- Durable persistence of `usage_records` AND of `metric_catalog`
  rows (per ADR 0012 — the catalog lives on the plugin alongside
  `usage_records` so the `usage_records.gts_id` `ON DELETE RESTRICT`
  FK can be enforced natively in a single backend transaction).
- Cursor token generation and validation.
- Dedup-on-conflict enforcement on
  `(tenant_id, gts_id, idempotency_key)`.
- Atomic `Active -> Inactive` transition at the storage boundary.
- Aggregation pushdown (the plugin executes aggregation server-side;
  the SDK trait emits the query, the plugin produces the result).
- Plugin-side per-method timeouts and `flush` for graceful shutdown.
- Plugin-host bulkheading, pooling, and circuit-breakers against the
  active plugin.
- Plugin SPI logical-table schema versioning.

Sources: phase-02 §"Plugin SPI Boundary".

### Gear non-goals reaffirmed on the SDK trait

- A dedicated backfill capability (watermarks, late-data coordination,
  or a bulk-import method beyond the existing batched `record_usage`
  path) is an explicit non-goal in v1. Old event timestamps are
  accepted without wall-clock validation, so bulk historical import
  rides the normal batched-ingestion path with each record's true
  event timestamp; see the timestamp / late-arrival invariant in
  `domain-model.md` §2.1 for the consequences for raw-tail consumers.
- Individual record amendment is intentionally omitted; the only
  post-acceptance mutation is the one-way `Active -> Inactive` status
  transition.
- Rate limiting, watermarks for high-cardinality bursts, and
  low-watermark coordination for late-arriving usage are caller- and
  operator-tuned at the gateway and source-gear layers and are not
  surfaced on the SDK trait.
- Multi-region deployment is not a v1 capability of the gear.
- Gear-emitted audit events for operator-write paths are deferred;
  the v1 access trail is composed at the gateway and PDP decision
  points.
- Pricing, rating, billing, invoice generation, and quota decisions
  are out of scope.
- Gear-owned compliance scope is not claimed; concrete control
  mapping is platform-compliance-owned.
- The Usage Collector exposes REST, SDK, and Plugin SPI surfaces
  only; there is no end-user UI and no business-event publish or
  subscribe bus.
- Gear-side caching of PDP decisions is forbidden.
- At-rest encryption, key management, masking, disposal, backup,
  point-in-time recovery, disaster recovery, replication, tiering,
  retention windows, archival, compression, encoding, and
  partitioning as gear-owned mechanisms are out of scope and
  plugin-owned.
- Dead-letter queue, poison-message handling, and compensation-saga
  patterns are out of scope; ingestion is synchronous and fail-closed.

Sources: phase-02 §"Versioning, Deprecation, And Non-Goals" / "Non-goals".

## Traceability

### Trait identifier and consumer contract

- `cpt-cf-usage-collector-interface-sdk-client` — the public SDK trait
  interface identifier carried by `UsageCollectorClientV1`. Source:
  phase-01 §"SDK trait surface (DESIGN section 3.3)"; phase-02 §"Public
  SDK Surface".
- `cpt-cf-usage-collector-contract-downstream-usage-reader` — the
  consumer contract referenced by the SDK trait. Source: phase-01
  §"SDK trait surface (DESIGN section 3.3)"; phase-02 §"Traceability
  Anchors" / "SDK trait surface and identifiers".

### Capabilities exposed by the SDK trait

- Submit single usage record and submit batched usage records:
  `cpt-cf-usage-collector-fr-ingestion`,
  `cpt-cf-usage-collector-fr-idempotency`,
  `cpt-cf-usage-collector-fr-usage-compensation`,
  `cpt-cf-usage-collector-adr-usage-compensation`,
  `cpt-cf-usage-collector-seq-emit-usage`. Throughput and batch-and-
  report timing: `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`. Ingestion
  latency: `cpt-cf-usage-collector-nfr-ingestion-latency`.
  Compensation rides this surface (no dedicated `compensate` method)
  per the §"No dedicated `compensate` method" note above. Sources:
  phase-01 §"SDK trait surface"; phase-02 §"Traceability Anchors" /
  "Capabilities exposed by the SDK trait" and §"SDK Method Inputs And
  Outputs" / "Ingestion" latency bullet; Phase 6 handoff
  §"Compensation Flow Text".
- Aggregated query: `cpt-cf-usage-collector-fr-query-aggregation`,
  `cpt-cf-usage-collector-seq-query-aggregated`,
  `cpt-cf-usage-collector-nfr-query-latency`. Sources: phase-01 §"SDK
  trait surface"; phase-02 §"Traceability Anchors" / "Capabilities
  exposed by the SDK trait".
- Raw cursor-paginated query: `cpt-cf-usage-collector-fr-query-raw`,
  `cpt-cf-usage-collector-seq-query-raw`. Source: same as aggregated
  query above.
- Deactivate usage event:
  `cpt-cf-usage-collector-fr-event-deactivation`,
  `cpt-cf-usage-collector-seq-deactivate-event`,
  `cpt-cf-usage-collector-adr-monotonic-deactivation`,
  `cpt-cf-usage-collector-principle-monotonic-deactivation`,
  `cpt-cf-usage-collector-flow-event-deactivation-cascade`,
  `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`,
  `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`.
  Sources: phase-01 §"DeactivationStatus", §"Event deactivation";
  phase-02 §"Traceability Anchors"; Phase 5 handoff §"Finalized cascade
  algorithm text" and §"Cascade response shape".
- Metric catalog (register / read / list / delete — Methods 6–9):
  `cpt-cf-usage-collector-fr-metric-registration`,
  `cpt-cf-usage-collector-fr-metric-deletion`,
  `cpt-cf-usage-collector-fr-metric-existence-and-kind`,
  `cpt-cf-usage-collector-seq-register-metric`,
  `cpt-cf-usage-collector-seq-delete-metric`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012 — every metric is a GTS type carrying a closed
  `metadata_fields: Vec<String>` declared-key list with `String`-typed
  values; `kind ∈ {counter, gauge}` is derived from the `gts_id`
  prefix matching one of two reserved base type ids
  (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`)
  per the 2026-06-02 amendment and is NOT stored as a column or
  declared as a trait).
  Referential integrity on delete is enforced by the plugin's
  `usage_records.gts_id → metric_catalog.gts_id` `ON DELETE RESTRICT`
  FK and surfaces to the SDK as
  `MetricReferenced { gts_id, sample_ref_count }`; declared
  metadata is gateway-validated at L1 per ADR 0012 (closed shape,
  keyed by `gts_id`) with undeclared keys surfaced as
  `UnknownMetadataKey { gts_id, key }` and bad kind prefixes
  surfaced at registration as `InvalidKindPrefix { gts_id }`.

> **Removed in ADR 0012.** Prior drafts cited ADR-0007
> (gateway-local-from-config catalog), ADR-0009 (catalog-plugin
> referential integrity), and ADR-0010 (GTS-typed metric metadata
> with inheritance, indexability flag, and `abstract` flag). All
> three are superseded in full by ADR 0012, which unifies the
> catalog on the plugin-DB and removes inheritance, indexability
> flags, and the abstract flag from the trait surface. ADR 0012's
> 2026-06-02 amendment further removes the open-but-typed JSON
> Schema surface (replaced by closed `metadata_fields: Vec<String>`
> with all values typed as `String`) and the per-metric trait map
> carrying `kind` (replaced by kind-derivation from the `gts_id`
> prefix against two reserved base type ids).

Sources: phase-01 §"Metric lifecycle"; DESIGN §3.6 / §3.7;
`plugin-spi.md` §"Method 6" through §"Method 9"; ADR 0012.

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
  `cpt-cf-usage-collector-entity-security-context`,
  `cpt-cf-usage-collector-entity-aggregation-query`,
  `cpt-cf-usage-collector-entity-raw-query`,
  `cpt-cf-usage-collector-entity-aggregation-result`,
  `cpt-cf-usage-collector-entity-usage-record-filter-field`,
  `cpt-cf-usage-collector-entity-keyset`,
  `cpt-cf-usage-collector-entity-pdp-decision`,
  `cpt-cf-usage-collector-entity-pdp-constraint`. Source: phase-01
  §"Domain Entities", §"Query Models And Views", §"Authorization And
  Tenancy Facts"; `out/phase-01-domain-contracts.md` §2. Raw-page
  output is the canonical `toolkit_odata::Page<UsageRecord>`
  envelope; cursor lifecycle is realized by
  `toolkit_odata::CursorV1` plus `validate_cursor_against`, per
  `cpt-cf-usage-collector-principle-cursor-gateway-ownership`. The
  former gear-owned page and cursor-token entities defined in
  earlier drafts of `domain-model.md` are no longer carried on the
  SDK surface.

### Authorization, fail-closed, and attribution anchors

- `cpt-cf-usage-collector-contract-authz-resolver`,
  `cpt-cf-usage-collector-principle-fail-closed`,
  `cpt-cf-usage-collector-principle-pdp-centric-authorization`,
  `cpt-cf-usage-collector-adr-pdp-centric-authorization`,
  `cpt-cf-usage-collector-adr-caller-supplied-attribution`,
  `cpt-cf-usage-collector-adr-mandatory-idempotency`,
  `cpt-cf-usage-collector-constraint-pii-identity-layer`,
  `cpt-cf-usage-collector-constraint-no-business-logic`. Source:
  phase-02 §"Traceability Anchors" / "Security and authorization
  anchors"; phase-01 §"Authorization And Tenancy Facts".

### Plugin SPI and persistence anchors (exclusions)

- `cpt-cf-usage-collector-interface-plugin`,
  `cpt-cf-usage-collector-contract-storage-plugin`,
  `cpt-cf-usage-collector-component-plugin-host`,
  `cpt-cf-usage-collector-adr-pluggable-storage`,
  `cpt-cf-usage-collector-principle-pluggable-storage`,
  `cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference`
  (ADR 0012 — unifies the metric catalog on the plugin-DB and sets
  `gts_id` as the catalog PK / FK on `usage_records`; the gateway
  L1 catalog cache is keyed by `gts_id` and stores the closed
  `metadata_fields` declared-key list with `kind` derived per
  lookup from `gts_id`'s prefix — plugins do NOT re-implement
  metadata validation),
  `cpt-cf-usage-collector-dbtable-metric-catalog`,
  `cpt-cf-usage-collector-dbtable-usage-records`. Source: phase-02
  §"Traceability Anchors" / "Plugin SPI / persistence anchors";
  ADR 0012.

### Versioning, stability, and quality NFR anchors

- `cpt-cf-usage-collector-adr-contract-stability`,
  `cpt-cf-usage-collector-principle-contract-stability`,
  `cpt-cf-usage-collector-nfr-plugin-contract-stability`,
  `cpt-cf-usage-collector-nfr-ingestion-latency`,
  `cpt-cf-usage-collector-nfr-query-latency`,
  `cpt-cf-usage-collector-nfr-throughput`,
  `cpt-cf-usage-collector-nfr-throughput-profile`,
  `cpt-cf-usage-collector-nfr-batch-and-report-timing`,
  `cpt-cf-usage-collector-nfr-workload-isolation`,
  `cpt-cf-usage-collector-nfr-graceful-degradation`,
  `cpt-cf-usage-collector-nfr-error-experience`,
  `cpt-cf-usage-collector-nfr-developer-operator-experience`,
  `cpt-cf-usage-collector-nfr-documentation-coverage`. Source:
  phase-02 §"Traceability Anchors" / "Versioning, stability, and
  contracts" and / "NFRs that shape the SDK surface".

### Components allocated to the SDK trait

- `cpt-cf-usage-collector-component-ingestion-gateway`,
  `cpt-cf-usage-collector-component-query-gateway`,
  `cpt-cf-usage-collector-component-deactivation-handler`,
  `cpt-cf-usage-collector-component-metric-catalog`. Each component
  performs PDP enforcement inline via the shared `authz_scope` helper
  inside the trait implementation. Source: phase-01 §"Component
  allocation relevant to SDK trait (DESIGN section 3.2)".

## Open Questions

These are residual choices that the SDK crate may finalize during
implementation. None block this reference; each notes the conservative
default this reference adopts.

- OQ-1 (phase-02 OQ-1, gap G-9): Whether the SDK trait surfaces
  `PdpConstraint` values to callers on query responses so callers can
  see which scope was applied. This reference keeps `PdpDecision` and
  `PdpConstraint` internal to the gear and does not surface them on
  the trait; the SDK trait conveys only the post-authorization outcome
  through `Ok` results or the `Authorization` error variant. The SDK
  crate may add a non-required diagnostic field on query result types
  in a future minor version without breaking compatibility.
- OQ-2 (phase-02 OQ-2, gap G-10): Whether `IdempotencyKey` is a
  domain newtype in `models.rs` or a plain string accepted at the
  trait boundary. This reference treats `IdempotencyKey` as a domain
  newtype in `models.rs`, consistent with the ToolKit `models.rs`
  template and the invariant that idempotency keys are required and
  opaque.
- OQ-3 (phase-02 OQ-3, gap G-4): Whether `UsageRecordAck` carries the
  plugin-minted `id` alongside the dedup indicator. This reference
  adopts both: the plugin-minted `id` and a `DedupOutcome` indicator
  are part of `UsageRecordAck`. Future minor versions may extend the
  ack shape with non-required fields.
- OQ-4 (gaps G-7 and G-11): Whether the SDK trait enforces numeric
  caps for raw `page_size`, aggregation result row count, group-by
  dimension count, and time-range window length at the trait
  boundary, and the inclusivity semantics of `time_range`. This
  reference defers numeric caps to the PRD NFR and the REST/OpenAPI
  contract; the SDK trait validates structural shape only (positive
  `page_size`, bounded `time_range`). The SDK rustdoc documents the
  conservative inclusive-start, exclusive-end convention for
  `time_range`, aligned with the REST wire semantics, without making
  it a breaking change surface.
- OQ-5 (gap G-8): Whether `correlation_id` on `SecurityContext` is
  caller-supplied or runtime-provided on SDK calls. This reference
  treats `correlation_id` as a field of the resolved `SecurityContext`
  provided by the ToolKit gateway upstream of the collector (REST) or
  by the in-process caller (SDK) and propagated by the caller; the
  SDK trait itself does not synthesize correlation IDs.
- OQ-6 (gap G-13): How the SDK trait surfaces its per-call timeout
  declaration. This reference adopts trait-level constant timeout
  values documented in the SDK rustdoc (one per method, bounded by
  the per-operation latency budgets). The SDK crate may evolve this
  to a configuration value in a future minor version without breaking
  the trait shape.

Sources: phase-02 §"Open Questions (annotated)", §"Gaps For Phase 3",
§"ToolKit Conventions" / "Trait shape" V1-suffix bullet, §"SDK Method
Inputs And Outputs" latency bullets; phase-01 §"Missing Or Uncertain
Facts".
