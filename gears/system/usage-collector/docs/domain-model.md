# Usage Collector - Domain Model

<!-- toc -->

- [1. Modeling Conventions](#1-modeling-conventions)
- [2. Core Entities](#2-core-entities)
  - [2.1 UsageRecord](#21-usagerecord)
  - [2.2 EntryType](#22-entrytype)
  - [2.3 ResourceRef](#23-resourceref)
  - [2.4 SubjectRef](#24-subjectref)
  - [2.5 Metric](#25-metric)
  - [2.6 MetricKind](#26-metrickind)
  - [2.7 IdempotencyKey](#27-idempotencykey)
  - [2.8 RecordMetadata](#28-recordmetadata)
  - [2.9 SecurityContext](#29-securitycontext)
  - [2.10 DeactivationStatus](#210-deactivationstatus)
  - [2.11 UsageRecordFilterField](#211-usagerecordfilterfield)
  - [2.12 Keyset](#212-keyset)
- [3. Query Domain](#3-query-domain)
  - [3.1 AggregationQuery](#31-aggregationquery)
  - [3.2 RawQuery](#32-rawquery)
  - [3.3 AggregationResult](#33-aggregationresult)
- [4. Authorization Domain](#4-authorization-domain)
  - [4.1 PdpDecision](#41-pdpdecision)
  - [4.2 PdpConstraint](#42-pdpconstraint)
- [5. Plugin Binding Domain](#5-plugin-binding-domain)
  - [5.1 PluginBinding](#51-pluginbinding)
- [6. Surface Mapping](#6-surface-mapping)
  - [6.1 Error Envelope](#61-error-envelope)
- [7. Cross-Entity Invariants](#7-cross-entity-invariants)

<!-- /toc -->

This companion document defines the field-level domain model referenced by
`DESIGN.md` section 3.1. It is the shared data dictionary for the Usage
Collector core, SDK trait, REST wire contract, and Plugin SPI. The dedicated
`sdk-trait.md`, `plugin-spi.md`, and `usage-collector-v1.yaml` artifacts
specify each surface's operation set, types, and wire schemas on top of the
domain semantics captured here; this document remains the single source of
truth for entity field semantics shared across those surfaces.

This document is not executable DDL, an ORM mapping, or a complete OpenAPI
schema. Physical storage layout, backend-specific indexes, retention, and
query acceleration remain plugin-owned. REST endpoint paths and wire envelope
details remain owned by the OpenAPI contract `usage-collector-v1.yaml` (sibling to DESIGN.md).

## 1. Modeling Conventions

Field names use the canonical snake_case names from the logical tables in
`DESIGN.md` section 3.7. Rust implementations may wrap these fields in newtype
or enum types, but must preserve the domain semantics documented here.

Identifiers such as `tenant_id`, `resource_id`, `subject_id`, `source_gear`,
and `gts_id` are opaque platform identifiers. The Usage Collector stores and
compares them but does not parse, classify, or derive identity information from
them.

All timestamps are UTC instants. The REST representation is RFC 3339 / ISO 8601
UTC text; Rust and plugin implementations may use native timestamp types at
their own boundaries.

Numeric usage values are measurement values, not money. Pricing, rating,
billing, invoice generation, and quota decisions are downstream concerns and
must not be added to these domain types.

Optional fields are omitted when absent. `SubjectRef` is present when
`subject_id` is present; `subject_type` is an optional qualifier because some
source systems do not maintain subject-type taxonomies.

## 2. Core Entities

### 2.1 UsageRecord

Traceability: `cpt-cf-usage-collector-entity-usage-record`

A `UsageRecord` is one accepted measurement of resource consumption attributed
to a tenant, resource, optional subject, source gear, and Metric (resolved
against the metric catalog, managed via the Plugin SPI and persisted in the
active storage plugin's database; see ADR 0012 at
`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`).
It is immutable after acceptance except for the one-way `status` transition
from `active` to `inactive`.

| Field             | Required             | Type                     | Description                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ----------------- | -------------------- | ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`              | Accepted record only | Opaque record identifier | Per-record identifier minted by the active plugin when the record is accepted. Client ingestion requests do not supply it.                                                                                                                                                                                                                                                                                                       |
| `tenant_id`       | Yes                  | Opaque tenant identifier | Caller-supplied tenant attribution. PDP authorization decides whether the caller may emit or read this tenant scope.                                                                                                                                                                                                                                                                                                             |
| `resource`        | Yes                  | `ResourceRef`            | Caller-supplied resource attribution. Both `resource_id` and `resource_type` are mandatory.                                                                                                                                                                                                                                                                                                                                      |
| `subject`         | No                   | `SubjectRef`             | Optional caller-supplied subject attribution. When present, `subject_id` is mandatory and `subject_type` is optional.                                                                                                                                                                                                                                                                                                            |
| `source_gear`   | Yes                  | Opaque gear identifier | Identity of the emitting source gear used in PDP emit authorization and downstream diagnostics.                                                                                                                                                                                                                                                                                                                                |
| `metric_gts_id`   | Yes                  | GTS identifier           | Reference to a `Metric.gts_id` present in the metric catalog (managed via the Plugin SPI, persisted in the active storage plugin's database). The same `gts_id` string that identifies the metric in the catalog is the value stored on every usage record that references it — no UUID derivation. Unknown Metrics are rejected before persistence. See ADR 0012 (`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`). |
| `value`           | Yes                  | Numeric                  | Measurement value. Permitted sign depends jointly on `MetricKind` and `entry_type` per the four-cell validation matrix in this section.                                                                                                                                                                                                                                                                                          |
| `entry_type`      | Yes                  | `EntryType`              | Discriminator separating ordinary usage (`usage`) from counter value-reversal (`compensation`). Default `usage`. See §2.2 `EntryType` and the four-cell validation matrix below.                                                                                                                                                                                                                                                 |
| `corrects_id`     | Conditional          | Opaque record identifier | Required when `entry_type = compensation`; references the `UsageRecord.id` of the `usage` row being corrected. MUST be omitted when `entry_type = usage`.                                                                                                                                                                                                                                                                        |
| `timestamp`       | Yes                  | UTC timestamp            | Event timestamp supplied by the usage source (event time, not arrival time). Accepted without wall-clock validation; see the timestamp / late-arrival invariant under §2.1 Invariants.                                                                                                                                                                                                                                           |
| `idempotency_key` | Yes                  | `IdempotencyKey`         | Caller-supplied key used to deduplicate retries within `(tenant_id, metric_gts_id)`. A same-key collision is resolved by exact-equality of the caller-supplied canonical fields: an exact-equality retry is silently deduplicated, while any differing canonical field is a Conflict (rejected, not absorbed).                                                                                                                   |
| `status`          | Yes                  | `DeactivationStatus`     | `active` on acceptance; may transition once to `inactive`. No reactivation exists.                                                                                                                                                                                                                                                                                                                                               |
| `metadata`        | No                   | `RecordMetadata`         | Optional opaque JSON object persisted and returned verbatim.                                                                                                                                                                                                                                                                                                                                                                     |

The permitted sign of `value` is jointly governed by `MetricKind` and
`entry_type` per the following four-cell validation matrix, enforced before
persistence:

| MetricKind | entry_type     | Allowed `value`                 |
| ---------- | -------------- | ------------------------------- |
| `counter`  | `usage`        | `value >= 0` (unchanged)        |
| `counter`  | `compensation` | `value < 0` (strictly negative) |
| `gauge`    | `usage`        | Any signed value (unchanged)    |
| `gauge`    | `compensation` | Rejected before persistence     |

Invariants:

- `tenant_id`, `resource`, `source_gear`, `metric_gts_id`, `value`,
  `timestamp`, `idempotency_key`, and `status` are never null on an accepted
  record.
- `timestamp` is event time, not ingestion time, and is not validated against
  wall-clock: any UTC instant (past or future) is accepted, so late-arriving
  and historical records are ingested at their event-time position in the
  `(timestamp, id)` sort order. Aggregation over a bounded `time_range`
  re-scans and stays complete regardless of arrival order; but a consumer
  tailing raw records by a forward `(timestamp, id)` cursor may not observe a
  record that lands behind a position it already passed — incremental raw
  tailing is best-effort, not a lossless change feed. There is no dedicated
  backfill capability (no watermarks or late-data coordination); bulk
  historical import uses the normal batch ingestion path with each record's
  true event timestamp.
- `metric_gts_id` must resolve to a Metric in the metric catalog (managed via
  the Plugin SPI, persisted in the active storage plugin's database) before
  the record reaches the plugin; the record's `metric_gts_id` value is the
  same `gts_id` string used as the catalog primary key (no UUID derivation;
  see ADR 0012).
- Deduplication is unique on `(tenant_id, metric_gts_id, idempotency_key)`.
  `source_gear` is intentionally not part of that key; multiple source
  gears authorized for the same tenant and Metric must coordinate key
  allocation. On a collision the outcome is decided by exact equality of the
  caller-supplied canonical fields (`value`, `timestamp`, `resource`,
  `subject`, `source_gear`, `metadata`; the match-key tuple and the
  server-owned `id` and `status` are excluded): an exact-equality retry is
  silently deduplicated, and any differing canonical field — including a
  metadata-only difference — is a Conflict that is rejected, not absorbed.
- The idempotency window is unbounded: the key never expires, has no TTL, and
  is never intentionally reusable, so the `(tenant_id, metric_gts_id,
idempotency_key)` uniqueness is permanent. The active plugin must preserve
  that key tuple permanently even when record bodies are purged or archived by
  retention — a retention purge must not free a dedup key.
- Accepted records are immutable except for the `status` transition performed
  by the deactivation path.
- Deactivation does not mutate `tenant_id`, `resource`, `subject`,
  `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, or
  `metadata`.
- `entry_type` defaults to `usage` on every ingestion and is never mutated
  after acceptance.
- When `entry_type = compensation`, `corrects_id` MUST be supplied and MUST
  reference an existing `UsageRecord` whose `entry_type = usage`, whose
  `(tenant_id, metric_gts_id)` matches this record's `(tenant_id,
metric_gts_id)`, and whose `status = active`. These constraints are
  enforced at ingestion (L1); no per-record remaining-amount tracking is
  introduced.
- A compensation referencing a row that is concurrently deactivating is
  rejected by the L1 "referenced record must be active" check.
- `corrects_id` MUST NOT be supplied when `entry_type = usage`.
- The four-cell validation matrix governs the permitted sign of `value`:
  `counter+usage` requires `value >= 0`; `counter+compensation` requires
  `value < 0`; `gauge+usage` accepts any signed value; `gauge+compensation`
  is rejected before persistence (gauges natively express down-movement, so
  a separate compensation primitive is meaningless and disallowed).

### 2.2 EntryType

Traceability: `cpt-cf-usage-collector-entity-entry-type`

`EntryType` discriminates ordinary usage from counter value-reversal on a
`UsageRecord`.

| Value          | Semantics                                                                                                                                                                                       |
| -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `usage`        | Default. The record represents an ordinary, source-emitted usage measurement. Allowed on both `counter` and `gauge` Metrics.                                                                    |
| `compensation` | The record is an append-only, strictly-negative counter value-reversal that reduces the running `SUM` for `(tenant_id, metric_gts_id)`. Allowed only on `counter` Metrics; rejected on `gauge`. |

Invariants:

- `compensation` is counter-only: a `gauge` Metric paired with
  `entry_type = compensation` is rejected before persistence. Gauges
  natively express down-movement by emitting a smaller point-in-time
  reading, so a separate compensation primitive would be redundant.
- A `compensation` record MUST carry `value < 0` (strictly negative). A zero
  or positive `value` paired with `entry_type = compensation` is rejected.
- A `compensation` record MUST carry `corrects_id` referencing a `usage`
  record that shares `(tenant_id, metric_gts_id)` and is currently active.
- Compensation is append-only: a compensation row is never rewritten and is
  never itself compensated. The only correction available against a
  compensation is whole-row deactivation via the same one-way
  `active -> inactive` transition that applies to any `entry_type`.

### 2.3 ResourceRef

Traceability: `cpt-cf-usage-collector-entity-resource-ref`

`ResourceRef` identifies the resource instance to which usage is attributed.
The composite is mandatory on every usage record and can be used to narrow
authorized read queries.

| Field           | Required | Type                       | Description                                                                             |
| --------------- | -------- | -------------------------- | --------------------------------------------------------------------------------------- |
| `resource_id`   | Yes      | Opaque resource identifier | Resource instance identifier inside the attributed tenant scope.                        |
| `resource_type` | Yes      | Opaque resource type       | Type discriminator such as `compute.vm` or another platform-owned resource type string. |
|                 |          |                            |                                                                                         |

Invariants:

- `resource_id` and `resource_type` must be supplied together.
- The Usage Collector validates only presence and structural shape; ownership
  and caller permission are PDP decisions.

### 2.4 SubjectRef

Traceability: `cpt-cf-usage-collector-entity-subject-ref`

`SubjectRef` optionally identifies the user, service account, or other principal
to which usage is attributed. It is caller-supplied and never derived from the
caller `SecurityContext`.

| Field          | Required    | Type                      | Description                                                                                                            |
| -------------- | ----------- | ------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `subject_id`   | Conditional | Opaque subject identifier | Internal platform identifier issued and governed by the identity layer. Required when subject attribution is supplied. |
| `subject_type` | No          | Opaque subject type       | Optional type discriminator for systems that maintain subject-type taxonomies.                                         |

Invariants:

- `subject_id` defines whether subject attribution is present.
- `subject_type` may be omitted when the source system has no meaningful subject
  type.
- `subject_type` must not be supplied without `subject_id`.
- When a subject is present, PDP authorization includes `subject_id` and includes
  `subject_type` only when supplied.
- When no subject is present, subject authorization is skipped; the system must
  not infer subject identity from the authenticated caller.
- Subject identifiers are opaque and are not PII within the Usage Collector
  boundary.

### 2.5 Metric

Traceability: `cpt-cf-usage-collector-entity-metric`

`Metric` is a platform-global definition of something the collector measures.
Metrics are not tenant-scoped and are managed by platform operators via the
SDK trait method `UsageCollectorClient::register_metric_type` or the REST
endpoint `POST /usage-collector/v1/metric-types` — both ingress paths converge
on a single metric catalog, managed via the Plugin SPI and persisted in the
active storage plugin's database. See ADR 0012
(`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`).

Each Metric is a **GTS Type Schema** — its identifier is a GTS _type_ id whose
last segment ends `~` per the GTS naming convention — not a GTS instance id.
Metric types are flat for v1: there is no parent pointer on the catalog row
and no inheritance chain walked at validation time. Every registered metric
type is concrete and may receive usage rows on its declared shape. Metric
type definitions are owned by usage-collector (semantic ownership) and
physically stored on the active storage plugin's backend database alongside
`usage_records`, per ADR 0012, which supersedes the prior gateway-local
catalog (ADR-0007), the dual-catalog referential-integrity ADR (ADR-0009),
and the inheritance-based metric-metadata model (ADR-0010). A gateway-side
Level-1 (L1) read-through cache holds a flat `Map<gts_id, {kind,
metadata_fields}>` keyed by `gts_id` for ingest-time validation; it is
refreshed synchronously on register and delete (both flow through the
gateway).

| Field             | Required | Type            | Description                                                                                                                                                                                                                                                                                |
| ----------------- | -------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `gts_id`          | Yes      | `MetricGtsId`   | Deployment-unique Metric **type** identifier; the last segment MUST end `~` (GTS type id) and the identifier MUST begin with one of the two reserved kind base type ids `gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`. Catalog primary key; FK target on usage records. |
| `metadata_fields` | Yes      | array\<string\> | set of allowed metadata keys; closed shape — record metadata keys MUST be a subset, all values typed as String                                                                                                                                                                             |

The per-metric declared-key set is owned by the metric type's own
`metadata_fields` — there is no inheritance, no ancestor walk, and no
inheritable trait. The gateway extracts the keys named in
`metadata_fields` per record and hands them to the active storage plugin
for backend-side `group_by` and `$filter` (see §2.11 and §3). MetricKind is
derived from the `gts_id` prefix matching one of two well-known base type
ids (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`); it
is not stored as a column or trait.

The usage-record reference to a metric is the `gts_id` string itself. The
catalog primary key on the `metric_catalog` table is `gts_id`, and the FK
column on `usage_records` is `gts_id` under `ON DELETE RESTRICT`. No UUID is
derived from the type id; consumers and plugin authors join on the textual
`gts_id` directly.

`kind` ∈ {`counter`, `gauge`} is **derived** from the `gts_id` prefix per
§2.6 `MetricKind`. It is not stored on the catalog row and is not declared
as a trait; every registered metric's `gts_id` MUST begin with exactly one
of the two reserved kind base type ids.

`unit` is a **deferred open item**. Whether `unit` becomes a declared
key in `metadata_fields` (with a domain-conventional name) or is
introduced via a separate dedicated field on the catalog row is
intentionally left open; both options remain on the table and the choice
does not block the metric-type / dimensions decision. The Metric entity does
NOT yet declare a `unit` field.

`MetricGtsId` is a newtype wrapping the platform-primitive `gts::GtsID`
(re-exported by `libs/toolkit-gts`). Its `Deserialize` impl parses the input
string as a GTS type id (trailing `~`) and asserts the parsed value begins
with one of the two reserved kind base type id constants
`COUNTER_BASE_TYPE_ID = "gts.cf.core.usage.counter.v1~"` or
`GAUGE_BASE_TYPE_ID = "gts.cf.core.usage.gauge.v1~"` exposed by the
usage-collector SDK / contracts crate. The newtype is the validation point on
REST `Json<Metric>` deserialization at
`POST /usage-collector/v1/metric-types`.

Invariants:

- `gts_id` is unique across the deployment, is a GTS _type_ id (ends `~`), and
  MUST begin with one of the two reserved kind base type id prefixes
  (`gts.cf.core.usage.counter.v1~` or `gts.cf.core.usage.gauge.v1~`).
  Identifiers that do not begin with one of those two prefixes (or are
  non-type identifiers) are rejected at the `MetricGtsId::deserialize`
  boundary and surface on the REST path as a structured `422` `Problem`
  envelope (`context.reason="invalid_gts_id"`).
- Metric types are flat for v1: there is no parent pointer, no ancestor chain
  validation, and no implicit ancestor materialization. If inheritance is
  required by a future capability, it will be reintroduced by a dedicated ADR
  that names its consumer (see ADR 0012).
- The catalog primary key is `gts_id`. The FK column on `usage_records` is
  `gts_id` under `ON DELETE RESTRICT`. No UUID is derived from the type id.
- Every registered metric type is concrete and may receive usage rows on its
  declared shape; there is no abstract / non-abstract distinction on the
  catalog row.
- `metadata_fields` declares a closed list of allowed metadata keys per
  metric. Only declared keys are accepted at ingest; every value is typed as
  `String` end-to-end (see §2.8). There is no free-form remainder and no
  per-key JSON-Schema surface.
- Per-metric declared keys are owned by the metric's own `metadata_fields`;
  they are not inheritable and indexing strategy is a plugin implementation
  concern. Every key in `metadata_fields` is queryable (declared =
  queryable); there is no separate indexable-trait gate on the metric
  specification surface per ADR 0012 (see §2.11).
- Metric registration is PDP-authorized and rejects duplicate identifiers
  (`metric_already_exists`).
- Metric deletion is PDP-authorized and is rejected by the storage plugin
  when the catalog row is still referenced by any `usage_records` row, via
  the `ON DELETE RESTRICT` FK established on `gts_id`. The plugin returns a
  structured `metric_referenced` error that the gateway surfaces as a
  deterministic REST / SDK error response.
- The collector does not store source-gear-to-Metric authorization
  mappings; those policies belong to the PDP.

### 2.6 MetricKind

Traceability: `cpt-cf-usage-collector-entity-metric-kind`

`MetricKind` is a **derived attribute**, not a stored column or trait. It is
computed by matching the metric's `gts_id` prefix against the two reserved
kind base type ids:

- `counter` ⇐ `gts_id` begins with `gts.cf.core.usage.counter.v1~`
- `gauge` ⇐ `gts_id` begins with `gts.cf.core.usage.gauge.v1~`

Registration MUST reject a `gts_id` that does not start with one of these
two prefixes. `MetricKind` controls ingestion-time validation and plugin
accumulation semantics derived from that prefix.

| Value     | Semantics                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `counter` | Source gears submit non-negative gross usage deltas as `entry_type = usage` (`value >= 0`). The cumulative total per `(tenant_id, metric_gts_id)` is the signed `SUM` over both `usage` and `compensation` entries: append-only compensation rows (`entry_type = compensation`, `value < 0`) reduce that total. `SUM` is therefore not monotonically increasing in the presence of compensation, and the plugin MUST NOT impose monotonicity checks across entry types. |
| `gauge`   | Source gears submit point-in-time readings as `entry_type = usage` that may rise or fall. Values are stored as-is without monotonicity checks or delta accumulation. `gauge` Metrics do not admit `entry_type = compensation`: a gauge already expresses down-movement directly, and a compensation row on a gauge Metric is rejected before persistence.                                                                                                               |

Invariants:

- `MetricKind` is derived from the `gts_id` prefix; it is not stored as a
  column or trait on the catalog row. Registration MUST reject any `gts_id`
  whose prefix is not one of the two reserved kind base type ids
  (`gts.cf.core.usage.counter.v1~`, `gts.cf.core.usage.gauge.v1~`).
- Compensation is counter-only. `gauge + compensation` is rejected before
  persistence.
- The previous "monotonically increasing totals per `(tenant_id,
metric_gts_id)`" framing on `counter` is superseded by this section:
  cumulative totals MAY be reduced by compensation entries.

### 2.7 IdempotencyKey

Traceability: `cpt-cf-usage-collector-entity-idempotency-key`

`IdempotencyKey` is a caller-supplied opaque string required on every usage
record.

Invariants:

- Keyless records are rejected before persistence.
- A same-key submission with the same `(tenant_id, metric_gts_id,
idempotency_key)` is resolved by exact equality of the caller-supplied
  canonical fields (`value`, `timestamp`, `resource`, `subject`,
  `source_gear`, `metadata`; the match-key tuple and the server-owned `id`
  and `status` are excluded). An exact-equality retry is silently
  deduplicated; any differing canonical field — including a metadata-only
  difference — is a Conflict that is rejected fail-closed (surfaced on the wire
  as the `idempotency_conflict` reason), never silently dropped.
- The idempotency window is unbounded: the key never expires, has no TTL, and
  is never intentionally reusable. The active plugin must preserve the
  `(tenant_id, metric_gts_id, idempotency_key)` tuple permanently even when
  record bodies are purged or archived by retention; a retention purge must not
  free a dedup key.
- The same key may legitimately appear under a different tenant or Metric.
- Source gears sharing the same tenant and Metric namespace must coordinate
  key prefixes or another allocation convention.

### 2.8 RecordMetadata

Traceability: `cpt-cf-usage-collector-entity-record-metadata`

`RecordMetadata` is optional per-record context supplied by the usage
source. It is validated against the referenced metric type's
`metadata_fields` (see ADR 0012 at
`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`) on a **closed
shape**: every key MUST appear in the metric's declared `metadata_fields`,
every value is typed as String (or string-coercible per the SDK / wire
spec), and undeclared keys are rejected.

The gateway holds a flat L1 read-through cache entry of `{kind,
metadata_fields}` keyed by `gts_id` against the plugin-side metric catalog.
Register and delete operations on a `gts_id` invalidate that metric's cache
entry synchronously; there is no cascade invalidation, because metric types
are flat (no parent / descendant relationship). Plugins do not own this
membership check; they store the raw `metadata_fields` array only.

| Property             | Value                                                                                                                                                                                                                                                |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shape                | Key/value map; every key is a String drawn from the metric's `metadata_fields`; every value is a String (or string-coercible per spec).                                                                                                              |
| Default maximum size | 8 KiB per record unless operator configuration overrides it                                                                                                                                                                                          |
| Validation           | **Closed shape**: every key MUST be a member of the metric's `metadata_fields`; every value MUST be a String (or string-coercible per spec); any key not in `metadata_fields` is rejected before persistence with error name `unknown_metadata_key`. |
| Queryable keys       | Every key in the metric's `metadata_fields` is queryable (see §2.11 and §3). The gateway extracts these per record for backend-side `group_by` and `$filter`. There is no separate indexable-trait gate; declared = queryable.                       |
| Interpretation       | Not interpreted, aggregated, classified, or transformed beyond closed-shape membership validation and declared-key extraction. Downstream consumers own any further interpretation.                                                                  |
| Query behavior       | Persisted and returned verbatim with raw records. Only declared keys exist on a record; there are no preserved "extras".                                                                                                                             |

Invariants:

- Validation failures against the metric's `metadata_fields` are rejected
  before persistence with an actionable validation error. An undeclared
  metadata key is rejected with error name `unknown_metadata_key`. A
  non-string-coercible value on a declared key is likewise rejected.
- Oversized metadata is rejected with an actionable validation error.
- Undeclared keys are never silently accepted, never preserved as extras, and
  never reach the storage plugin — the surface is closed.
- Declared keys extracted from `RecordMetadata` are the metadata values the
  plugin uses for query acceleration; any subsequent change to
  `metadata_fields` on a metric type is reconciled via the gateway L1 cache
  invalidation path (per-metric, keyed by `gts_id`). Indexing strategy on
  those keys is a plugin implementation concern (see ADR 0012 at
  `./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`).
- Metadata must not be used to add pricing, billing, quota, or authorization
  logic inside the collector.

### 2.9 SecurityContext

Traceability: `cpt-cf-usage-collector-entity-security-context`

`SecurityContext` is the platform-authenticated caller context. The Usage
Collector receives it from the ToolKit gateway (REST) or directly from the
caller (in-process SDK trait) and consumes it for per-operation PDP
authorization and correlation propagation, but does not own its schema or
persist it on usage records.

| Field                 | Required               | Type                       | Description                                                                                                             |
| --------------------- | ---------------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `principal`           | Yes                    | Opaque principal reference | Authenticated caller identity as provided by the platform identity layer.                                               |
| `tenant_scope_claims` | Platform-owned         | Opaque claims              | Tenant-scope claims available to PDP evaluation. The collector does not infer authorization from these claims directly. |
| `auxiliary_claims`    | Platform-owned         | Opaque claims              | Additional platform claims passed through to PDP evaluation when available.                                             |
| `correlation_id`      | Yes for API operations | Opaque request identifier  | Identifier propagated through gateway, PDP decision logs, gear logs, and platform audit trail.                        |

Invariants:

- Requests without a resolved `SecurityContext` fail closed.
- The collector never synthesizes identities and never falls back to anonymous
  access.
- `SecurityContext` is input to authorization, not a source for implicit tenant,
  resource, or subject attribution.

### 2.10 DeactivationStatus

Traceability: `cpt-cf-usage-collector-entity-deactivation-status`

`DeactivationStatus` records the lifecycle state of an accepted
`UsageRecord` regardless of its `entry_type`.

| Value      | Meaning                                                                                                                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `active`   | Default state for newly accepted records (both `entry_type = usage` and `entry_type = compensation`).                                                                                                 |
| `inactive` | Record was deactivated by an authorized operator. The record remains queryable and distinguishable from active records and is excluded from authoritative aggregation and event-style query surfaces. |

Invariants:

- The only transition is `active -> inactive`. Deactivation is one-way and
  is atomic at the plugin boundary.
- Deactivation applies to any `entry_type`. A `usage` row and a
  `compensation` row are individually deactivatable.
- **Depth-1 cascade**: deactivating an active `usage` row atomically flips
  every active `compensation` row whose `corrects_id` points to that
  `usage` row from `active` to `inactive` as part of the same plugin-side
  operation. Deactivating a `compensation` row never cascades — there is
  no row that references a compensation.
- A second deactivation request for an already-inactive record is rejected
  with an actionable error, regardless of `entry_type`.

### 2.11 UsageRecordFilterField

Traceability: `cpt-cf-usage-collector-entity-usage-record-filter-field`

`UsageRecordFilterField` is the domain-level description of every wire field
that may appear in an OData `$filter` expression on the raw-query surface (and
in `group_by` on the aggregation surface — see §3). It is **not** a fixed
static enum: it is a **derived shape** composed of a fixed-field core plus
every key in the queried metric type's `metadata_fields`, resolved per
request from the metric's own catalog row (see §2.5 and §2.8) in the gateway
L1 cache. See ADR 0012
(`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`) for the metric
catalog and reference-model decisions.

Field names on the wire match the canonical `UsageRecord` field names in §2.1
exactly; declared-key names are taken verbatim from the metric's
`metadata_fields`. No alternative shorthand spellings are accepted.

**Fixed fields (always available, all metrics):**

| Wire field name | Source              | Allowed OData operators            |
| --------------- | ------------------- | ---------------------------------- |
| `tenant`        | `tenant_id` on §2.1 | `eq`, `in`                         |
| `resource`      | `ResourceRef` §2.3  | `eq`, `in`                         |
| `subject`       | `SubjectRef` §2.4   | `eq`, `in`                         |
| `source_gear` | §2.1                | `eq`, `in`                         |
| `timestamp`     | §2.1                | `eq`, `ne`, `lt`, `le`, `gt`, `ge` |
| `status`        | §2.10               | `eq`, `ne`, `lt`, `le`, `gt`, `ge` |

The fixed-field core is identical across every metric type and survives any
metric-catalog change.

**Per-metric declared keys (resolved per request):**

The set of additional fields available in `$filter` (and `group_by`) is
**every key in the queried metric type's `metadata_fields`** (see §2.5 and
§2.8). The set is resolved per request from the `metric_gts_id` on the
query (REQUIRED on `RawQuery` and `AggregationQuery`, see §3): the gateway
reads the metric's `metadata_fields` from its L1 cache (keyed by `gts_id`)
and admits exactly those key names as filter fields for that request.
There is no separate indexable-trait gate — declared = queryable.

Because declared-key values are typed as String (per §2.8 closed shape),
the natural operator set for per-metric declared keys is `eq` / `in`,
matching the other opaque-identifier fixed fields above. Wider operator
sets (range, ordering) on a declared key MAY be supported in a future
revision if a specific value family warrants them; v1 admits `eq` / `in`
only.

Invariants:

- The fixed-field opaque identifiers (`tenant`, `resource`, `subject`,
  `source_gear`) accept only `eq` and `in`; ordering and range operators
  are rejected as a structural validation error.
- `timestamp` and `status` accept the full comparison operator set
  (`eq`, `ne`, `lt`, `le`, `gt`, `ge`).
- Per-metric declared keys accept `eq` and `in` only in v1; any other
  operator on a declared-key field is rejected as a structural validation
  error before plugin dispatch.
- The set of admissible filter fields is computed per request from the
  metric's own `metadata_fields` for the metric named in the query's
  `metric_gts_id`. Requests using any field outside the union of (fixed
  fields, that metric's declared keys) are rejected before plugin dispatch
  with an actionable error naming the offending field and the metric.
- Every key in `metadata_fields` is filterable; there is no separate
  indexable-trait gate. Conversely, any name not in `metadata_fields` is
  rejected before plugin dispatch — there are no undeclared "extras"
  reaching the record per §2.8 closed-shape semantics.
- The derived shape is recomputed on every request from the L1 cache; a
  metric-catalog register/delete invalidates the affected `gts_id`'s cache
  entry synchronously (per ADR 0012), so a freshly-registered declared key
  is filterable on the very next request to that metric.

### 2.12 Keyset

Traceability: `cpt-cf-usage-collector-entity-keyset`

`Keyset` is the typed last-row sort-key tuple consumed by the toolkit cursor
encoder when paginating raw queries.

| Component   | Type                     | Description                                                                             |
| ----------- | ------------------------ | --------------------------------------------------------------------------------------- |
| `timestamp` | UTC timestamp            | Primary sort key; matches `UsageRecord.timestamp` of the last row on the emitted page.  |
| `id`        | Opaque record identifier | Deterministic tiebreaker; matches `UsageRecord.id` of the last row on the emitted page. |

Invariants:

- `timestamp` is the primary sort key; `id` is the deterministic tiebreaker.
  Together they MUST yield a total, stable order across all plugins so that
  `(timestamp, id)` pairs are unique within a tenant's record stream.
- `Keyset` is produced from the last `UsageRecord` of an emitted page and is
  serialized by the toolkit gateway into the opaque `CursorV1` returned in
  `toolkit_odata::Page<UsageRecord>.page_info.next_cursor`.
- `Keyset` is never exposed to callers in raw form; consumers receive only the
  opaque `CursorV1` token and pass it back unmodified.

## 3. Query Domain

The query surface (raw and aggregation) is single-metric in v1: every query
names exactly one metric type via `metric_gts_id`, which lets the gateway
resolve the metric's full `metadata_fields` set (per §2.5 and §2.8) into
the request's admissible filter and grouping fields without cross-metric
reconciliation. Cross-metric aggregation is a non-goal of the v1 query
surface — it would require either a common-dimension projection across
heterogeneous declared-key sets or a degenerate fixed-field-only mode, and
neither is in scope for this revision. See ADR 0012
(`./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`) for the metric
catalog and reference-model decisions.

The metric catalog backing this resolution is the single metric catalog,
managed via the Plugin SPI and persisted in the active storage plugin's
database alongside `usage_records`, per ADR 0012. The gateway reads from a
synchronous L1 cache against that catalog (per-metric, keyed by `gts_id`,
refreshed on register and delete) when admitting filter fields and group-by
dimensions per request.

### 3.1 AggregationQuery

Traceability: `cpt-cf-usage-collector-entity-aggregation-query`

`AggregationQuery` requests server-side aggregation over authorized usage
records. It is available through the SDK trait and REST API and is pushed down
to the Plugin SPI after PDP constraints are applied.

| Field           | Required | Type                     | Description                                                                                                                                                         |
| --------------- | -------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `time_range`    | Yes      | UTC start/end interval   | Mandatory bounded interval. The REST contract defines inclusive/exclusive wire semantics.                                                                           |
| `metric_gts_id` | Yes      | GTS identifier           | Exactly one Metric type. Requests with no Metric or multiple Metrics are rejected. Used to resolve the metric's `metadata_fields` set for `group_by` and `$filter`. |
| `aggregation`   | Yes      | Enum                     | One of `SUM`, `COUNT`, `MIN`, `MAX`, or `AVG`.                                                                                                                      |
| `tenant_id`     | No       | Opaque tenant identifier | User-supplied narrowing filter applied after PDP constraints.                                                                                                       |
| `resource`      | No       | `ResourceRef`            | Optional resource narrowing filter.                                                                                                                                 |
| `subject`       | No       | `SubjectRef`             | Optional subject narrowing filter.                                                                                                                                  |
| `source_gear` | No       | Opaque gear identifier | Optional source-gear narrowing filter.                                                                                                                            |
| `group_by`      | No       | List of dimensions       | Any combination of the **fixed fields** in §2.11 plus every key in the queried metric's **`metadata_fields`** (resolved per request from `metric_gts_id`).          |
| `$filter`       | No       | OData filter expression  | Restricted to the union of fixed fields (§2.11) and every key in the queried metric's `metadata_fields`, with the per-field operator allowances in §2.11.           |

Invariants:

- PDP constraints define the authorization boundary and are applied before
  user-supplied filters.
- User filters can only narrow the authorized scope.
- `group_by` and `$filter` admissibility is computed per request from the
  queried metric's own `metadata_fields` via the gateway L1 cache (keyed by
  `gts_id`); fields outside the union of (fixed fields, every key in that
  metric's `metadata_fields`) are rejected before plugin dispatch with an
  actionable error naming the offending field and the metric.
- Empty result sets inside the authorized scope are not errors.
- Aggregation result size and report-shape limits follow the PRD
  batch-and-report timing NFR and the OpenAPI contract `usage-collector-v1.yaml` (sibling to DESIGN.md).
- **Aggregation across entry types**: `SUM` is computed over both `usage`
  and `compensation` entries — `SUM(value)` is the signed net total per
  group, with compensation rows reducing it. `COUNT`, `MIN`, `MAX`, and
  `AVG` operate over `usage` entries only; `compensation` rows are excluded
  from these aggregations because compensation entries adjust `SUM`; they
  are not events. Inactive records (any `entry_type`) are excluded from all
  five aggregations.

### 3.2 RawQuery

Traceability: `cpt-cf-usage-collector-entity-raw-query`

`RawQuery` requests cursor-paginated raw usage records for **exactly one
metric type**. It is available through the SDK trait and REST API and is
pushed down to the Plugin SPI after PDP constraints are applied.

| Field           | Required | Type                     | Description                                                                                                                                                          |
| --------------- | -------- | ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `time_range`    | Yes      | UTC start/end interval   | Mandatory bounded interval.                                                                                                                                          |
| `metric_gts_id` | **Yes**  | GTS identifier           | Exactly one Metric type. Used to resolve the metric's `metadata_fields` set for `$filter`. Requests with no Metric or multiple Metrics are rejected before dispatch. |
| `tenant_id`     | No       | Opaque tenant identifier | Optional tenant narrowing filter.                                                                                                                                    |
| `resource`      | No       | `ResourceRef`            | Optional resource narrowing filter.                                                                                                                                  |
| `subject`       | No       | `SubjectRef`             | Optional subject narrowing filter.                                                                                                                                   |
| `$filter`       | No       | OData filter expression  | Restricted to the union of fixed fields (§2.11) and every key in the queried metric's `metadata_fields`, with the per-field operator allowances in §2.11.            |
| `cursor`        | No       | `toolkit_odata::CursorV1` | Gears Toolkit-owned opaque continuation marker carried in a previous `toolkit_odata::Page<UsageRecord>`.                                                                     |
| `page_size`     | No       | Positive integer         | Requested page size, bounded by the REST/SDK contract.                                                                                                               |

Invariants:

- `metric_gts_id` is REQUIRED. Rationale: `RawQuery` is single-metric so the
  metric's `metadata_fields` set is always resolvable per request, and the
  admissible filter-field set in §2.11 is well-defined. Cross-metric raw
  scans (across heterogeneous declared-key sets) are a non-goal of the v1
  query surface and remain out of scope; a query intended to span multiple
  metric types is rejected. Query-time dimensions resolve to the metric's
  full `metadata_fields` set.
- PDP constraints are intersected with user filters before plugin dispatch.
- `$filter` admissibility is computed per request from the queried metric's
  own `metadata_fields` via the gateway L1 cache (keyed by `gts_id`);
  fields outside the union of (fixed fields, every key in that metric's
  `metadata_fields`) are rejected before plugin dispatch with an actionable
  error naming the offending field and the metric.
- PDP denial, empty read constraints, or invalid cursor input fail closed with a
  deterministic error.
- No matching records inside the authorized scope returns an empty page, not an
  error.

### 3.3 AggregationResult

Traceability: `cpt-cf-usage-collector-entity-aggregation-result`

`AggregationResult` is the grouped output of an `AggregationQuery`.

| Field           | Required | Type            | Description                                                             |
| --------------- | -------- | --------------- | ----------------------------------------------------------------------- |
| `metric_gts_id` | Yes      | GTS identifier  | Metric that was aggregated.                                             |
| `aggregation`   | Yes      | Enum            | Aggregation function used to produce each value.                        |
| `buckets`       | Yes      | List of buckets | Each bucket contains dimension values and the aggregated numeric value. |

Bucket shape:

| Field        | Required | Type    | Description                                 |
| ------------ | -------- | ------- | ------------------------------------------- |
| `dimensions` | Yes      | Object  | Dimension names and values from `group_by`. |
| `value`      | Yes      | Numeric | Aggregated value for the bucket.            |

Interpretation of `value`:

- When `aggregation = SUM`, `value` is the signed net total across `usage`
  and `compensation` entries within the bucket — compensation entries
  reduce it.
- When `aggregation` is `COUNT`, `MIN`, `MAX`, or `AVG`, `value` is
  computed over `usage` entries only within the bucket; `compensation`
  entries are excluded from these aggregations because compensation
  entries adjust `SUM`; they are not events.
- Inactive records (any `entry_type`) are excluded from every aggregation.

## 4. Authorization Domain

### 4.1 PdpDecision

Traceability: `cpt-cf-usage-collector-entity-pdp-decision`

`PdpDecision` is the permit-or-deny result returned by `authz-resolver` for a
single operation.

| Field         | Required         | Type                    | Description                                                            |
| ------------- | ---------------- | ----------------------- | ---------------------------------------------------------------------- |
| `effect`      | Yes              | Enum                    | `permit` or `deny`.                                                    |
| `constraints` | Read permit only | List of `PdpConstraint` | Read-scope filters that define the authorization boundary.             |
| `reason`      | No               | Opaque reason/category  | Optional PDP-owned explanation used for diagnostics and error mapping. |

Invariants:

- Deny decisions reject the operation before any state change or plugin read.
- Read operations require permit plus non-empty authorized constraints.
- Write operations use the decision over the full attribution tuple and do not
  rely on cached or inferred authorization.

### 4.2 PdpConstraint

Traceability: `cpt-cf-usage-collector-entity-pdp-constraint`

`PdpConstraint` is a server-side query filter returned by PDP with a permit
decision. It is applied before user-supplied filters.

| Field            | Required  | Type                        | Description                              |
| ---------------- | --------- | --------------------------- | ---------------------------------------- |
| `tenant_ids`     | PDP-owned | Set of tenant identifiers   | Authorized tenant scope for the read.    |
| `resource_refs`  | PDP-owned | Set of `ResourceRef` values | Optional authorized resource scope.      |
| `subject_refs`   | PDP-owned | Set of `SubjectRef` values  | Optional authorized subject scope.       |
| `metric_gts_ids` | PDP-owned | Set of GTS identifiers      | Optional authorized Metric scope.        |
| `source_gears` | PDP-owned | Set of gear identifiers   | Optional authorized source-gear scope. |

Invariants:

- Constraints are combined with user filters as an intersection.
- User filters never widen the scope returned by PDP.
- The collector does not cache constraints across requests.

## 5. Plugin Binding Domain

### 5.1 PluginBinding

Traceability: `cpt-cf-usage-collector-entity-plugin-binding`

`PluginBinding` is the in-process pair returned per call by the host
`Service`'s lazy resolution path: the `GtsInstanceId` cached on first use
by `GtsPluginSelector::get_or_init`, and the `Arc<dyn
UsageCollectorPluginV1>` looked up via `ClientHub::try_get_scoped` under
`ClientScope::gts_id(&instance_id)`. There is no separate "Gear
Orchestrator" component — the host gear's own `Service` constructor
materializes the selector, and the plugin gear's `init()` materializes
the scoped client. The SPI major version
is encoded structurally inside `gts_schema_id` (the trailing `.v1~`
segment of `UsageCollectorPluginSpecV1::gts_schema_id()`) and is not
materialized as a separate runtime field.

| Field             | Required | Type                              | Description                                                                                                                                                                                                   |
| ----------------- | -------- | --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `gts_instance_id` | Yes      | `GtsInstanceId`                   | Scope used to resolve the selected storage plugin; cached by `GtsPluginSelector::get_or_init` on first dispatch and reused for the host `Service`'s lifetime. Encodes the SPI major version as a path suffix. |
| `client`          | Yes      | `Arc<dyn UsageCollectorPluginV1>` | The bound plugin's scoped trait object — registered by the plugin gear's `init()` via `ClientHub::register_scoped` under `ClientScope::gts_id(&instance_id)` and cloned out on each dispatch.               |

Invariants:

- The Usage Collector has exactly one active storage binding per configured GTS
  instance scope.
- Bootstrap fails when no binding can be resolved (no matching
  `PluginV1<UsageCollectorPluginSpecV1>` instance in `types-registry`, or
  `ClientHub::try_get_scoped` returns `None` after selector resolution —
  surfaced as `PluginUnavailable`).
- The collector does not invent a fallback binding or keep a parallel local
  persistence path.
- Plugin SPI compatibility follows the public major-version stability contract;
  the SPI version is encoded in `gts_schema_id` (e.g.
  `cf.core.credstore.plugin.v1~`),. No runtime
  negotiation.
- Binding "state" is not modeled as a finite state machine (no
  `Unbound`/`Resolving`/`Bound`/`Refreshing`/`Failed` discriminants exist in
  the reference gears); it is recomputed on each call from the two
  structural facts above.

## 6. Surface Mapping

| Surface    | Consumes                                                                                                               | Produces                                                                                                                                                                                                                                          |
| ---------- | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| SDK trait  | Usage submissions, `AggregationQuery`, `RawQuery`, deactivation requests                                               | Per-record acknowledgements, `AggregationResult`, `toolkit_odata::Page<UsageRecord>`, deactivation outcome                                                                                                                                         |
| REST API   | Same as SDK plus Metric registration, Metric list/get/delete, and health probe requests                                | Same as SDK plus Metric catalog state, health probe payloads, and platform-standard errors. Operational telemetry is pushed via OTLP from ToolKit's `SdkMeterProvider`; no in-gear HTTP metrics endpoint is exposed.                             |
| Plugin SPI | Idempotency-keyed `UsageRecord` persistence commands, query commands, Metric lifecycle commands, deactivation commands | Persistence acknowledgements, dedup outcomes (silently deduplicated on an exact-equality retry, Conflict on a canonical-field mismatch), `AggregationResult`, `toolkit_odata::Page<UsageRecord>`, Metric catalog results, classified plugin errors |

### 6.1 Error Envelope

`UsageCollectorError` is the public error envelope across all SDK surface
methods. It is declared in `usage-collector-sdk/src/error.rs` (the SDK
crate, not the host) as a flat `thiserror::Error` enum and is
transport-agnostic. The SDK crate does **not** depend on
`toolkit-canonical-errors`; consumers pattern-match variants directly.

The host crate (`usage-collector`) lifts `UsageCollectorError` onto the
canonical `toolkit_canonical_errors::CanonicalError` via
`From<UsageCollectorError> for CanonicalError` in
`usage-collector/src/infra/sdk_error_mapping.rs`; `CanonicalError`'s
built-in `IntoResponse` produces the RFC-9457 `Problem` envelope on the
REST surface. The variant → AIP-193 category → HTTP-status mapping
table is owned by DESIGN.md §3.3 Error Envelopes; the SDK variant
catalog is owned by `sdk-trait.md` "Error Taxonomy". A companion
plugin-side enum `UsageCollectorPluginError` is owned by `plugin-spi.md`
"Error Taxonomy".

## 7. Cross-Entity Invariants

- Every accepted record references a Metric in the metric catalog (managed
  via the Plugin SPI, persisted in the active storage plugin's database; see
  ADR 0012 at
  `./ADR/0012-unified-plugin-catalog-and-gts-id-reference.md`) by `gts_id`.
- Every write and read operation requires a resolved `SecurityContext` and a PDP
  decision.
- The collector fails closed on missing/invalid `SecurityContext`, PDP
  unavailability, validation failure, plugin readiness, or storage errors.
- `RecordMetadata` is the only extensible per-record payload; it remains opaque
  to the collector.
- Physical lifecycle, retention, backup, archival, purging, and backend-specific
  query acceleration are plugin-owned.
- Public REST, SDK, and Plugin SPI schemas may add optional fields within a
  major version, but removing fields or changing semantics requires the
  appropriate major-version break.
