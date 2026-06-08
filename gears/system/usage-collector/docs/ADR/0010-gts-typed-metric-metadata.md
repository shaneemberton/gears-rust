---
status: superseded
superseded_by: cpt-cf-usage-collector-adr-0012-unified-plugin-catalog-and-gts-id-reference
date: 2026-05-30
---

> Superseded by [0012](./0012-unified-plugin-catalog-and-gts-id-reference.md).

# Model usage-collector metrics as GTS Type Schemas with declared dimensions and trait-modelled kind

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Declared-per-metric typed metadata schema, metrics modelled as GTS Type Schemas](#declared-per-metric-typed-metadata-schema-metrics-modelled-as-gts-type-schemas)
  - [Metric-id naming convention plus opaque metadata](#metric-id-naming-convention-plus-opaque-metadata)
  - [Opaque metadata plus generic backend indexing](#opaque-metadata-plus-generic-backend-indexing)
- [More Information](#more-information)
- [Traceability](#traceability)

<!-- /toc -->

**ID**: `cpt-cf-usage-collector-adr-gts-typed-metric-metadata`

## Context and Problem Statement

Reviewer feedback recorded in `RESEARCH-metadata.md` §1 (Trigger) flagged that
the current Usage Collector (UC) contract — opaque, unindexed `metadata` plus a
metric identifier as the only typed dimension — forces consumers into two bad
shapes: fold every extra dimension into `metric_gts_id` (combinatorial explosion
of metric ids) or stuff structured data into `metadata` and re-aggregate
client-side. Either shape conflicts with PRD §1.3, which commits UC to
server-side aggregation: downstream consumers must obtain aggregated views
directly from UC without running their own aggregation layer. The static
`UsageRecordFilterField` enum (`domain-model.md` §2.11) cannot carry per-metric
dimensions, so `group by product_family` or any other business dimension is
structurally impossible inside UC today.

ADR-0009 (`cpt-cf-usage-collector-adr-catalog-plugin-referential-integrity`) is
the prerequisite for this decision: it restores the Plugin Service Provider
Interface (SPI) catalog surface (the five catalog methods that ADR-0007 had
removed), pins the metric catalog rows physically alongside `usage_records` in
the storage plugin's database, and enforces referential integrity with a real
`ON DELETE RESTRICT` foreign key (FK). ADR-0010 layers a schema-carrying API
onto that restored substrate; it does not relitigate where the catalog lives or
who owns it.

The question this ADR answers: how does usage-collector model metric metadata
so that server-side aggregation can group by per-metric dimensions with
data-quality enforcement at ingest, while remaining backend-agnostic across the
pluggable-storage seam ADR-0002 restored (via ADR-0009)?

## Decision Drivers

- **PRD §1.3 server-side aggregation goal** — the metadata substrate must
  support the OData-style `$filter` and `$apply` (group/aggregate) surface over
  per-metric dimensions, not just the fixed-field set today's
  `AggregationQuery.group_by` permits.
- **Data quality at ingest** — gateway Level-1 (L1) validation must reject
  malformed metadata before the plugin inserts the row, so backends never see
  garbage; this is the only place where validation can be enforced once across
  every backend.
- **Backend agnosticism** — the metadata model must work across the pluggable
  storage seam (ADR-0002's restored scope, per ADR-0009) without baking
  per-backend assumptions into the gateway or into the public API.
- **Centralized validation** — one validator (the `jsonschema` crate over the
  merged effective schema) at the gateway, not N per-plugin validators that
  drift over time.
- **ID stability across descendants** — using UUID v5 derivation per
  Global Type System (GTS) guide §5.1 keeps the FK column stable even as the
  type-id chain evolves at the string level (renames/aliases) and keeps storage
  fixed-width.
- **No new platform coupling for a small merge core** — the ~40–150 lines of
  `effective_schema` / `merge_schema_with_parent` / `effective_traits` needed
  for the validation hot path are pure functions over `serde_json::Value`;
  taking a runtime dependency on `types-registry-sdk` for that surface is
  disproportionate (see `RESEARCH-metadata.md` §9.3).

## Considered Options

- **Declared-per-metric typed metadata schema, metrics modelled as GTS Type
  Schemas** — each metric type id ends `~`, declares an open-but-typed JSON
  Schema for metadata, declares indexable dimensions via a property-level GTS
  trait (`x-uc-indexable: true`), and inherits via `allOf` from its parent
  type along an explicitly registered chain. The platform base
  `gts.cf.core.usage.metric.v1~` and any operator-registered intermediate
  "extension point" type are marked `x-gts-abstract`; concrete leaves carry
  usage rows.
- **Metric-id naming convention plus opaque metadata** — use a string
  convention on `metric_id` (e.g., `kind.dimensionA.dimensionB.v1`) to convey
  shape; keep `metadata` fully opaque; require operator-supplied conventions to
  live outside the system. Aggregation engines work off the parsed
  `metric_id` segments.
- **Opaque metadata plus generic backend indexing** — keep `metadata` fully
  opaque, rely on the storage backend to index whichever Java Script Object
  Notation (JSON) fields it can (Timescale's JSONB-Generalized Inverted Index
  (GIN), ClickHouse's `Map(String, String)` columnar indexing), and surface
  those fields generically through query APIs without per-metric typing.

## Decision Outcome

Chosen option: **declared-per-metric typed metadata schema with each metric
modelled as a GTS Type Schema**, because it is the only option that satisfies
PRD §1.3's server-side aggregation promise with data-quality enforcement at
ingest while keeping the metadata contract backend-agnostic. The pure-string
naming convention provides no validation, no inheritance, and no introspection
surface; the opaque-plus-generic-indexing option breaks ADR-0002 backend
agnosticism (the query semantics become whatever each backend chooses to
support).

The decision pins all of the following invariants. They are load-bearing for
the cascade phases that follow and must not be diluted by downstream artifacts:

- **Metric-as-type rule** — every metric type identifier ends with `~`; every
  metric registered through the catalog SPI ADR-0009 restored is a GTS Type
  Schema, not a GTS instance. Concrete leaves (the only types that may carry
  usage rows) are non-abstract; the platform base
  `gts.cf.core.usage.metric.v1~` and any operator-registered intermediate
  "extension point" type are marked `x-gts-abstract` and MUST NOT receive
  usage rows.
- **Open-but-typed metadata schema** — declared properties on the merged
  effective schema are type-checked at the gateway via the inlined
  `effective_schema()` (the per-level `raw_schema` walked through `parent`
  Arc-links and inlined into one `allOf` chain); `additionalProperties` is NOT
  closed; the free-form remainder is preserved as ordinary `metadata` extras so
  the PRD §5.1 arbitrary-context FR continues to hold without a separate field.
- **Descendant extension via `allOf`** — derived metric types extend their
  parent's schema via `allOf` with a `$ref` to the parent (`$id` style
  `gts://<parent-type-id>`); inheritance is additive. The merge core (lifted
  in-tree per "Code reuse" below) resolves the `gts://` references because
  vanilla JSON-Schema validators cannot fetch that scheme.
- **`kind` modelled as a GTS trait** — `kind` (e.g., `consumption` /
  `allocation` / `rate`) lives under `x-gts-traits` on each metric type;
  inheritance is carried via `effective_traits` per GTS guide §10 with
  rightmost/child wins on key collisions. The base `x-gts-traits-schema`
  defines the allowed keys, value types, and defaults. Note: per the GTS
  guide §10 examples, `kind` is normally a categorical value; the well-known
  instance option (per the GTS guide §6.6) remains available if a future
  refinement wants registry-discoverable kind values, but ADR-0010 pins
  trait-based modelling for now and does not require well-known instances.
- **Indexable dimensions** — indexable dimensions are a subset of the
  declared schema properties marked via a property-level GTS trait
  (`x-uc-indexable: true` on the relevant property). The trait travels through
  inheritance like any other trait; descendants may extend the indexable set
  but MUST NOT remove a parent-declared indexable dimension. Only properties
  marked indexable are eligible for `$filter` / `$apply` group-by; undeclared
  metadata extras are preserved but not queryable.
- **UUID-v5 FK on usage rows** — `usage_record.metric_type_uuid UUID NOT NULL
REFERENCES <catalog>(metric_type_uuid)`, where the UUID is derived
  deterministically from the metric type id per GTS guide §5.1
  (`gts::GtsID::new(...).to_uuid()`). This is the same FK column ADR-0009
  introduced as `ON DELETE RESTRICT` on the plugin side. Type id strings remain
  in logs and human-readable API responses, but the FK column is UUID.
- **Explicit ancestor registration** — every intermediate ancestor metric type
  MUST be registered in the catalog before any derived type referencing it may
  be registered or used. Chain assembly walks the catalog from leaf to base.
  This resolves the §11 open question that the research memo flagged.
- **Leaves-only usage attach** — a metric registered via the API is concrete
  and a leaf at registration time; `usage_record.metric_type_uuid` MUST refer
  to a non-abstract leaf type at insert time. Flipping a leaf into an
  intermediate "extension point" requires the operator to mark the original
  abstract via the metric-lifecycle path defined by ADR-0009 (which surfaces
  the structured "metric referenced" error if the leaf still has rows).
- **Closed-shape footgun** — the `allOf` + `additionalProperties: false`
  shape is rejected as a configurable default. The reason is the standard
  JSON-Schema rule that `additionalProperties` only "sees" properties declared
  in the same schema object, while `allOf` validates the instance against each
  branch independently and against the whole instance. Worked example reused
  verbatim from `RESEARCH-metadata.md` §7.2: a parent type declares
  `{ "properties": { "region": {"type":"string"} }, "additionalProperties":
false }` and a descendant adds `{ "allOf": [ { "$ref": "gts://<parent>~" },
{ "properties": { "model": {"type":"string"} },
"additionalProperties": false } ] }`. Validating the obviously-correct
  instance `{ "region": "us-east", "model": "gpt-4" }` fails on both branches
  — branch 1 rejects `model` as an illegal additional property, branch 2
  rejects `region` for the same reason. Neither branch can see properties
  contributed by the other, and `unevaluatedProperties` (the Draft 2019-09 fix)
  is forbidden by GTS guide §5.5 because GTS schemas are pinned to Draft-07.
  Open-but-typed sidesteps this entirely; full-property flattening is only
  required if a closed shape is ever wanted, and that remains a deferred,
  per-metric escape hatch rather than the default.

### Consequences

- **Cascade targets to be updated in subsequent phases of this rework**:
  - `domain-model.md` — §2.5 (Metric gains `metadata_schema`, declared
    dimensions, type-id semantics), §2.8 (RecordMetadata becomes
    schema-validated with open remainder), §2.11
    (`UsageRecordFilterField` becomes fixed-fields plus dynamic per-metric
    dimensions), §3 (RawQuery `metric_gts_id` required; query domain extension
    semantics) — Phase 3.
  - `PRD.md` — §5.1 (metadata FR: typed-with-open-remainder), §5.4
    (pluggable-storage carve-out reverts in lockstep with ADR-0009), §5.7
    (metric lifecycle: schema-carrying register / delete) — Phase 4.
  - `DESIGN.md` — §1.2 Key ADRs (add ADR-0010 link, alongside the ADR-0009
    link Phase 5 also adds), §3.7 logical tables (catalog row carries
    `metadata_schema JSONB`; usage row carries `metric_type_uuid UUID` FK) —
    Phase 5.
  - `plugin-spi.md` — re-add the five catalog SPI methods ADR-0009 restored,
    now carrying the GTS-typed metadata schema as a payload on register /
    fetch; preserve the structured "metric referenced" error from ADR-0009 —
    Phase 6.
  - `sdk-trait.md` — register / delete metric SDK surface carries the metadata
    schema and indexable-dimension declarations — Phase 7.
  - `DECOMPOSITION.md` — capture the lift of the merge core in-tree and the
    explicit non-dependency on `types-registry-sdk`; surface the gateway L1
    validation cache as a load-bearing component — Phase 8.
  - `features/foundation.md` + `features/metric-lifecycle.md` — feature spec
    alignment for the schema-carrying lifecycle path — Phase 9.
  - `usage-collector-v1.yaml` (OpenAPI) — `RawQuery.metric_gts_id` becomes
    required; AggregationQuery `$filter` / `$apply` admit dynamic per-metric
    dimensions on top of the fixed-field set; metric register / delete payloads
    carry `metadata_schema` — Phase 10.
- The static `UsageRecordFilterField` enum (`domain-model.md` §2.11) is
  replaced by **fixed fields plus per-metric-declared dimensions resolved per
  request** from the catalog. Fixed fields remain (`tenant`, `resource`,
  `subject`, `source_gear`, `timestamp`, `status`); dynamic dimensions are
  resolved by looking up the metric type's `effective_schema()` and selecting
  the properties marked `x-uc-indexable: true`.
- **`RawQuery.metric_gts_id` becomes required** (was optional). Dimension
  resolution requires the metric type; query-time without a metric type can no
  longer be answered with declared dimensions in scope. Aggregation queries
  remain single-metric for the same reason; cross-metric aggregations would
  require a separate decision and are out of scope here.
- The deliberate stance in `domain-model.md:259` ("The parent type id ... is
  not declared as a GTS Type Schema") is **flipped** — metrics become
  first-class GTS Type Schemas. The cascade in Phase 3 rewrites that paragraph
  in lockstep with the §2.5 update.
- The **gateway L1 validation cache** becomes load-bearing for the ingest hot
  path: it must be present and correct for the gateway to validate metadata
  against the merged effective schema before dispatching to the plugin.
  Invalidation on register / delete cascades to descendants. The implementation
  model is `local_client.rs:86-97` (the cascade-invalidation pattern already
  used in `types-registry`); the concrete invalidation logic is a code-level
  concern, not an ADR-level decision.
- **Types-registry publication remains optional / deferred** — ADR-0010 does
  not require publication to the `types-registry` gear for cross-service
  discovery. The catalog SPI restored by ADR-0009 is the authoritative
  durable source; publishing a rebuildable projection upstream is left as a
  later, additive decision.
- **Code reuse — lift, do not depend**: usage-collector lifts the small merge
  core (~40–150 lines of `effective_schema` / `merge_schema_with_parent` /
  `effective_traits`) in-tree per `RESEARCH-metadata.md` §9.3, validates with
  the `jsonschema` crate over the merged `Value`, and keeps the existing `gts`
  crate dependency for id parsing and UUID derivation. The runtime dependency
  on `types-registry-sdk` is **not** taken for this surface. A code comment
  must point back to `types-registry/types-registry-sdk/src/models.rs` so the
  two copies do not silently diverge on `allOf`-inlining semantics.
- **Wire status code for `MetadataValidationError` is HTTP 400 (canonical).**
  `MetadataValidationError` is the only metadata-schema validation error
  surfaced by the gateway L1 validator. It lifts to HTTP 400 (AIP-193
  `InvalidArgument`) on the wire, NOT 422 — consistent with the broader
  AIP-193 error envelope used across the usage-collector REST surface. The
  wire encoding pins the status code at `usage-collector-v1.yaml:2323-2325`
  (error envelope `status: enum [400]`) and is mirrored in
  `sdk-trait.md:1588-1591` ("Lifts to AIP-193 InvalidArgument (HTTP 400) on
  the wire"). DESIGN.md deliberately does not pin the status code; the
  wire-level pin lives in the OpenAPI surface.
- **Catalog row column naming canon — `_type_uuid` suffix family.** Catalog
  row column naming follows a uniform `_type_uuid` suffix family: `type_uuid`
  (PK on the catalog row), `parent_type_uuid` (FK to the parent metric type
  in the hierarchy), and `metric_type_uuid` (the FK from `usage_records` to
  `metric_catalog`). The suffix family is enforced across DESIGN.md §3.7,
  `plugin-spi.md` Catalog table, `usage-collector-v1.yaml` `MetricType`
  schema, and DECOMPOSITION.md §2.2; future readers MUST treat
  `parent_type_uuid` (not `parent_uuid`) as the canonical column name.

### Confirmation

- `jsonschema`-crate validation passes over the merged effective schema in the
  gateway L1 path; metadata payloads that violate declared property types or
  required-field constraints are rejected before reaching the plugin.
- Referential integrity at delete time is enforced by the ADR-0009 plugin DB
  constraint (`ON DELETE RESTRICT` on `metric_type_uuid`); ADR-0010 reuses
  that same FK column and adds no new referential semantics.
- Gateway L1 cache invalidation on register / delete cascades to descendants;
  the implementation model is `local_client.rs:86-97`. A cache that fails to
  cascade is a defect against this ADR, not a tolerated divergence.
- `cypilot validate` PASS for the new ADR after the Phase 2 commit; transient
  cross-reference gaps from later cascade phases (DESIGN.md not yet citing
  ADR-0010, PRD / feature / SPI updates not yet landed) are expected and
  resolved by Phases 3-10.
- `cypilot validate-toc` PASS for the new ADR file after the Phase 2 `cypilot
toc` run.
- PR review approval on branch `usage-collector/simplified-specs` once the full
  cascade chain lands.

## Pros and Cons of the Options

### Declared-per-metric typed metadata schema, metrics modelled as GTS Type Schemas

Each metric type id ends `~`, declares an open-but-typed JSON Schema for
metadata, declares indexable dimensions via a property-level GTS trait, and
inherits via `allOf` from its parent type along an explicitly registered chain.
The platform base and operator-registered intermediates are abstract; concrete
leaves carry usage rows. The FK column on `usage_record` is UUID v5 of the
type id per GTS guide §5.1.

- Good, because PRD §1.3's server-side aggregation goal is met: the dynamic
  `$filter` / `$apply` surface resolves per-metric declared dimensions from the
  catalog and groups / filters by them inside UC, with no client-side
  aggregation step.
- Good, because data quality is enforced once at the gateway via the
  `jsonschema` crate over the merged effective schema, so plugins never see
  malformed metadata and backend implementations need not re-implement
  validation.
- Good, because backend agnosticism (ADR-0002 restored by ADR-0009) is
  preserved — the validation logic and the indexable-dimension contract live
  in the gateway; backends remain free to choose their physical indexing
  strategy (JSONB-GIN, columnar, materialized views) without changing the
  public API.
- Good, because UUID-v5 FK derivation keeps the storage FK fixed-width and
  stable across id renames or aliases (per GTS guide §5.1) and avoids
  variable-length text comparisons on the hot path.
- Good, because GTS trait machinery (`x-gts-traits` /
  `x-gts-traits-schema`) already exists and is already validated by the GTS
  Rust library, so modelling `kind` and the indexable-dimension marker as
  traits reuses platform invariants instead of inventing new ones.
- Good, because the open-but-typed shape accommodates legacy callers that
  send extra keys on `metadata`: undeclared keys are preserved as free-form
  remainder rather than rejected, so this decision does not break the PRD §5.1
  arbitrary-context guarantee.
- Bad, because the gateway L1 validation cache becomes load-bearing — a cache
  bug at the ingest tier becomes an ingest-time data-quality regression rather
  than a perf-only issue.
- Bad, because operator coordination grows with the declared-property surface:
  any addition or rename of a declared dimension is a contract change for
  downstream aggregation consumers, even though the underlying schema is
  open-but-typed.
- Bad, because the open-but-typed shape is permissive on undeclared keys —
  typos in dimension names go through as ordinary metadata extras (they are
  not queryable so they cannot poison `group by`, but they are silently
  accepted; downstream consumers must detect and normalize them if they care).
  This is the deliberate trade for not closing the schema.
- Bad, because the merge core (~40–150 lines) must be lifted and owned by
  usage-collector instead of consumed via `types-registry-sdk`; a code comment
  must keep the two implementations in sync, which is real maintenance cost.
- Neutral, because operators must explicitly register every intermediate
  ancestor before a derived type referencing it can be registered or used —
  this is the chosen ancestor-registration policy and an honest operational
  cost rather than a defect.

### Metric-id naming convention plus opaque metadata

Use a string convention on `metric_id` (e.g., `kind.dimensionA.dimensionB.v1`)
to convey shape; keep `metadata` fully opaque; require operator-supplied
conventions to live outside the system. Aggregation engines parse the
`metric_id` segments to discover dimensions.

- Good, because zero schema infrastructure is needed inside UC: no merge
  core, no gateway L1 validation, no indexable-dimension trait.
- Good, because the option is trivial to ship — the registration path stays
  exactly as it is today and the storage layout does not change.
- Good, because it works on any backend without DDL preparation; the backend
  has no schema to honor.
- Bad, because there is no enforcement: typos in dimension segments silently
  create new metric ids, and aggregation across "the same dimension" requires
  client-side reconciliation. This is precisely what `RESEARCH-metadata.md`
  §1 reviewer feedback rejects as inadequate.
- Bad, because the convention drifts: each operator can encode dimensions
  differently, and a consumer that wants "give me sum(value) grouped by
  product_family" must enumerate every possible metric id client-side and
  aggregate locally — exactly the shape PRD §1.3 forbids.
- Bad, because the aggregation engine cannot introspect declared dimensions
  — there are none. Group-by semantics collapse back to the fixed-field set
  (`tenant`, `resource`, `subject`, `source_gear`, `timestamp`, `status`).
- Bad, because there is no inheritance: a vendor that wants to add a
  dimension on top of a platform base metric must mint a new metric id and
  reconcile downstream, with no merge support from UC.
- Neutral, because GTS guide §2 declares metric ids opaque to UC; parsing
  them for semantic content is explicitly out of scope and adopting this
  option would conflict with that.

### Opaque metadata plus generic backend indexing

Keep `metadata` fully opaque; rely on the storage backend to index whichever
JSON fields it can (Timescale's JSONB-GIN, ClickHouse's
`Map(String, String)` columnar indexing); surface those fields generically
through query APIs without per-metric typing.

- Good, because gateway logic stays minimal — no merge core, no validation,
  no indexable-dimension contract.
- Good, because each backend can optimize for its own strengths: Timescale
  prepares JSONB-GIN indexes, ClickHouse uses its `Map(String, String)`
  columnar form, future backends could choose whatever they like.
- Bad, because the query surface becomes backend-specific. Two backends that
  both honor the public API will diverge on what "group by dimension X" means
  in practice — Timescale honors JSONB-GIN-eligible paths, ClickHouse honors
  `Map` keys, and neither agrees on the projection semantics. This directly
  breaks ADR-0002's backend agnosticism, which ADR-0009 explicitly restored
  for the catalog.
- Bad, because there is no centralized validation: every plugin would have
  to re-implement schema enforcement or accept any JSON it receives. Drift
  between plugin implementations is inevitable.
- Bad, because there is no inheritance and no way to mark which keys on
  `metadata` are "real" dimensions vs. free-form extras — downstream
  consumers cannot reliably distinguish a typo from a new dimension.
- Bad, because the option offers no path to data-quality enforcement at
  ingest. Garbage flows to the plugin and only surfaces at aggregation time,
  if at all.

## More Information

- The `unit` field-vs-trait deferral is recorded explicitly as a
  **non-blocking open item**. ADR-0010 does not pin whether `unit` is a
  top-level declared property on the metric metadata schema (the same shape
  as any other declared dimension) or a GTS trait under `x-gts-traits`
  alongside `kind`. Both options remain on the table; whichever decision
  lands later MUST NOT break the headline decision in this ADR. Recording the
  deferral here so the open item does not get lost.
- **Do not depend on `types-registry-sdk`** for the merge core. Per
  `RESEARCH-metadata.md` §9.3, the surface usage-collector actually needs is
  ~40 lines for pure schema merge (`effective_schema` +
  `merge_schema_with_parent`) and ~55 additional lines for trait inheritance
  (`effective_traits`, used by the kind-as-trait and indexable-dimension-trait
  decisions). The `types-registry-sdk` crate is a lib/SDK that drags in the
  full client trait, register / query types, error enum, and pins to its
  `gts` version; the coupling is disproportionate for ~40–150 lines of pure
  functions over `serde_json::Value`. Precedent: the platform team already
  reimplemented `effective_traits` locally because upstream `gts-rust` did
  not expose `resolve_schema(...)` publicly (`models.rs:291` TODO marker).
  Lifting into usage-collector is consistent with what already happened
  upstream.
- Evidence and references:
  - `RESEARCH-metadata.md` §3 (Design Space — Dimensions): the three coherent
    shapes for indexable dimensions and why declared-per-metric was chosen.
  - `RESEARCH-metadata.md` §5 (Adopting GTS Type-Schemas), including §5.1
    (mechanism verified in code), §5.2 (the instance-to-type pivot), §5.3
    (hybrid-storage precedent).
  - `RESEARCH-metadata.md` §7 (Schema Model Details), including §7.1
    (open-but-typed vs strict-closed), §7.2 (the `allOf` +
    `additionalProperties: false` footgun, reused verbatim above), §7.3
    (inheritance and inherited type-checking).
  - `RESEARCH-metadata.md` §8 (Validation Strategy), including the
    gateway-L1 placement and the cache pattern.
  - `RESEARCH-metadata.md` §9 (Code Reuse Audit), including the §9.3
    lift-don't-depend recommendation.
  - `RESEARCH-metadata.md` §10 (Decisions Summary) for the consolidated
    decision blocks.
  - `RESEARCH-metadata.md` §11 (Open Question) — the type-hierarchy semantics
    open question is now answered by this ADR: explicit ancestor registration,
    leaves-only usage attach, platform base plus operator-flagged intermediate
    extension points are abstract.
  - GTS guide §5.1 (UUID v5 of type id as FK column), §5.2 (resource-group
    hybrid pattern as precedent), §5.3 (`events.type_uuid` as precedent),
    §5.5 (Draft-07 pinning, `allOf` inheritance and the forbidden post-Draft-07
    keywords including `unevaluatedProperties`), §10 (`x-gts-traits` semantics
    and inheritance), §11 (abstract and final types).
  - ADR-0009 (`cpt-cf-usage-collector-adr-catalog-plugin-referential-integrity`)
    final text — the catalog SPI and FK surface ADR-0010 layers onto.
- Domain applicability:
  - **ARCH** — addressed (this IS the architectural decision: model metrics
    as GTS Type Schemas with declared open-but-typed metadata, kind-as-trait,
    indexable-dimension trait, UUID-v5 FK).
  - **DATA** — addressed (FK column type pinned as `metric_type_uuid UUID
NOT NULL`; full table schema deferred to DESIGN per ARCH-ADR-NO-001).
  - **PERF** — addressed (gateway L1 validation cache invariant; per-request
    dimension resolution; cascade-invalidation on register / delete).
  - **INT** — addressed (catalog SPI surface restored by ADR-0009 carries the
    schema as a payload; no new gear integration introduced).
  - **MAINT** — addressed (lift-and-own merge core; explicit non-dependency on
    `types-registry-sdk`; comment-link the lifted code back to its origin).
  - **SEC** — Not applicable: ADR-0010 introduces no secrets, no auth
    changes, no Personally Identifiable Information (PII) surface; the existing
    Policy Decision Point (PDP) authorization layer ADR-0001 defines remains
    unchanged.
  - **REL** — addressed via Confirmation (FK + `jsonschema` validation +
    cache invalidation).
  - **OPS** — Not applicable: operator-facing register / delete flow is
    ADR-0009 territory; ADR-0010 does not change operational procedures.
  - **TEST** — Not applicable in the ADR body: test design belongs in
    `features/*` (Phase 9) per TEST-ADR-NO-001.
  - **COMPL** — Not applicable: no regulated-data implications.
  - **UX** — addressed indirectly (query surface changes:
    `RawQuery.metric_gts_id` required; dynamic `$filter` per metric).
  - **BIZ** — Not applicable in the ADR body: requirements belong in PRD per
    BIZ-ADR-NO-001.

## Traceability

- **PRD**: [PRD.md](../PRD.md)
- **DESIGN**: [DESIGN.md](../DESIGN.md)

This decision directly addresses or relates to the following requirements,
decisions, or design elements:

- builds on `cpt-cf-usage-collector-adr-catalog-plugin-referential-integrity`
  (ADR-0009) — the catalog SPI surface and the `metric_type_uuid` FK column
  ADR-0010 layers the schema-carrying API on top of.
- relates to `cpt-cf-usage-collector-adr-pluggable-storage` (ADR-0002) — the
  restored pluggable-storage scope (per ADR-0009) covers the metric catalog
  that now carries the typed metadata schema; ADR-0010 does not alter the
  Plugin SPI binding decision in ADR-0002.
- preserves `cpt-cf-usage-collector-adr-caller-supplied-attribution`
  (ADR-0003) — caller-supplied attribution invariants are unchanged; ADR-0010
  only adds the typed-dimension surface alongside `metadata`, not in place of
  it.
- relates to `cpt-cf-usage-collector-adr-contract-stability` (ADR-0006) — the
  public API extension for dynamic `$filter` / `$apply` over declared
  dimensions is additive (new query parameters resolved per metric type);
  existing fixed-field aggregation queries continue to be honored without
  change.
- `cpt-cf-usage-collector-fr-metric-registration` — metric registration now
  carries an open-but-typed metadata schema and the indexable-dimension
  declarations; PDP authorization and validation remain at the gateway, then
  cross the Plugin SPI to the plugin DB.
- `cpt-cf-usage-collector-fr-metric-deletion` — referential delete semantics
  are inherited from ADR-0009 unchanged; the FK column whose enforcement
  ADR-0009 pinned is the `metric_type_uuid` column this ADR pins as
  UUID v5 of the type id.
- `cpt-cf-usage-collector-fr-pluggable-storage` — the pluggable-storage seam
  carries the catalog rows (with their `metadata_schema` payload) and the
  usage rows (with their `metric_type_uuid` FK) into the plugin DB, alongside
  the existing usage-record surface.
