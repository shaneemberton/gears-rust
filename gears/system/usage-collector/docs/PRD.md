---
cpt:
  kind: PRD
  version: 0.9.1
  status: draft
  updated: 2026-06-03
---

# PRD — Usage Collector

<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Environment](#3-operational-concept--environment)
  - [3.1 Gear-Specific Environment Constraints](#31-gear-specific-environment-constraints)
- [4. Scope](#4-scope)
  - [4.1 In Scope](#41-in-scope)
  - [4.2 Out of Scope](#42-out-of-scope)
- [5. Functional Requirements](#5-functional-requirements)
  - [5.1 Usage Ingestion](#51-usage-ingestion)
  - [5.2 Metric Kinds](#52-metric-kinds)
  - [5.3 Attribution & Isolation](#53-attribution--isolation)
  - [5.4 Pluggable Storage](#54-pluggable-storage)
  - [5.5 Usage Query & Aggregation](#55-usage-query--aggregation)
  - [5.6 Corrections (Event Deactivation & Usage Compensation)](#56-corrections-event-deactivation--usage-compensation)
  - [5.7 Metrics](#57-metrics)
  - [5.8 Security and Data Governance](#58-security-and-data-governance)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [6.1 Gear-Specific NFRs](#61-gear-specific-nfrs)
  - [6.2 NFR Exclusions](#62-nfr-exclusions)
- [7. Public Library Interfaces](#7-public-library-interfaces)
  - [7.1 Public API Surface](#71-public-api-surface)
  - [7.2 External Integration Contracts](#72-external-integration-contracts)
  - [7.3 Endpoints Summary](#73-endpoints-summary)
- [8. Use Cases](#8-use-cases)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)
- [13. Open Questions](#13-open-questions)
- [14. Traceability](#14-traceability)

<!-- /toc -->

## 1. Overview

### 1.1 Purpose

A usage metering gear for collecting usage records from platform services and providing aggregated usage data to clients. The Usage Collector is the centralized product surface for platform usage data: it accepts usage records, retains them durably, and serves raw and aggregated views to downstream consumers.

### 1.2 Background / Problem Statement

Platform services need a centralized place to report resource consumption (API calls, AI tokens, storage bytes, compute hours) so that downstream systems (billing, quota reporting, dashboards) can operate on consistent data. Without a central usage gear, each consumer implements its own collection logic, leading to inconsistent data, duplicated effort, and no single source of truth.

The Usage Collector addresses this by accepting usage records from source gears and providing a query/aggregation API to consumers. Business logic (pricing, billing rules, invoice generation, quota enforcement decisions) remains the responsibility of downstream consumers.

### 1.3 Goals (Business Outcomes)

- **Centralized metering**: All platform services that measure resource consumption report to a single authoritative store, eliminating per-service tracking implementations and data inconsistencies across the platform.
- **Operator self-service for new Metrics**: Platform operators can register new billable Metrics (e.g., GPU hours, custom credit units) via API without code changes or service redeployment, supporting rapid product iteration.
- **Downstream consumers need no aggregation layer**: Billing, quota enforcement, and dashboard systems obtain aggregated usage views directly from the Usage Collector within interactive latency bounds, without maintaining their own aggregation infrastructure.
- **Developer integration efficiency**: Platform developers can integrate a service with the SDK or REST API using published examples and receive actionable validation errors during ingestion.
- **Operator support readiness**: Platform operators can diagnose common ingestion, authorization, Metric lifecycle, and storage-extension readiness problems using self-service documentation and standard service health information.

**Success Metrics**:

| Goal                                           | Measurable Success Criterion                                                                                                                                                                                 | Baseline                                                                                                                | Target                                                                                                                           | Timeframe                                                                                             |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Centralized metering                           | Existing platform services with billable operations integrated with Usage Collector as the authoritative usage source                                                                                        | No authoritative platform-wide usage source; billable services use per-service or consumer-specific tracking            | 100% of existing billable platform services integrated; zero per-service custom metering implementations remain for launch scope | By first production deployment; verified again within 30 calendar days after launch                   |
| Operator self-service                          | Time to register a new billable Metric and emit the first accepted record without code changes or service redeployment                                                                                       | New billable usage dimensions require service-specific coordination outside Usage Collector                             | ≤ 5 minutes from authorized API request to first accepted record for a valid Metric                                              | Available at first production deployment and sustained in monthly release-readiness checks            |
| Downstream consumers need no aggregation layer | Registered launch consumers serve primary aggregation use cases through the Usage Collector query API                                                                                                        | Billing, quota, and dashboard consumers require separate aggregation paths or cannot use one authoritative query source | 0 downstream-maintained aggregation tables for launch-scope billing, quota, and dashboard use cases                              | By first production deployment; verified during the first 90 calendar days after launch               |
| Developer integration efficiency               | Platform developer can use SDK or REST examples to submit a valid usage record in a clean service integration                                                                                                | No shared Usage Collector integration guide or sample flow exists                                                       | First successful ingestion in ≤ 30 minutes for a developer familiar with platform auth and tenant concepts                       | Documentation and examples ready before production release candidate                                  |
| Operator support readiness                     | Platform operator can identify the owner-facing cause category for common failures: authn/authz denial, unregistered Metric, metadata limit rejection, storage-extension readiness, and query-latency breach | Troubleshooting depends on gear maintainer assistance and ad hoc log review                                           | ≥ 90% of sampled common failure cases resolved to a documented cause category without maintainer escalation                      | Runbook complete before production release candidate; sampled during each quarterly operations review |

### 1.4 Glossary

| Term                   | Definition                                                                                                                                                                                                                                                                                                                                                                                                          |
| ---------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Usage Record           | A single data point representing resource consumption by a tenant, with a numeric value and a timestamp, attributed to a registered Metric                                                                                                                                                                                                                                                                          |
| Metric                 | A registered, platform-global definition of something the Usage Collector measures; the Metric Kind (counter/gauge) and an optional unit label are product-level descriptors.                                                                                                                                                                                                                                       |
| Metric Kind            | Classifies a Metric by accumulation semantics — counter (non-negative deltas, signed-net SUM) or gauge (point-in-time).                                                                                                                                                                                                                                                                                             |
| GTS type id / gts_id   | The platform-typed identifier for a Metric; this PRD treats it as opaque.                                                                                                                                                                                                                                                                                                                                           |
| metadata_fields        | The closed, ordered list of metadata keys a Metric type accepts; per-record metadata is validated against this list at the gateway.                                                                                                                                                                                                                                                                                 |
| Declared property      | A metadata key declared on a Metric type's metadata key set.                                                                                                                                                                                                                                                                                                                                                        |
| Counter                | A Metric Kind representing a non-negative delta since the last report (e.g., API calls in this batch). Counter records support cumulative usage totals netted via `SUM` across `usage` and `compensation` entries.                                                                                                                                                                                                  |
| Gauge                  | A Metric Kind representing a point-in-time value that can go up or down (e.g., current memory usage in bytes). Stored as-is without monotonicity constraints.                                                                                                                                                                                                                                                       |
| Idempotency Key        | A client-provided identifier that makes at-least-once processing safe: an exact-equality re-submission under the same key is silently absorbed (no duplicate record), while a same-key submission whose content differs is surfaced as a conflict rather than silently dropped. The key is never reused for a different record (unbounded window).                                                                  |
| Usage Collector Plugin | A storage extension selected by operators to provide the persistence and query capability behind the Usage Collector                                                                                                                                                                                                                                                                                                |
| Record Metadata        | An optional, extensible JSON object attached to a usage record, allowing usage sources to include context-specific properties (e.g., LLM model name, token category, geographic region) that are opaque to the Usage Collector and interpreted by downstream consumers                                                                                                                                              |
| Deactivation           | An operator-initiated transition of an existing usage record's `status` from active to `inactive`. The record is retained for downstream reference and remains queryable but is distinguishable from active records by downstream consumers. Deactivation does not modify any other property of the record                                                                                                          |
| Compensation           | A counter-only correction primitive that partially reverses a previously reported usage record by SUM netting; defined in §5.6.                                                                                                                                                                                                                                                                                     |
| GTS                    | Global Type System — the platform type and identifier system used by registry/orchestration dependencies outside the Usage Collector PRD boundary                                                                                                                                                                                                                                                                   |
| PDP                    | Policy Decision Point — the platform authorization service that gates every operation in this PRD.                                                                                                                                                                                                                                                                                                                  |
| SecurityContext        | A platform-resolved structure carrying the authenticated caller's identity; supplied to the gear by the platform — never accepted from the payload.                                                                                                                                                                                                                                                               |
| Data Owner             | The role that owns the meaning of and decisions about usage data — tenant administrators own usage records attributed to their tenant; platform operators own the metric catalog. The Usage Collector does not own usage data                                                                                                                                                                                       |
| Data Custodian         | The role that holds and protects usage data on behalf of its owners but does not own it. The Usage Collector acts as custodian: it persists, isolates, and serves usage records under PDP-mediated authorization                                                                                                                                                                                                    |
| Audit Trail            | The combination of platform gateway access logs, platform authentication and PDP decision logs, and platform audit infrastructure that records authentication, authorization, ingestion, query, and operator-write outcomes for non-repudiation and forensic purposes. The Usage Collector contributes correlation identifiers to this trail but does not host its own audit log in v1 ([§4.2](#42-out-of-scope))   |
| PII                    | Personally identifiable information — any information relating to an identified or identifiable natural person. Within the Usage Collector boundary the gear handles only opaque platform identifiers; resolution of those identifiers to natural persons is owned by the platform identity layer ([§5.3](#53-attribution-isolation) Subject Attribution)                                                         |
| SPI                    | Service Provider Interface — the storage-plugin extension contract; distinct from the SDK trait and the REST API.                                                                                                                                                                                                                                                                                                   |
| RPO                    | Recovery Point Objective — the maximum acceptable amount of data loss measured in time after a failure. The Usage Collector does not define a gear-specific RPO; recovery is delegated to the platform DR posture and the active storage plugin ([§6.2](#62-nfr-exclusions) Gear-Specific Disaster Recovery exclusion)                                                                                          |
| RTO                    | Recovery Time Objective — the maximum acceptable duration between a failure and restoration of service. The Usage Collector does not define a gear-specific RTO; recovery is delegated to the platform DR posture and the active storage plugin ([§6.2](#62-nfr-exclusions) Gear-Specific Disaster Recovery exclusion)                                                                                          |
| OWASP ASVS             | OWASP Application Security Verification Standard — an open-source application security verification framework. The Usage Collector aligns with the platform's adopted ASVS baseline rather than asserting gear-specific ASVS certification (see `cpt-cf-usage-collector-fr-standards-compliance`)                                                                                                                 |
| DSR                    | Data Subject Rights — the rights granted to data subjects (access, rectification, erasure, restriction, portability, objection) by applicable data-protection regulations. DSR execution is owned by the platform identity, legal, and governance layers; the Usage Collector does not host a gear-local DSR workflow ([§6.2](#62-nfr-exclusions) Consent Management and Data Subject Rights workflows exclusion) |

## 2. Actors

### 2.1 Human Actors

#### Platform Operator

**ID**: `cpt-cf-usage-collector-actor-platform-operator`

- **Role**: Deploys and configures the usage collector gear, selects storage backend, monitors system health.
- **Needs**: Ability to choose and configure storage backends without code changes.

#### Platform Developer

**ID**: `cpt-cf-usage-collector-actor-platform-developer`

- **Role**: Integrates platform services with the Usage Collector using the SDK or API to emit usage data.
- **Needs**: Well-documented SDK for emitting usage data with minimal integration effort.

#### Tenant Administrator

**ID**: `cpt-cf-usage-collector-actor-tenant-admin`

- **Role**: Queries raw and aggregated usage data for their tenant.
- **Needs**: Access to raw and aggregated usage records filtered by type, subject, and resource for their tenant only, with time-range filtering.

### 2.2 System Actors

#### Usage Source

**ID**: `cpt-cf-usage-collector-actor-usage-source`

- **Role**: Any authenticated system that produces usage records.

#### Usage Consumer

**ID**: `cpt-cf-usage-collector-actor-usage-consumer`

- **Role**: Any system that queries aggregated usage data (e.g., billing system, quota enforcer, dashboard).

#### Storage Backend

**ID**: `cpt-cf-usage-collector-actor-storage-backend`

- **Role**: The underlying data store (e.g., ClickHouse or TimescaleDB) that persists usage records.

**Actor Permissions** (shared across human and system actors):

| Actor                                             | Permitted Operations                                                                                                                                                                                                                                                                                                                                                           | Denied by Default                                                                                                                                                                                                                        |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-actor-platform-operator`  | Deactivate individual records; create and delete Metrics                                                                                                                                                                                                                                                                                                                       | Querying or modifying records belonging to any tenant without an explicit security context                                                                                                                                               |
| `cpt-cf-usage-collector-actor-platform-developer` | Emit usage records for Metrics the source gear is PDP-authorized to emit, within the source gear's authorized tenant scope                                                                                                                                                                                                                                                 | Emitting records for Metrics outside the source gear's PDP-authorized set; attributing records to subjects or resources outside the authorized scope                                                                                   |
| `cpt-cf-usage-collector-actor-tenant-admin`       | Query aggregated and raw usage records scoped to their own tenant                                                                                                                                                                                                                                                                                                              | Accessing usage data of any other tenant; invoking operator-only operations (deactivation, Metric registration)                                                                                                                          |
| `cpt-cf-usage-collector-actor-usage-source`       | Emit usage records for registered Metrics; the scope of permitted target tenants, resources, source gears, and Metrics is enforced by the platform PDP at emit time — the caller must be PDP-authorized for the tenant supplied in the record (covering both same-tenant and parent→subtenant scenarios), the supplied resource and source gear, and the referenced Metric | Emitting records attributed to tenants, resources, or source gears outside the PDP-authorized scope; emitting records referencing Metrics outside the PDP-authorized set; emitting records referencing Metrics that are not registered |
| `cpt-cf-usage-collector-actor-usage-consumer`     | Query aggregated and raw usage data scoped to the authenticated tenant; subject to PDP constraint filters                                                                                                                                                                                                                                                                      | Accessing cross-tenant data; mutating usage records                                                                                                                                                                                      |
| `cpt-cf-usage-collector-actor-storage-backend`    | Receive and persist usage records forwarded by the gateway plugin; respond to query operations initiated by the plugin                                                                                                                                                                                                                                                         | Direct access from any actor other than the authorized plugin instance                                                                                                                                                                   |

Authorization is enforced via the platform PDP (`authz-resolver`) on all read and write operations. Unauthenticated requests are rejected before any authorization check. Failures result in immediate rejection with no partial operation (fail-closed).

## 3. Operational Concept & Environment

### 3.1 Gear-Specific Environment Constraints

The Usage Collector runs as a stateless, horizontally scaled gear fronted by the platform API gateway, with all durable state held in the operator-selected storage plugin. Production deployments operate 24/7; the gear exposes no business-hours-only operating mode. A single deployment serves the operator-selected region; cross-region replication is delegated to platform topology and the active storage plugin's deployment profile (cross-reference [§4.2](#42-out-of-scope) deferred Multi-Region Replication, `cpt-cf-usage-collector-fr-standards-compliance`). Concrete deployment, observability pipelines, and storage-tier HA configuration are governed by platform operations and the active plugin's deployment guide; the PRD records only product-level operating posture and the measurable thresholds in [§6](#6-non-functional-requirements).

## 4. Scope

### 4.1 In Scope

- Usage record ingestion from platform services
- Counter and gauge metric semantics
- Per-tenant usage attribution, PDP-authorized at emit time
- Per-subject (user, service account) usage attribution, PDP-authorized at emit time
- Per-resource usage attribution
- Ingestion authorization via the platform PDP
- Idempotency via client-provided keys
- Pluggable storage backend selection
- Query API for aggregated usage data with time-range filtering and grouping
- Tenant isolation on all read and write operations
- Per-record metadata constrained by the Metric's declared metadata key set
- Individual event deactivation with downstream visibility of active/inactive status
- Metric registration (create, delete)
- Caller authentication is performed by the platform gateway upstream of the gear
- Delegated audit trail through platform gateway access logs and platform audit infrastructure, with gear-emitted correlation identifiers on every API operation
- Custodianship of tenant usage data under PDP-mediated read and write boundaries, including tenant-owner, operator-steward, and gear-custodian role distinctions

### 4.2 Out of Scope

- **Business Logic**: Pricing, rating, billing rules, invoice generation, quota enforcement decisions — responsibility of downstream consumers
- **Multi-Region Replication**: Deferred to future phase
- **Retention Policy Management**: out of scope for v1 (no gear-level retention enforcement); the unbounded idempotency-key obligation is preserved (see §5.1).
- **Dedicated Backfill Capability**: out of scope for v1; bulk historical import rides the normal ingestion path.
- **Individual Event Amendment**: Operator-initiated property updates to existing usage records are out of scope for phase 1 of the gear; covered in a later phase
- **Audit Events**: Structured audit-event emission to the platform `audit_service` for operator-initiated writes is out of scope for phase 1 of the gear; covered in a later phase
- **Rate Limiting**: Per-source-gear and per-(source, tenant) ingestion quotas and rate-limit enforcement are out of scope for phase 1 of the gear; covered in a later phase
- **Watermark and Reconciliation Metadata**: Per-source-gear and per-tenant ingestion metadata (watermarks, event counts, latest event timestamps, ingestion statistics) and the corresponding metadata-exposure API are out of scope for phase 1 of the gear; covered in a later phase. External reconciliation workflows that depend on this metadata are out of scope for the gear entirely

## 5. Functional Requirements

### 5.1 Usage Ingestion

#### Usage Record Ingestion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-ingestion`

The system **MUST** accept usage records from authenticated usage sources. Each usage record represents a single measurement of resource consumption attributed to a tenant.

- **Rationale**: Centralizing usage ingestion ensures all downstream consumers operate on the same data.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Idempotent Ingestion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-idempotency`

The system **MUST** require a client-provided idempotency key on every usage record. The system **MUST** reject any record submitted without an idempotency key with an actionable error. When a record is submitted whose idempotency key matches a previously accepted record for the same tenant and Metric **and** every caller-supplied field is identical — value, timestamp, resource (resource_ref), subject (subject_ref), source_gear, and metadata — the system **MUST** silently deduplicate the submission (no error, no duplicate record); this is the exact-equality retry case. When the key matches a previously accepted record for the same tenant and Metric but **any** caller-supplied field differs from the stored record — including a metadata-only difference — the system **MUST** reject the submission with an actionable conflict error and **MUST NOT** silently drop the second write. The dedup boundary is per-tenant per-Metric: the same idempotency key may legitimately reappear under a different tenant or a different Metric without being treated as a duplicate. The idempotency window is **UNBOUNDED**: a key has no time-to-live, never expires, and is never intentionally reusable, so the per-tenant per-Metric uniqueness of an idempotency key is permanent.

- **Rationale**: Client-side retries on transient failures can produce duplicate submissions; deduplication prevents incorrect aggregations. For counter metrics, a retry of a keyless delta inflates the accumulated total without any means of detection or correction. For gauge metrics, duplicate readings can still poison downstream consumers that derive counts, distinct timestamps, or rate-of-change signals from raw records. Requiring an idempotency key on every emission eliminates this data integrity risk at the source gear, removes a kind-dependent special case from the ingestion contract, and lets source gears adopt a single retry pattern across all metrics they emit. Splitting the same-key outcome is deliberate: an exact-equality retry is the benign at-least-once case and remains safe to absorb silently, but a key reused with different content is a caller bug. Surfacing that divergence as a conflict rather than silently dropping the second write protects billing-correctness and other downstream consumers from data that would otherwise be lost without any signal, while an unbounded window guarantees a key can never be silently recycled into a different record.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Per-Record Extensible Metadata

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-record-metadata`

The system **MUST** support a closed-shape metadata model declared per metric type: every metadata key supplied on a usage record **MUST** be a member of the referenced Metric's declared metadata-key list. Undeclared metadata keys **MUST NOT** be accepted — the system rejects such records at the gateway with an actionable validation error. All metadata values are treated as strings on the wire and at rest. The system **MUST** enforce a configurable maximum metadata size and **MUST** reject records exceeding the configured limit with an actionable error.

The metadata surface is **closed**: there is no free-form remainder, no open-extras escape hatch, and no silently-preserved undeclared properties. Downstream consumers (billing, reporting, analytics) extract declared keys by name; the Usage Collector's query surface addresses the same declared keys.

- **Rationale**: Different usage sources need to attach context-specific properties to usage records (e.g., LLM model name, token type, request category, geographic region) that enable downstream reporting and analytics. A closed-shape model lets metric-type authors declare exactly the keys that matter, gives downstream consumers a stable contract they can address by name, and removes the open-extras attack surface (undeclared keys can no longer be smuggled into the store and silently preserved). String-only value typing keeps the gateway validation cheap — a declared-keys membership check — and aligns the v1 surface with the quota-reporting downstream consumer narrowing.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`

### 5.2 Metric Kinds

#### Counter Metric Kind

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-counter-semantics`

The system **MUST** enforce counter-kind semantics: source gears submit non-negative delta values representing consumption since their last report. The system **MUST** reject records for counter-kind Metrics with negative values. The system **MUST** accumulate submitted deltas into a persistent, signed-net cumulative `SUM` per (tenant, metric) tuple, which `cpt-cf-usage-collector-fr-usage-compensation` MAY reduce via append-only compensation entries.

- **Rationale**: Delta-based reporting decouples the source gear's internal state from the Usage Collector's persistent totals. Source gears never report cumulative values, so process restarts and counter resets in the source gear are transparent — a restart simply results in the next emission starting from zero again, which is valid.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Gauge Metric Kind

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-gauge-semantics`

The system **MUST** support gauge-kind Metrics representing point-in-time values. Records for gauge-kind Metrics **MUST** be stored as-is without monotonicity constraints or delta accumulation.

- **Rationale**: Gauges represent instantaneous measurements (e.g., current active connections, memory usage in bytes) that naturally fluctuate and have no meaningful cumulative total.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

### 5.3 Attribution & Isolation

#### Tenant Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-tenant-attribution`

1. The system **MUST** attribute every usage record to a tenant supplied by the caller in the request.
2. The system **MUST** authorize the caller's tenant attribution via the platform PDP before any record is accepted, verifying that the authenticated caller is permitted to emit records for the specified tenant. This covers both same-tenant emission and parent→subtenant scenarios (e.g., a platform-level metering agent collecting usage for resources owned by its subtenants).
3. The gateway **MUST** independently validate tenant attribution on ingest as a defense-in-depth check.

- **Rationale**: Requiring callers to supply the target tenant explicitly supports all emission scenarios — including remote forwarders and external systems that emit records on behalf of multiple tenants — through a single uniform path. PDP authorization remains the security boundary enforcing which tenants a given caller is permitted to report for.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Resource Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-resource-attribution`

Every usage record **MUST** be attributed to a specific resource instance within a tenant, identified by a resource ID and resource type. Resource attribution is mandatory; the system **MUST** reject records that omit either field.

- **Rationale**: Per-resource attribution enables granular billing, per-resource quota enforcement, and detailed usage analysis at the resource level. Mandatory attribution ensures downstream consumers always have a resource scope to aggregate and filter on, without needing to handle the absence of this field.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Subject Attribution

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-subject-attribution`

1. The system **MUST** support attributing usage records to a subject (user, service account, or other principal) within a tenant, identified by a caller-supplied subject ID and, when available, an optional subject type. Subject attribution is optional per usage record to accommodate system-level resource consumption not attributable to a specific subject (e.g., background jobs where per-user attribution is not meaningful); when subject attribution is supplied, the subject ID **MUST** be present, the subject type **MAY** be omitted for systems without subject-type taxonomies, and a subject type **MUST NOT** be supplied without a subject ID.
2. When a subject is supplied, the system **MUST** authorize the caller's subject attribution via the platform PDP before any record is accepted, verifying that the authenticated caller is permitted to emit records attributed to the specified subject ID and, when supplied, subject type. When no subject ID is supplied, PDP subject validation is skipped.
3. The system **MUST NOT** derive subject identity from the caller's SecurityContext: subject attribution is always caller-supplied, never implicitly populated from the authenticated principal.

- **Rationale**: Per-subject attribution enables chargeback, per-subject quota enforcement, and visibility into which principals drive consumption within a tenant. Accepting the target subject explicitly from the caller — rather than implicitly from the caller's own SecurityContext — supports emission scenarios where the calling service attributes consumption to subjects other than itself (e.g., a service emitting per-user records on behalf of the users it serves, or a remote forwarder relaying records originally produced by multiple named subjects). PDP authorization remains the security boundary enforcing which subjects a given caller is permitted to report for, preventing spoofing.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`
- **Data Classification**: Subject IDs are opaque platform identifiers; PII handling is owned by the platform identity layer (see [§6.2](#62-nfr-exclusions) NFR Exclusions).

#### Tenant Isolation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-tenant-isolation`

The system **MUST NOT** grant any caller access to a tenant's usage data — for reads or writes — without an explicit PDP authorization for that tenant. The system **MUST** treat every tenant scope independently: no caller is implicitly authorized for any tenant, and authorization for one tenant **MUST NOT** be inferred from authorization for another (sibling, parent, or child). Cross-tenant access is permitted only when the PDP explicitly authorizes the authenticated caller for the target tenant (e.g., a parent tenant administrator authorized to read its subtenants' usage). The system **MUST** fail closed on authorization failures.

- **Rationale**: Tenant data isolation is a security and compliance requirement, but parent→subtenant hierarchies and platform-level administrative roles legitimately require cross-tenant visibility. Anchoring isolation on PDP authorization keeps the security boundary precise while supporting the hierarchical scenarios the platform exposes (see `cpt-cf-usage-collector-fr-tenant-attribution`, `cpt-cf-usage-collector-fr-ingestion-authorization`).
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-usage-consumer`

#### Ingestion Authorization

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-ingestion-authorization`

1. The system **MUST** authorize each usage record emission before it is persisted. The security boundary for ingestion authorization is the PDP check on the caller's authenticated identity against the supplied tenant, resource, source gear, and referenced Metric.
2. The system **MUST** verify the caller is permitted to emit records attributed to the specified tenant, resource, source gear, and Metric, before any record is accepted.
3. The system **MUST** validate that the referenced Metric is registered, rejecting records that reference an unknown Metric.
4. Authorization failures **MUST** be surfaced immediately to the caller before any domain operation is committed.
5. The system **MUST** fail closed: unauthorized records are never persisted, and there is no silent discard of denied emissions.

- **Rationale**: Anchoring authorization on the authenticated caller plus the full attribution tuple (tenant, resource, source gear, Metric) lets the PDP enforce per-caller emission scope without trusting any caller-supplied attribution. Metric existence validation preserves data quality by ensuring records reference known Metrics.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

### 5.4 Pluggable Storage

#### Pluggable Storage Backend

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-pluggable-storage`

The system **MUST** support pluggable storage backends. Operators **MUST** be able to select the active backend without changing Usage Collector product behavior.

**Scope**: Pluggable storage covers both **usage records** (ingestion, query, deactivation, compensation) and the **metric catalog**. The metric catalog is the sole catalog and is reached through the storage plugin; details in DESIGN.

- **Rationale**: Pluggable storage avoids lock-in and allows operators to choose the backend that fits their needs. Co-locating catalog rows and usage rows on the same plugin-owned backend lets the deletion path enforce referential integrity natively instead of relying on cross-store coordination, and keeps the storage plugin the single seam through which the gear reaches durable state.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-storage-backend`

### 5.5 Usage Query & Aggregation

#### Aggregated Usage Query

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-query-aggregation`

The system **MUST** provide an API for querying aggregated usage data. Queries **MUST** support time-bounded aggregation for exactly one Metric and **SHOULD** allow consumers to narrow and group results by tenant, subject, resource, source gear, and time period where authorized. The supported aggregation operations and wire-level filters are defined in DESIGN.md and the OpenAPI contract.

The system **MUST** reject aggregation requests that omit the metric filter or supply more than one metric value, with an actionable error.

The system **MUST** authorize each query via the platform PDP. PDP-returned constraints define the authorization boundary and **MUST** be applied as query filters before execution. User-supplied filters (including `tenant`) **MUST** be applied in addition to PDP-returned constraints — they can only further narrow the result set, never widen it beyond the PDP-authorized scope. The system **MUST** fail closed on authorization failures (PDP denial or empty constraints).

- **Rationale**: Downstream consumers (billing, dashboards) need aggregated views without fetching and processing raw records. Restricting each aggregation to a single Metric ensures the aggregated values share consistent semantics and units — combining counts, byte volumes, or duration measures across different Metrics is meaningless and would mask data-quality issues. Product-level filtering and grouping still enable rich breakdowns within a Metric while preserving PDP-authorized scope.
- **Actors**: `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-tenant-admin`

#### Raw Usage Query

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-query-raw`

The system **MUST** provide an API for querying raw usage records as paged results. Queries **MUST** support a mandatory time range and **SHOULD** allow consumers to narrow results by tenant, Metric, subject, and resource where authorized. Paging mechanics and wire-level filter details are defined in DESIGN.md and the OpenAPI contract.

The system **MUST** authorize each query via the platform PDP using the same decision and constraint-enforcement model as the aggregation query path: PDP-returned constraints define the authorization boundary, and user-supplied filters (including `tenant`) only further narrow the result set within that scope. The system **MUST** fail closed on authorization failures.

- **Rationale**: Some consumers need access to individual records for auditing, debugging, or dispute resolution.
- **Actors**: `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-tenant-admin`

### 5.6 Corrections (Event Deactivation & Usage Compensation)

The Usage Collector exposes two complementary correction primitives: **event deactivation** is cross-kind whole-row error retraction (any entry, operator-only, one-way `active → inactive` latch), and **usage compensation** is counter-only append-only value-reversal (source-gear-emitted on the ingestion path, with a strictly-negative value referencing the original entry). The two are disjoint by purpose and aggregation contract: deactivation removes a row from every aggregation; compensation reduces the netted `SUM` only and leaves `COUNT` / `MIN` / `MAX` / `AVG` untouched.

#### Individual Event Deactivation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-event-deactivation`

The system **MUST** support deactivating individual usage events by transitioning the event's `status` from active to `inactive` while retaining the event for downstream reference. Deactivation **MUST NOT** modify any property of the record other than `status`. Downstream consumers **MUST** be able to distinguish active from inactive records when querying, and inactive records **MUST** remain queryable.

Deactivation is one-way: the Usage Collector does not provide a reactivation operation. The system **MUST** reject deactivation requests targeting an already-inactive record with an actionable error.

Deactivation applies uniformly to any entry — both usage rows and compensation rows can be deactivated through the same operation. Deactivation of a usage row with one or more active compensations referencing it triggers a **depth-1 cascade** to those compensations, flipping them to `inactive` in the same one-way step, so the net `SUM` returns to the state it held before either the usage record or its compensations were accepted. The cascade is strictly depth-1 by construction (a compensation row cannot itself be compensated; see `cpt-cf-usage-collector-fr-usage-compensation`).

- **Rationale**: Deactivation retires a record from downstream consumption without losing its history, letting storage plugins, query consumers, and aggregation pipelines reason about active/inactive transitions as a first-class lifecycle event. Making deactivation one-way keeps each record's lifecycle monotonic. Cascading to active compensations preserves the post-correction `SUM` invariant without forcing a second operator action.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Usage Compensation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-usage-compensation`

The system **MUST** accept counter-only, append-only **compensation** entries that partially reverse a previously reported usage value through `SUM` netting, without rewriting or deactivating the original row. A compensation entry is submitted via the **same ingestion path** used for usage records (no dedicated compensate endpoint, SDK method, or storage-plugin call exists), is attributed via the platform PDP on the caller's identity, and is protected by the existing mandatory idempotency key (cross-reference `cpt-cf-usage-collector-fr-idempotency`).

The system **MUST** enforce the following invariants at ingestion before persistence:

- **Counter-only**: a compensation entry referencing a `gauge`-kind Metric **MUST** be rejected with an actionable error. Compensation is defined only for `counter`-kind Metrics; the only correction available for a `gauge` Metric is deactivation (cross-reference `cpt-cf-usage-collector-fr-event-deactivation`).
- **Strictly negative value**: a compensation entry on a `counter`-kind Metric **MUST** carry a value strictly less than zero; zero and positive values are rejected with an actionable error.
- **Valid reference to the original entry (ingestion-time)**: every compensation entry **MUST** reference an existing usage entry that shares its tenant and Metric and is currently `active`. Any failure is rejected with an actionable error. The "must be active" check is the concurrency boundary: a compensation referencing a row that is concurrently being deactivated is rejected by this check, without distributed coordination.
- **Aggregation effect**: a compensation entry **MUST** affect `SUM` only — `SUM(value)` over `active` rows nets usage and compensation signed values. `COUNT`, `MIN`, `MAX`, and `AVG` **MUST** operate over usage entries only ("compensation entries adjust SUM; they are not events").
- **Cascade on deactivation**: when a usage row that has one or more active compensations referencing it is deactivated, the system **MUST** apply the depth-1 cascade defined in `cpt-cf-usage-collector-fr-event-deactivation`, flipping the referencing compensation rows to `inactive` in the same one-way step.

The system **MUST NOT** support compensating a compensation row. The system **MUST NOT** validate non-negative net `SUM` and **MUST NOT** emit negative-net detection, alerts, or downstream reconciliation; per-record outstanding balances and lot / FIFO-LIFO tracking are explicit non-goals.

- **Rationale**: Counter value-reversal is a distinct concern from whole-row retraction. Compensation supports partial give-backs (capacity refunds, partial revocations) without rewriting the original usage row, preserving the append-only invariant and the audit history. Routing compensation through the same ingestion path as usage reuses the existing PDP attribution and idempotency machinery, keeps the public contract surface stable, and yields netting deterministically through `SUM` without any business-logic computation inside the metering substrate.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`
- **Depends on**: `cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-idempotency`, `cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-event-deactivation`, `cpt-cf-usage-collector-fr-ingestion-authorization`

### 5.7 Metrics

Metrics are platform-global definitions: a Metric exists once for the whole deployment and is referenced by any tenant's usage records. Metrics are not scoped to or owned by tenants.

#### Metric Existence and Kind Enforcement

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-metric-existence-and-kind`

The system **MUST** reject any usage record that references an unregistered Metric. The system **MUST** enforce kind-dependent invariants based on the referenced Metric's Metric Kind — in particular, records for counter-kind Metrics with negative delta values **MUST** be rejected (cross-reference [§5.2](#52-metric-kinds) `cpt-cf-usage-collector-fr-counter-semantics`). Metric Kind is derived from the registered metric type. Rejections **MUST** be returned to the caller immediately with an actionable error before any record is accepted for delivery.

A Metric is identified by a platform Metric identifier and described by a Metric Kind and an optional unit label. Beyond the kind-dependent invariants and the closed metadata-key list declared on the metric type at registration (see `cpt-cf-usage-collector-fr-metric-registration`), the Usage Collector **MUST NOT** require per-record schemas beyond the metric type's declared metadata keys.

- **Rationale**: Restricting validation to existence and kind invariants keeps Metric registration lightweight while preserving the data-integrity guarantees that matter: records that reference unknown Metrics cannot enter the store, and counter accumulation cannot be poisoned by negative deltas. Per-record metadata ([§5.1](#51-usage-ingestion) `cpt-cf-usage-collector-fr-record-metadata`) is closed-shape: every metadata key on a usage record **MUST** be a member of the metric type's declared keys, undeclared keys are rejected at the gateway, and all values are treated as strings end-to-end. Deriving kind from the registered metric type collapses two registration-time invariants into one and removes a class of "kind disagrees with identifier" inconsistency bugs.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`

#### Metric Registration

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-metric-registration`

The system **MUST** allow platform operators to register a new Metric via API without code changes or service redeployment. A registration request specifies the Metric identifier and the closed metadata-key list — an array of strings naming every metadata key the Metric will accept on usage records (per [§5.1](#51-usage-ingestion) `cpt-cf-usage-collector-fr-record-metadata`). Metric Kind is derived from the registered metric type; at register time the system **MUST** validate the identifier and the derived kind, and **MUST** reject malformed or unknown-kind identifiers with an actionable validation error. The derived kind governs the kind-dependent invariants in `cpt-cf-usage-collector-fr-counter-semantics` / `cpt-cf-usage-collector-fr-gauge-semantics`. Registered Metrics become immediately available for ingestion across all tenants.

Primary use cases: AI/LLM token metering (input/output tokens, custom credit units), compute metering (vCPU-hours, GPU-hours), API request metering (calls by tenant and endpoint), storage metering (GB-hours across tiers), and network transfer (bytes ingress/egress).

The Metric identifier **MUST** be unique across the deployment; duplicate registration requests **MUST** be rejected with an actionable error. Registration **MUST** be authorized by the platform PDP against the caller's identity; unauthorized requests are rejected before any change is made.

When a Metric is registered, the platform operator **MUST** also configure the PDP authorization policies that declare which source gears are permitted to emit records referencing this Metric, and for which tenants. The Usage Collector does not store this authorization mapping internally — it is owned by the PDP.

Registration is available on both the REST and in-process SDK surfaces; surface details are in DESIGN.

- **Rationale**: New resource types (AI tokens, GPU-hours, custom credit units) must be meterable without service redeployment. Declaring a closed metadata-key list on the metric type lets the gateway validate declared per-record keys at ingest with a cheap membership check while giving downstream consumers a stable, addressable contract — and removes the open-extras attack surface (undeclared keys are validation errors rather than silently-preserved extras). Deriving Metric Kind from the registered metric type collapses kind and identifier into a single invariant, eliminating a class of "kind disagrees with identifier" registration bugs. Pushing source-gear-to-Metric authorization into PDP avoids duplicating policy data; exposing the same operation on the SDK in addition to REST lets in-process callers register Metrics without round-tripping the REST surface.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`

#### Metric Deletion

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-metric-deletion`

The system **MUST** allow platform operators to delete a registered Metric via API. Deletion **MUST** be authorized by the platform PDP against the caller's identity.

Deletion is **referential**: the system **MUST** reject deletion of a Metric whose type is referenced by any existing usage row, returning a deterministic, structured "metric referenced" error to the caller. Referential delete protection on the metric catalog is enforced by the storage plugin so the rejection is atomic with the delete attempt and does not depend on cross-store coordination (mechanics in DESIGN).

After a successful (i.e., unreferenced) deletion, the Metric identifier becomes available for re-registration. Any subsequent ingestion attempt referencing the deleted Metric is rejected by `cpt-cf-usage-collector-fr-metric-existence-and-kind` until the Metric is re-registered.

Deletion is available on both the REST and in-process SDK surfaces; surface details are in DESIGN.

- **Rationale**: Referential delete eliminates the orphaned-attribution failure mode that an unconditional delete leaves behind, and enforcing it natively at the storage layer (rather than an application-level guard) makes the constraint atomic with the delete and survives any future caller bypassing the gateway. Exposing the operation on the SDK in addition to REST keeps the two surfaces convergent on a single domain service over the plugin-owned catalog.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`

### 5.8 Security and Data Governance

The Usage Collector is a custodian of tenant usage data; ownership, identity, audit, and lifecycle controls are anchored on platform services and operator-selected dependencies. The requirements below state PRD-level guarantees and delegations; implementation mechanics are defined in DESIGN.md and the platform services themselves.

#### Authentication Delegation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-authn-delegation`

The Usage Collector **MUST** accept only SecurityContext values resolved by the platform upstream of the gear — populated by the platform gateway on the REST surface and supplied directly by the caller on the in-process SDK surface. The Usage Collector **MUST NOT** implement, store, validate, or refresh caller credentials, **MUST NOT** consume any credential-resolution contract, and **MUST NOT** synthesize, anonymize, or downgrade an identity when the upstream authentication boundary is unavailable; requests arriving without a SecurityContext (or with a SecurityContext whose subject is unresolved) **MUST** be rejected by every entry point before any business processing. Authentication primitives — MFA, SSO/federation, session management, and credential issuance/rotation/revocation/complexity policies — are owned exclusively by the platform identity layer; the Usage Collector's responsibility ends at validating that an inbound SecurityContext is present. The `correlation_id` carried on the inbound SecurityContext **MUST** be propagated unchanged to every downstream PDP call, storage-plugin dispatch, and structured operational event so the platform audit trail can reconcile gear activity end-to-end.

- **Rationale**: The collector is a metering substrate downstream of the platform gateway; authentication is the gateway's responsibility. Stating the requirement as "accept only resolved SecurityContexts; reject otherwise" lets the same posture compose uniformly across gears with no gear-local credential surface.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-tenant-admin`

#### Data Classification

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-data-classification`

The system **MUST** treat its persisted data using the following PRD-level classification:

- **Opaque platform identifiers** (tenant ID, subject ID, resource ID, source-gear identifier, Metric identifier) — internal platform references issued by the platform identity, tenancy, and registry layers. The Usage Collector **MUST NOT** interpret, decode, or correlate these identifiers to natural persons; PII management for identifiers belongs to the platform identity layer.
- **Operational telemetry** (usage record value, timestamp, idempotency key, deactivation status) — non-personal metering data describing resource consumption events.
- **Caller-supplied metadata** (the optional per-record metadata object) — opaque to the Usage Collector. Source gears **MUST NOT** place PII, payment data, regulated health data, or credentials into metadata; this prohibition is a product-level contract on usage sources and is reiterated to integrators in the API documentation referenced by `cpt-cf-usage-collector-nfr-documentation-coverage`.

- **Rationale**: Explicit classification makes the data the gear holds inspectable at PRD level and bounds Privacy by Design, regulatory, and residency obligations to delegations on the platform layer and the operator-selected plugin. Excluding PII at the gear boundary preserves the "platform-identity-owns-PII" constraint that DESIGN already relies on.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`, `cpt-cf-usage-collector-actor-platform-operator`

#### Audit Trail Delegation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-audit-trail`

The system **MUST** rely on the platform gateway access log, the platform PDP decision logs, and the platform audit infrastructure as the authoritative audit trail for authentication (owned upstream by the platform gateway), authorization, ingestion, query, deactivation, and Metric-lifecycle operations. The Usage Collector **MUST** read the `correlation_id` carried on every inbound platform-resolved SecurityContext and propagate it unchanged to every PDP call, storage-plugin dispatch, structured operational event, and outbound response so platform-level access and audit records can be reconciled with gear-level activity. The Usage Collector **MUST NOT** synthesize, rewrite, or omit the `correlation_id` and **MUST NOT** invent or maintain a parallel gear-local audit log in v1; gear-emitted audit events for operator writes are explicitly deferred ([§4.2](#42-out-of-scope)).

- **Rationale**: The platform owns the audit pipeline end-to-end (gateway, PDP, audit-store). The collector's contribution is to accept the SecurityContext (which already carries the `correlation_id`) from upstream and propagate it forward — no gear-local identity handling and no gear-local audit log.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-usage-consumer`

#### Non-Repudiation

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-non-repudiation`

Every accepted ingestion, query, deactivation, and Metric-lifecycle operation **MUST** be attributable to the caller identity carried on the inbound platform-resolved SecurityContext (subject id, tenant scope claims, and the `correlation_id`) and recorded in the platform audit trail referenced by `cpt-cf-usage-collector-fr-audit-trail`. The Usage Collector **MUST NOT** accept operations whose inbound SecurityContext is absent, anonymous, synthesized, or otherwise produced inside the gear (cross-reference `cpt-cf-usage-collector-fr-authn-delegation`), and **MUST NOT** drop the caller-identity binding between gateway acceptance, PDP authorization, plugin dispatch, persistence, or operator-write completion. The `correlation_id` carried on the inbound SecurityContext **MUST** be propagated unchanged to every PDP call, storage-plugin dispatch, and structured operational event so the platform audit pipeline can reconcile every gear action to its originating caller without duplicating signing or replay defenses inside the metering substrate.

- **Rationale**: Non-repudiation rests on the platform identity layer (which is upstream of the collector); the collector's contribution is to refuse anonymous/synthesized inputs and to propagate the inbound SecurityContext and `correlation_id` end-to-end.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-platform-operator`

#### Privacy by Design Application

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-privacy-controls`

The system **MUST** apply Privacy by Design principles at PRD level as follows:

- **Data minimization** — the Usage Collector **MUST** persist only the attribution tuple, value, timestamp, idempotency key, deactivation status, and optional opaque metadata required for metering; no additional caller, identity, or content fields are introduced.
- **Purpose limitation** — usage data **MUST** be used only for metering and for downstream consumer queries; business decisions (pricing, rating, invoicing, quota decisions) are out of scope and remain the responsibility of downstream consumers (cross-reference [§4.2](#42-out-of-scope)).
- **Storage limitation** — physical retention, archival, and purging are delegated to the operator-selected storage plugin's deployment profile (cross-reference `cpt-cf-usage-collector-fr-data-lifecycle`). The Usage Collector itself does not impose unbounded retention as a product property; the unbounded idempotency window of `cpt-cf-usage-collector-fr-idempotency` is a separate dedup-key-preservation obligation, not a record-body retention window.
- **Privacy by default** — the gear exposes no end-user UI, no per-subject profile views, and no default broad-access paths; every read and write **MUST** pass PDP authorization with the default of deny.
- **Pseudonymization** — subject, tenant, resource, and source-gear identifiers stored by the Usage Collector **MUST** remain opaque platform identifiers; the gear **MUST NOT** introduce identifier-to-person resolution paths.

- **Rationale**: Stating Privacy by Design as positive obligations closes the checklist at PRD level and makes the boundary with the platform identity layer explicit, rather than relying solely on a single "not applicable" exclusion. This requirement intentionally does not assert standalone GDPR Article 25 conformance — that conformance is governed at the platform level (cross-reference `cpt-cf-usage-collector-fr-standards-compliance` and [§6.2](#62-nfr-exclusions)).
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`, `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-tenant-admin`

#### Data Ownership and Stewardship

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-data-ownership`

The system **MUST** apply the following ownership and stewardship model at PRD level:

- **Data owner of usage records** — the tenant administrator (`cpt-cf-usage-collector-actor-tenant-admin`) for usage records attributed to their tenant.
- **Data steward of the Metric catalog and storage-plugin selection** — the platform operator (`cpt-cf-usage-collector-actor-platform-operator`).
- **Catalog ownership split** — the Usage Collector **owns the metric catalog semantically** (registration API, PDP, validation, schema authority); the active storage plugin **stores it physically** alongside usage records so referential integrity between the two is enforced natively at the storage layer.
- **Data custodian of all persisted records** — the Usage Collector gear itself. The Usage Collector **MUST NOT** assert ownership of tenant usage data and **MUST NOT** authorize cross-tenant access without an explicit PDP decision (cross-reference `cpt-cf-usage-collector-fr-tenant-isolation`).
- **Data sharing** — usage data **MUST** be shared with downstream consumers only through the public read surfaces (`cpt-cf-usage-collector-interface-rest-api`, `cpt-cf-usage-collector-interface-sdk-client`) and only within the PDP-authorized scope.
- **Third-party data usage** — third-party systems consuming usage data **MUST** access it as `cpt-cf-usage-collector-actor-usage-consumer` callers authenticated by the ToolKit gateway upstream of the collector and authorized by the platform PDP; there is no out-of-band export or bulk-extract path provided by the gear.
- **User-generated content** — caller-supplied per-record metadata remains under the data owner's tenant scope and is not exposed outside the PDP-authorized read paths.

- **Rationale**: Recording ownership, stewardship, and custodianship explicitly makes accountability auditable and resolves "who decides about this data" without inventing a new approval workflow inside the gear. Limiting sharing and third-party access to the public read surfaces preserves the security boundary established by PDP authorization.
- **Actors**: `cpt-cf-usage-collector-actor-tenant-admin`, `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-consumer`

#### Data Quality Preservation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-fr-data-quality`

The system **MUST** preserve data quality at ingestion through the following product-level guarantees:

- **Accuracy** — kind-invariant enforcement (cross-reference `cpt-cf-usage-collector-fr-counter-semantics`, `cpt-cf-usage-collector-fr-metric-existence-and-kind`) and idempotency (cross-reference `cpt-cf-usage-collector-fr-idempotency`) prevent negative-delta poisoning and duplicate accumulation.
- **Completeness** — mandatory attribution (cross-reference `cpt-cf-usage-collector-fr-tenant-attribution`, `cpt-cf-usage-collector-fr-resource-attribution`) ensures every persisted record carries the fields downstream consumers depend on.
- **Freshness** — freshness has two distinct halves and **MUST NOT** be conflated. (a) **Ingestion ack latency** — the synchronous ingestion path returns `Acknowledged` within the bound declared by `cpt-cf-usage-collector-nfr-ingestion-latency`; the gear exposes no parallel ingestion path that could lag this guarantee. (b) **Queryability freshness** — visibility of the ack'd record through the raw and aggregated query surfaces is governed by `cpt-cf-usage-collector-nfr-query-freshness` and is **plugin-bound, with no upper bound at the gear floor**; consumers **MUST NOT** assume read-your-writes against the query surfaces and **MUST** use the ingestion ack for any same-request outcome (admission control, post-emit summary, immediate-readback dashboards).
- **Validation** — structural attribution and metadata-size validation are performed at the gateway and ingestion-gateway boundary before persistence; invalid records are rejected with actionable errors (cross-reference [§5.1](#51-usage-ingestion), [§5.3](#53-attribution-isolation), `cpt-cf-usage-collector-fr-record-metadata`).
- **Cleansing** — once accepted, raw usage records **MUST NOT** be silently amended by the gear. Corrections are expressed through the two complementary correction primitives recorded in [§5.6](#56-corrections-event-deactivation--usage-compensation): (a) **event deactivation** (`cpt-cf-usage-collector-fr-event-deactivation`) — a cross-kind whole-row retraction applicable to any entry, optionally followed by a fresh idempotency-keyed re-emission; and (b) **usage compensation** (`cpt-cf-usage-collector-fr-usage-compensation`) — counter-only append-only negative entries that partially reverse a previously reported counter usage value through `SUM` netting. Dedicated backfill capability and full-record amendment remain [§4.2](#42-out-of-scope) out of scope.

- **Rationale**: Treating accuracy, completeness, freshness, validation, and cleansing as preservation properties of the ingestion contract (rather than separate background processes) keeps data quality verifiable through existing FRs and avoids introducing mutable-record semantics that would break downstream determinism.
- **Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-usage-consumer`

#### Data Lifecycle Delegation

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-data-lifecycle`

The system **MUST** delegate physical data lifecycle to the active storage plugin's deployment profile:

- **Retention and archival** — the operator-selected storage plugin owns retention policy, archival tiering, and any time-window enforcement; the Usage Collector itself does not impose retention windows at gear level (cross-reference [§4.2](#42-out-of-scope) deferred Retention Policy Management). The unbounded idempotency window of `cpt-cf-usage-collector-fr-idempotency` is **distinct** from data retention: it constrains dedup-key reuse, not record-body lifetime. A storage plugin **MUST** permanently preserve the dedup identity of every accepted record (per tenant, Metric, and idempotency key) even when the record body is later purged or archived under retention — retention may reclaim record bodies, but it **MUST NOT** free a dedup key or otherwise make a previously used idempotency key reusable.
- **Purging** — record purging policy, including legal-hold and right-to-erasure execution where applicable to the platform, is delegated to the platform legal/governance layer and the active plugin's purge mechanism; the Usage Collector does not provide a gear-local purge API in v1. Purging or archiving a record body **MUST NOT** release its dedup key tuple: a subsequent same-key submission is still evaluated against the unbounded idempotency window.
- **Migration** — storage-plugin migration (e.g., backend swap, version upgrade) is governed by `cpt-cf-usage-collector-nfr-plugin-contract-stability` and the platform's plugin release process; the Usage Collector preserves the public REST and SDK contracts across plugin migrations within a major version.
- **Historical access** — historical usage records remain queryable through the existing raw and aggregated query surfaces (`cpt-cf-usage-collector-fr-query-raw`, `cpt-cf-usage-collector-fr-query-aggregation`) for the retention window provided by the active plugin; inactive records continue to be queryable per `cpt-cf-usage-collector-fr-event-deactivation`.

- **Rationale**: Lifecycle policy depends on plugin capability and platform governance posture; pinning it inside the Usage Collector would freeze retention to a single backend's profile. Delegating to the plugin and platform layer preserves operator choice while still expressing the product-level commitments downstream consumers depend on (historical query, plugin-contract continuity, no surprise purges driven by the gear). The plugin still owns retention, but the unbounded idempotency window is a separate, permanent obligation: keeping the dedup key tuple after a body is purged is what prevents a reclaimed key from silently re-accepting a divergent record, so dedup correctness survives any retention policy the operator selects.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-storage-backend`, `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-tenant-admin`

#### Standards, Legal, and Compliance Applicability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-fr-standards-compliance`

The system **MUST** declare standards, legal, and compliance applicability at PRD level as follows:

- **Industry security standards** — security and authorization controls **MUST** be aligned with the platform's adopted security baseline (e.g., OWASP Application Security Verification Standard (ASVS) at the platform service level); gear-specific certifications are delegated to platform-level governance and are not asserted as standalone gear obligations.
- **Interoperability standards** — the REST surface **MUST** conform to OpenAPI 3 (wire contract authored in `usage-collector-v1.yaml`, sibling to DESIGN.md; DESIGN §3.3 Endpoints Overview together with [§7.1](#71-public-api-surface) of this PRD remain the prose summary of the public endpoint surface, while the yaml is authoritative for wire schemas and the canonical error envelope shape; the per-endpoint stability column in the DESIGN §3.3 Endpoints Overview governs endpoint-level stability for v1 and the major-version stability contract is declared in the yaml info description); the in-process SDK and Plugin Service Provider Interface (SPI) signatures follow the platform gear-binding contract (see DESIGN.md for the language binding and trait/interface shape); HTTP semantics follow the platform API gateway contract.
- **Regulatory obligations** — none are asserted as standalone gear obligations. The gear handles no payment card data (PCI DSS not applicable), no protected health data (HIPAA not applicable), and no financial-reporting source data (SOX not applicable). General data-protection regulation conformance is governed at platform level; the gear satisfies its boundary through `cpt-cf-usage-collector-fr-privacy-controls` and `cpt-cf-usage-collector-fr-data-classification` (cross-reference [§6.2](#62-nfr-exclusions)).
- **Legal duties** — terms of service, privacy policy, consent management, and data-subject-rights handling are delegated to the platform identity, legal, and governance layers; the Usage Collector does not host or enforce these documents and does not provide a gear-local consent or Data Subject Rights (DSR) workflow. Contractual obligations for tenant-owned usage data follow the platform tenant agreement.
- **Data sovereignty and residency** — data residency, cross-border transfer restrictions, and replication topology are delegated to the platform deployment topology and the operator-selected storage plugin's deployment profile; the Usage Collector itself is residency-agnostic and does not introduce additional cross-region paths beyond what the active plugin provides (cross-reference [§4.2](#42-out-of-scope) deferred Multi-Region Replication).
- **Reporting obligations** — compliance reporting consumes the platform audit trail (`cpt-cf-usage-collector-fr-audit-trail`); the Usage Collector contributes correlation identifiers but does not generate gear-local compliance reports.

- **Rationale**: Pinning every standard, legal duty, and regulatory framework to its actual governing layer (platform identity, platform legal, operator-selected plugin) keeps the gear's boundary clear, avoids overclaiming certification at gear level, and prevents drift between the PRD and the surfaces that actually carry these obligations.
- **Actors**: `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-tenant-admin`

## 6. Non-Functional Requirements

### 6.1 Gear-Specific NFRs

#### Query Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-query-latency`

Aggregation queries over a 30-day range for a single tenant **MUST** complete within 500ms at p95 under the load envelope defined by `cpt-cf-usage-collector-nfr-throughput-profile` (sustained ≥ 10,000 records/sec ingestion, ≥ 100 concurrent aggregation queries, no active burst in progress), measured over a ≥ 30-minute steady-state window.

- **Threshold**: p95 ≤ 500ms over a ≥ 30-minute steady-state window inside the `cpt-cf-usage-collector-nfr-throughput-profile` envelope; permitted measurement tolerance ±10% (i.e., p95 ≤ 550ms accepted for any single steady-state window) provided the 30-minute trailing trend stays at or below 500ms.
- **Rationale**: Interactive dashboard and billing queries need timely responses. Anchoring on the throughput profile and a measurement tolerance removes the ambiguity in the prior wording and makes the criterion repeatable.
- **Architecture Allocation**: See DESIGN.md

#### High Availability

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-availability`

The system **MUST** maintain 99.95% monthly availability for usage ingestion endpoints.

- **Threshold**: 99.95% uptime per calendar month
- **Rationale**: Usage collection is on the critical path for all billable operations.
- **Architecture Allocation**: See DESIGN.md

#### Ingestion Throughput

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-throughput`

The system **MUST** sustain ingestion of at least 10,000 usage records per second under the steady-state load envelope defined by `cpt-cf-usage-collector-nfr-throughput-profile` (sustained ≥ 10,000 records/sec; concurrent aggregation queries ≤ 100; no active burst in progress; measurement window ≥ 30 minutes of steady-state operation; sample-mean and p95 reported separately).

- **Threshold**: ≥ 10,000 records/sec sustained sample-mean over a ≥ 30-minute steady-state measurement window; instantaneous 1-minute sample-mean tolerance ≥ 0.95 × sustained rate (i.e., ≥ 9,500 records/sec for any 1-minute sample inside the steady-state window).
- **Rationale**: High-volume services (LLM Gateway, API Gateway) generate significant event throughput; the ingestion path must not become a bottleneck. Anchoring on the throughput profile removes the ambiguity in "normal operation" by pinning the test condition to the sustained, burst, and concurrent-query envelope defined in `cpt-cf-usage-collector-nfr-throughput-profile`.
- **Architecture Allocation**: See DESIGN.md

#### Ingestion Latency

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-ingestion-latency`

The system **MUST** complete usage record ingestion within 200ms at p95 under the load envelope defined by `cpt-cf-usage-collector-nfr-throughput-profile` (sustained ≥ 10,000 records/sec, burst ≤ 30,000 records/sec for ≤ 5 minutes per 60-minute window, ≥ 100 concurrent aggregation queries, ≥ 700,000,000 accepted calls per 24-hour day), measured at the platform gateway over a ≥ 30-minute steady-state window.

- **Threshold**: p95 ≤ 200ms over a ≥ 30-minute steady-state measurement window inside the `cpt-cf-usage-collector-nfr-throughput-profile` envelope; permitted measurement tolerance ±10% (i.e., p95 ≤ 220ms accepted for any single steady-state window) provided the 30-minute trailing trend stays at or below 200ms.
- **Rationale**: Low ingestion latency prevents blocking in usage source services. Anchoring on the throughput profile and a measurement tolerance removes the ambiguity in "normal load" and makes the criterion repeatable.
- **Architecture Allocation**: See DESIGN.md

#### Workload Isolation

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-workload-isolation`

The system **MUST** ensure that aggregation query workloads do not degrade ingestion latency. These workloads **MUST** be isolated from the ingestion path such that concurrent execution maintains ingestion p95 latency within the `cpt-cf-usage-collector-nfr-ingestion-latency` threshold.

- **Threshold**: Ingestion p95 latency remains ≤ 200ms during concurrent query operations
- **Rationale**: Aggregation queries are analytical workloads that can compete for storage resources with the latency-sensitive ingestion path.
- **Architecture Allocation**: See DESIGN.md

#### Query Freshness

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-query-freshness`

The system **MUST** publish a plugin-agnostic consistency contract between the synchronous ingestion ack path and the subsequent raw / aggregated / catalog query surfaces. The contract is **floor-and-ceiling**: the gear floor is the minimum every active plugin honours under default deployment posture, and each plugin's deployment guide MAY advertise a stronger ceiling.

- **Floor (gear-level)**: ingestion `Acknowledged` is durable per `cpt-cf-usage-collector-fr-ingestion` and the `(tenant_id, metric_gts_id, idempotency_key)` dedup tuple is permanently visible to subsequent ingestion attempts — the dedup-key tuple is preserved permanently and independently of the plugin's record-body retention policy — per `cpt-cf-usage-collector-fr-idempotency` and `cpt-cf-usage-collector-fr-data-lifecycle`. Visibility of the same record through `cpt-cf-usage-collector-fr-query-raw`, `cpt-cf-usage-collector-fr-query-aggregation`, and the catalog read paths reached by `cpt-cf-usage-collector-fr-metric-existence-and-kind` is **eventually consistent with no upper bound** relative to the ingestion ack. The floor is per-`(tenant_id, metric_gts_id)`; no cross-tenant or cross-metric ordering claim is made. No monotonic-reads-per-`(tenant_id, metric_gts_id)` guarantee at the floor.
- **Ceiling (per-plugin)**: each `usage-collector-plugin-<backend>` deployment guide **MUST** publish the plugin's actual consistency profile (e.g., "sync, single-node", "bounded-staleness ≤ N ms", "eventual, no bound — see workload-isolation routing"). Consumers that depend on a tighter bound consciously couple themselves to that plugin's ceiling; the coupling **MUST** be recorded in the consumer's own design document.
- **Consumer rule**: read-after-write source-gear flows (admission control, post-emit summary, immediate-readback dashboards) **MUST NOT** be designed against the query surfaces. Same-request outcome flows **MUST** consume the ingestion ack. Near-real-time observers poll within `cpt-cf-usage-collector-nfr-query-latency` and accept lag bounded by the active plugin's published ceiling.
- **Threshold**: Floor: no gear-level numeric bound (absence claim, verified by documentation review over DESIGN §3.10.8, `plugin-spi.md` §"Consistency profile", and the feature pointers). Ceiling: per-plugin published profile, verified against each plugin's release-readiness review.
- **Rationale**: The workload-isolation NFR routes ingestion and query to isolated backend pools (`cpt-cf-usage-collector-nfr-workload-isolation`); that isolation creates queryability lag between the ack path and the query path that nothing else names. Publishing the floor at PRD level lets consumers code defensively against the weakest plugin without reading per-plugin documentation, and lets plugin authors advertise stronger ceilings honestly rather than under an implicit gear-wide claim that overpromises for backends like ClickHouse-replicated. The architectural decision is recorded in DESIGN §5.1 (consistency-contract ADR).
- **Architecture Allocation**: See DESIGN.md §3.10.8 (Consistency contract).

#### Authentication Required

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-authentication`

The system **MUST** reject every REST and SDK call without a platform-resolved authentication context. Authentication is delegated to the platform gateway on REST and to the caller-supplied authentication context on the in-process SDK; the Usage Collector **MUST NOT** implement an authentication path of its own, **MUST NOT** consume any credential-resolution contract, and **MUST NOT** allow any handler or method to begin business processing before the authentication context is validated as present and platform-resolved. The correlation identifier carried on the inbound authentication context **MUST** be propagated unchanged on every downstream call so the gateway-emitted authentication record can be reconciled with gear-level activity.

- **Threshold**: Zero handlers or methods proceed to business processing without a platform-resolved authentication context; zero requests with absent, anonymous, or synthesized identity reach plugin dispatch.
- **Rationale**: Usage data is billing-sensitive and authentication is a platform-wide gateway responsibility; stating the NFR as "reject every call without a platform-resolved authentication context" ensures one uniform authentication boundary across gears.
- **Architecture Allocation**: See DESIGN.md.

#### Authorization Enforcement

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-authorization`

The system **MUST** enforce authorization for all read and write operations based on the caller's authenticated identity, tenant context, and Metric-level permissions where applicable.

- **Threshold**: Zero unauthorized data access or write
- **Rationale**: Authorization prevents unauthorized usage data manipulation and cross-tenant data leakage.
- **Architecture Allocation**: See DESIGN.md

#### Horizontal Scalability

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-scalability`

The system **MUST** scale horizontally to handle increased ingestion and query load without architectural changes. Horizontal scaling **MUST** preserve a per-instance ingestion-efficiency ratio of ≥ 0.8 relative to the launch single-instance baseline up to 4× the launch fleet size, measured under the `cpt-cf-usage-collector-nfr-throughput-profile` envelope and sustaining `cpt-cf-usage-collector-nfr-ingestion-latency` (p95 ≤ 200ms) and `cpt-cf-usage-collector-nfr-query-latency` (p95 ≤ 500ms) at every fleet-size step.

- **Threshold**: For fleet sizes N ∈ {1×, 2×, 3×, 4×} of the launch fleet, sustained ingestion throughput ≥ 0.8 × N × launch single-instance baseline, with ingestion p95 ≤ 200ms (tolerance ±10%) and aggregation query p95 ≤ 500ms (tolerance ±10%); measurement window ≥ 30 minutes of steady-state operation per fleet size.
- **Rationale**: Usage volume grows with platform adoption; vertical scaling is insufficient for sustained growth. Replacing the prior "linear throughput scaling" wording with an efficiency ratio, a tested fleet-size ladder, and explicit latency bounds makes the requirement testable and bounds the inevitable per-instance overhead.
- **Architecture Allocation**: See DESIGN.md

#### Graceful Degradation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-graceful-degradation`

The system **MUST** continue accepting and persisting usage records even if downstream consumers (billing, monitoring) are unavailable.

- **Threshold**: Zero ingestion failures due to downstream consumer unavailability
- **Rationale**: Usage collection must not be blocked by consumer outages; the collector is the source of truth and must remain operational independently.
- **Architecture Allocation**: See DESIGN.md

#### Plugin Contract Stability Across Versions

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-plugin-contract-stability`

The Plugin SPI (`cpt-cf-usage-collector-interface-plugin`), the SDK trait (`cpt-cf-usage-collector-interface-sdk-client`), and the REST API (`cpt-cf-usage-collector-interface-rest-api`) **MUST** each remain stable within a major version. A plugin built against Plugin SPI version `N` **MUST** continue to work against version `N.x` for any value of `x`; the same guarantee applies to in-process consumers of the SDK trait and to remote consumers of the REST API. Breaking changes **MUST** be expressed as a new major version that coexists with the prior major version for at least one migration window, so plugin authors, consumer gears, and remote callers can migrate on independent schedules from the Usage Collector itself.

- **Threshold**: Each public surface compiled or wired against the initial released major version **MUST** continue to function unchanged against every minor and patch release of the same major version; at most one prior major version is supported concurrently per surface.
- **Rationale**: Plugin authors, downstream consumer gears, and remote usage sources are typically not the same teams as Usage Collector maintainers (e.g., a TimescaleDB or ClickHouse plugin maintained by an external storage team, or a billing system in a separate release train). Forcing them to recompile or redeploy on every minor Usage Collector release creates ecosystem coordination overhead and discourages reuse.
- **Architecture Allocation**: See DESIGN.md

#### Developer and Operator Experience

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-developer-operator-experience`

The gear **MUST** provide a predictable integration and operations experience for SDK and REST API consumers. Platform developers **MUST** be able to complete a first successful usage-record submission using published examples within 30 minutes when they already have valid platform credentials and tenant context. Platform operators **MUST** be able to complete Metric registration, verify Metric visibility, and identify common failure categories using documented API behavior and standard health information without maintainer assistance for routine cases.

- **Threshold**: Developer first-ingestion walkthrough ≤ 30 minutes; operator Metric registration and visibility verification ≤ 5 minutes; ≥ 90% of sampled routine failure cases mapped to documented cause categories without maintainer escalation.
- **Rationale**: The Usage Collector is a shared platform surface; adoption depends on fast integration by service teams and repeatable diagnosis by operators.
- **Verification**: Release-readiness walkthrough using the published SDK/REST examples and operator runbook.

#### Documentation Coverage

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-documentation-coverage`

The gear **MUST** publish documentation covering user, administrator, API, training, and help-system needs before production release candidate.

- **User documentation**: Tenant administrators and downstream consumers can understand raw and aggregated usage query semantics, active versus inactive records, Metric selection, and authorization outcomes.
- **Admin documentation**: Platform operators can configure storage-extension selection, readiness expectations, Metric lifecycle operations, and operational ownership boundaries.
- **API documentation**: REST API documentation and SDK documentation cover every public operation, authentication and authorization expectations, examples for ingestion and query, compatibility guarantees, and platform-standard error behavior.
- **Training material**: A concise onboarding guide or quickstart demonstrates first ingestion, first aggregation query, Metric registration, and common error interpretation.
- **Help-system expectations**: The gear does not provide an interactive user interface or embedded help system; self-service help is provided through published docs, actionable API errors, and operational runbooks.
- **Threshold**: 100% of public REST and SDK operations documented with at least one successful-path example and documented error categories for authn/authz denial, unregistered Metric, metadata size rejection, inactive event deactivation, and storage-extension unavailability.
- **Rationale**: Complete documentation lowers integration and support load for a shared platform capability.
- **Verification**: Documentation coverage review before release candidate and on every major API or SDK change.

#### Support Readiness

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-support-readiness`

The gear **MUST** define production support expectations for the Usage Collector as a shared platform infrastructure dependency.

- **Support tier**: Production Usage Collector deployments are a platform `p1` support dependency for ingestion availability and a `p2` support dependency for query degradation that does not block ingestion.
- **SLA expectations**: Ingestion outage or fail-closed authn/authz outage affecting valid callers receives operator acknowledgement within 15 minutes during covered production hours; query-latency degradation receives acknowledgement within 1 business day; documentation or API usage questions receive acknowledgement within 2 business days.
- **Self-service support**: Published runbooks and troubleshooting material cover common ingestion rejection causes, query authorization denials, Metric lifecycle errors, storage-extension readiness failures, and latency-threshold breaches.
- **Diagnostic capability**: Operators and support engineers can use health visibility, metrics visibility, structured errors, and correlation identifiers to classify failures without direct database inspection.
- **Troubleshooting support**: Troubleshooting guidance states the expected owner for platform authn/authz issues, storage-extension readiness issues, usage-source payload errors, Metric lifecycle errors, and downstream consumer query problems.
- **Threshold**: 100% of the listed common failure classes have documented diagnosis steps, owner routing, and expected caller-facing error category before production release candidate.
- **Rationale**: Shared metering failures affect billing, quota, and dashboard consumers; support routing and self-service diagnosis must be ready before launch.
- **Verification**: Operations readiness review before release candidate and after each support-impacting major change.

#### Throughput Profile

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-throughput-profile`

The system **MUST** sustain the following ingestion and query workload profile at launch capacity:

- **Sustained ingestion**: ≥ 10,000 usage records per second (cross-reference `cpt-cf-usage-collector-nfr-throughput`).
- **Peak ingestion burst**: ≥ 30,000 usage records per second for ≤ 5 minutes in any 60-minute window without breaching `cpt-cf-usage-collector-nfr-ingestion-latency` (p95 ≤ 200ms).
- **Concurrent query consumers**: ≥ 100 active aggregation queries without breaching `cpt-cf-usage-collector-nfr-query-latency` (p95 ≤ 500ms) or degrading ingestion p95 (`cpt-cf-usage-collector-nfr-workload-isolation`).
- **Daily transaction volume**: ≥ 700,000,000 accepted ingestion calls per 24-hour day at the sustained rate.
- **Seasonal / cyclical pattern**: monthly billing-cycle close is the highest concurrent-query period; ingestion volume is not expected to spike seasonally beyond the burst envelope.

- **Threshold**: Sustained ≥ 10,000 records/sec; burst ≥ 30,000 records/sec for ≤ 5 minutes per 60-minute window; ≥ 100 concurrent aggregation queries; ≥ 700,000,000 accepted ingestion calls per 24-hour day.
- **Rationale**: Documenting the steady-state, peak, burst, and concurrent-consumer profile lets capacity planning, alert thresholds, and load tests share one product-level envelope.
- **Architecture Allocation**: See DESIGN.md

#### Capacity Headroom

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-capacity-headroom`

The system **MUST** preserve headroom for projected platform growth without architectural change at the gear surface:

- **12-month growth**: handle 3× the launch sustained ingestion and concurrent-query volume by adding instances and tuning the active storage plugin, with no breaking PRD or DESIGN change.
- **24-month growth**: handle 6× the launch sustained ingestion and concurrent-query volume without a new major version of the public REST API, SDK trait, or Plugin SPI (cross-reference `cpt-cf-usage-collector-nfr-plugin-contract-stability`).
- **Tenant fan-out**: support ≥ 10,000 distinct authorized tenants emitting concurrently without per-tenant gear configuration.
- **Metric catalog size**: support ≥ 10,000 registered Metrics in the platform-global catalog without breaching `cpt-cf-usage-collector-nfr-ingestion-latency`.

- **Threshold**: 3× growth in 12 months; 6× growth in 24 months without breaking-version change; ≥ 10,000 concurrent tenants; ≥ 10,000 registered Metrics.
- **Rationale**: Capacity planning at PRD level lets operators size storage and compute ahead of platform growth and protects the public contracts during growth-driven scaling. Initial release establishes the capacity baseline; historical growth data is not yet available (cross-reference [§11](#11-assumptions)).
- **Architecture Allocation**: See DESIGN.md

#### Availability Boundary

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-nfr-availability-boundary`

The system **MUST** clarify the operational boundary around `cpt-cf-usage-collector-nfr-availability`:

- **Operating coverage**: ingestion, query, deactivation, and Metric lifecycle endpoints are served 24/7 with no business-hours-only operating mode.
- **Maintenance windows**: any gear-driven planned maintenance affecting ingestion or query **MUST** be announced ≥ 7 calendar days in advance and **MUST** be counted against the 99.95% monthly availability budget; routine plugin and platform maintenance follow the active plugin's and platform's published windows.
- **Durability of acknowledged records**: once the API has returned a successful ingestion acknowledgement, the record **MUST** be retrievable through the raw and aggregated query surfaces for the retention window provided by the active storage plugin; the gear **MUST NOT** return an acknowledgement before plugin acceptance.
- **Geographic availability**: a single deployment serves the operator-selected region; cross-region availability is delegated to the platform topology and [§4.2](#42-out-of-scope) deferred Multi-Region Replication.
- **Recovery semantics at gear level**: gear instances are stateless and replaceable; physical backup, restore, Recovery Point Objective (RPO), and Recovery Time Objective (RTO) mechanics are delegated per the [§6.2](#62-nfr-exclusions) Gear-Specific Disaster Recovery exclusion to the platform's general DR posture and the active storage plugin's DR mechanisms.

- **Threshold**: 24/7 operating coverage; planned-maintenance notice ≥ 7 calendar days; zero acknowledged-then-lost records under gear-level recovery; single-region per deployment.
- **Rationale**: Pinning operating coverage, maintenance discipline, the durability boundary, and the recovery delegation at PRD level lets downstream consumers and tenants reason about availability without relying on platform-DR mechanics.
- **Architecture Allocation**: See DESIGN.md

#### Batch and Report Timing

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-batch-and-report-timing`

The system **MUST** bound the following batch and report-style operations under the load envelope defined by `cpt-cf-usage-collector-nfr-throughput-profile` (sustained ≥ 10,000 records/sec, ≥ 100 concurrent aggregation queries, no active burst in progress), measured over a ≥ 30-minute steady-state window with permitted measurement tolerance ±10% on each p95 figure below:

- **Batched ingestion submission**: a single API call carrying up to 100 usage records **MUST** complete within 500ms at p95.
- **Report-style aggregation**: a 90-day single-tenant aggregation request grouping across up to two of {time bucket, subject, resource, source gear} and returning ≤ 100,000 result rows **MUST** complete within 5 seconds at p95.
- **Bulk raw query page**: a raw-query page of ≤ 1,000 records over a 24-hour window **MUST** complete within 1 second at p95.

- **Threshold**: Batched ingestion p95 ≤ 500ms (≤ 100 records); 90-day report p95 ≤ 5s (≤ 2 groupings, ≤ 100,000 rows); 24-hour raw-page p95 ≤ 1s (≤ 1,000 records).
- **Rationale**: Source gears batch emissions to amortize gateway and PDP cost; downstream consumers run report-style queries during billing close and dashboard refresh. Bounding batch and report timing keeps these workloads inside their interactive budgets.
- **Architecture Allocation**: See DESIGN.md

#### Deployment Operations

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-deployment-operations`

The system **MUST** support the following release and rollback expectations at PRD level:

- **Release cadence**: minor and patch releases ship at most every 14 calendar days; emergency fix releases ship as needed within the same 14-day window without forcing dependent surface bumps (cross-reference `cpt-cf-usage-collector-nfr-plugin-contract-stability`).
- **Rollback**: any released gear version **MUST** be rollbackable to the immediately prior version within ≤ 10 minutes using standard platform deployment tooling, with zero acknowledged-record loss.
- **Progressive release**: production deployments **MUST** be releasable through canary or rolling strategies; a canary stage covering ≥ 5% of ingestion traffic **MUST** be supported before fleet-wide promotion.
- **Environment parity**: development, staging, and production deployments **MUST** run the same gear artifact with environment-specific configuration overrides only; environment-only forks of gear code are not permitted.

- **Threshold**: Release cadence ≤ 14 calendar days; rollback ≤ 10 minutes with zero acknowledged-record loss; canary stage ≥ 5% available; one gear artifact across dev/stage/prod.
- **Rationale**: Predictable release and rollback discipline lets platform operators absorb Usage Collector changes alongside platform-wide releases without bespoke per-gear orchestration; environment parity prevents production-only defects from slipping past staging.
- **Architecture Allocation**: See DESIGN.md

#### Operational Visibility

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-operational-visibility`

The system **MUST** expose product-level operational visibility for ingestion, query, authorization, and storage-plugin health:

- **Observable signals**: ingestion latency, ingestion throughput, query latency, PDP error rate, plugin acceptance error rate, plugin readiness, and Metric-catalog freshness **MUST** each be exposed as observable signals scrapable by the platform monitoring infrastructure.
- **Structured logs**: every accepted and rejected API operation **MUST** emit a structured log record carrying the correlation identifier from `cpt-cf-usage-collector-fr-audit-trail`.
- **Log retention**: gear-emitted structured log records **MUST** be retained ≥ 30 calendar days at the platform log infrastructure.
- **Alert categories**: alert configuration **MUST** cover at least ingestion-latency breach (p95 above `cpt-cf-usage-collector-nfr-ingestion-latency` for ≥ 5 minutes), throughput cliff (sustained drop ≥ 50% from the trailing 1-hour baseline), availability-budget burn (≥ 25% of monthly budget consumed in any 24-hour window), query-latency breach (p95 above `cpt-cf-usage-collector-nfr-query-latency` for ≥ 15 minutes), and plugin-unready (the host's structural plugin readiness alert fails to hold for ≥ 1 minute; this is a structural condition observed by the Plugin Host, not a probe result).
- **Dashboards**: operator dashboards **MUST** present the observable signals and alert states above; dashboard implementation lives outside the gear per [§3.1](#31-gear-specific-environment-constraints).
- **Capacity-monitoring signal**: utilization against `cpt-cf-usage-collector-nfr-throughput-profile` and `cpt-cf-usage-collector-nfr-capacity-headroom` **MUST** be projectable from the exposed signals (e.g., 90-day sustained-ingestion trend, concurrent-query trend, registered-Metric count trend).

- **Threshold**: 7 observable signals; correlation identifiers on 100% of API operations; ≥ 30 calendar days log retention; 5 alert categories defined; capacity utilization projectable from exposed signals.
- **Rationale**: Operations needs an enumerated visibility surface so dashboards, alerting, and capacity planning consume the same product-level signal set; pinning categories at PRD level prevents drift between gears and the platform monitoring layer.
- **Architecture Allocation**: See DESIGN.md

#### Error and Recovery Experience

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-nfr-error-experience`

The system **MUST** present a consistent error and recovery experience to callers:

- **Actionable errors**: every API error response **MUST** identify the caller-facing cause category from a documented set of cause categories and indicate whether the operation is retryable or terminal. The canonical enumeration of gear-specific cause categories is the OpenAPI contract (`usage-collector-v1.yaml`).
- **Retry guidance**: retryable errors **MUST** be safe to retry under `cpt-cf-usage-collector-fr-idempotency` without producing duplicate accumulation; non-retryable errors **MUST NOT** advise retry.
- **Degraded mode**: when the platform-resolved authentication context, the platform PDP, or the active storage plugin is structurally unavailable, the gear **MUST** fail closed with a retryable error and **MUST NOT** synthesize identities, decisions, or records (cross-reference `cpt-cf-usage-collector-fr-authn-delegation`, `cpt-cf-usage-collector-fr-ingestion-authorization`, `cpt-cf-usage-collector-nfr-graceful-degradation`).
- **Escalation path**: caller-facing errors **MUST** route to the documented owners in `cpt-cf-usage-collector-nfr-support-readiness` (platform identity, platform PDP, usage-source payload owner, storage-extension owner, downstream consumer).

- **Threshold**: 100% of API errors carry a documented cause category and retryability flag; zero retry advice on terminal errors; zero synthesized fallback identities or records under degraded mode.
- **Rationale**: A consistent error contract lets source gears and downstream consumers implement uniform retry policies and lets operators triage incidents using one taxonomy.
- **Architecture Allocation**: See DESIGN.md

### 6.2 NFR Exclusions

The following commonly applicable NFR categories are not applicable to this gear:

- **Safety (ISO/IEC 25010:2023 §4.2.9)**: Not applicable — the Usage Collector is a server-side data API with no physical interaction, no safety-critical operations, and no ability to cause harm to people, property, or the environment.
- **End-user UI accessibility and usability**: Not applicable — the Usage Collector exposes no user-facing UI. Developer, API consumer, and operator experience obligations are covered by `cpt-cf-usage-collector-nfr-developer-operator-experience`, `cpt-cf-usage-collector-nfr-documentation-coverage`, and `cpt-cf-usage-collector-nfr-support-readiness`.
- **Internationalization / Localization**: Not applicable — the gear exposes no user-facing text, labels, or locale-sensitive output.
- **Privacy by Design (GDPR Art. 25) as a standalone regulatory conformance claim**: Not applicable. PRD-level Privacy by Design obligations are realized through `cpt-cf-usage-collector-fr-privacy-controls` and `cpt-cf-usage-collector-fr-data-classification`; standalone GDPR Article 25 conformance is governed at platform level. Subject IDs stored by the Usage Collector are opaque internal platform identifiers; PII management is the responsibility of the platform identity layer (cross-reference [§5.3](#53-attribution-isolation) Subject Attribution).
- **Regulatory Compliance (GDPR, HIPAA, PCI DSS, SOX) as standalone gear obligations**: Not applicable — this is an internal platform infrastructure gear. The gear handles no payment card data (PCI DSS N/A), no healthcare records (HIPAA N/A), and no financial-reporting source data (SOX N/A). Platform-level regulatory obligations are governed at the platform level; the gear's applicability statement is `cpt-cf-usage-collector-fr-standards-compliance`.
- **Consent Management and Data Subject Rights workflows**: Not applicable at gear level. Consent capture, withdrawal, and data-subject-rights execution (access, rectification, erasure, restriction, portability, objection) are owned by the platform identity, legal, and governance layers; the Usage Collector does not host a gear-local consent store or DSR workflow (cross-reference `cpt-cf-usage-collector-fr-standards-compliance`).
- **Data Sovereignty and Cross-Border Transfer policy at gear level**: Not applicable. Data residency, cross-border transfer restrictions, and replication topology are governed by the platform deployment topology and the operator-selected storage plugin's deployment profile (cross-reference `cpt-cf-usage-collector-fr-data-lifecycle`, `cpt-cf-usage-collector-fr-standards-compliance`, and [§4.2](#42-out-of-scope) deferred Multi-Region Replication).
- **Gear-Specific Disaster Recovery (RPO / RTO / backup policy)**: Not applicable as a standalone gear requirement. Backup, restore, RPO, and RTO are governed by the platform's general disaster-recovery posture and the operator-selected storage backend's own DR mechanisms; the Usage Collector does not define gear-specific recovery thresholds.
- **Device / Platform Requirements (UX-PRD-004)**: Not applicable — the Usage Collector is server-side platform infrastructure with no UI client. It is consumed exclusively via the in-process SDK trait (`cpt-cf-usage-collector-interface-sdk-client`), the Plugin SPI (`cpt-cf-usage-collector-interface-plugin`), and the REST API (`cpt-cf-usage-collector-interface-rest-api`); no browser, mobile, desktop, offline, or responsive-design surfaces exist, so per-device, per-platform, and offline-mode obligations do not apply at gear level.
- **Inclusivity Requirements (UX-PRD-005)**: Not applicable — the Usage Collector serves a narrow technical audience (platform developers, platform operators, tenant administrators, and downstream consumer services) through the in-process SDK, Plugin SPI, and REST API. The gear exposes no end-user UI surface, no per-subject profile view, and no human-targeted content, so cognitive-accessibility, diverse-user-population, and cultural-sensitivity obligations remain at the platform level rather than being asserted as standalone gear obligations.

## 7. Public Library Interfaces

### 7.1 Public API Surface

The Usage Collector exposes three public surfaces: an in-process SDK trait consumed by platform gears, a Plugin SPI implemented by storage extensions, and a REST API consumed by remote usage sources, operator tooling, and downstream consumers. The REST API is the full product surface for ingestion, query, event deactivation, Metric lifecycle, and health visibility. The SDK trait is a narrower in-process consumer surface, while the Plugin SPI is the storage-extension surface. The entries below describe stable capability surfaces at PRD level; detailed signatures and wire contracts are defined in DESIGN.md and the linked contract documents.

#### Usage Collector SDK

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-interface-sdk-client`

**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-platform-developer`, `cpt-cf-usage-collector-actor-usage-consumer`

<!-- cpt-cf-id-content -->

**Type**: In-process async client trait
**Stability**: stable (V1)
**Description**: In-process consumer surface covering ingestion of usage and compensation records (`cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-usage-compensation`, `cpt-cf-usage-collector-fr-idempotency`), raw query (`cpt-cf-usage-collector-fr-query-raw`), aggregated query (`cpt-cf-usage-collector-fr-query-aggregation`), and individual event deactivation (`cpt-cf-usage-collector-fr-event-deactivation`). Operator and Metric-lifecycle operations are intentionally REST-only.
**Consumed / Provided Data**: consumes usage submissions, raw and aggregated query requests, and deactivation requests; provides acceptance acknowledgements, raw usage views, and aggregated usage results. Operator-only data classes are intentionally not exposed on this trait.
**Availability / Fallback**: in-process trait availability follows the Usage Collector gear and its active storage dependency. The SDK does not provide an alternate persistence path or synthesize usage data.
**Breaking Change Policy**: Major version bump required for trait method signature changes; within a version, only additive changes (new methods with default implementations). The platform supports one previous major version of this trait concurrently to give consumer gears a migration window, consistent with `cpt-cf-usage-collector-nfr-plugin-contract-stability`.
See DESIGN.md for the trait signature.

<!-- cpt-cf-id-content -->

#### Plugin SPI

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-interface-plugin`

**Actors**: `cpt-cf-usage-collector-actor-storage-backend`

<!-- cpt-cf-id-content -->

**Type**: Storage plugin SPI
**Stability**: stable (V1)
**Description**: Storage-extension surface implemented by each plugin for persistence of usage and compensation records (`cpt-cf-usage-collector-fr-pluggable-storage`, `cpt-cf-usage-collector-fr-usage-compensation`), raw and aggregated query (`cpt-cf-usage-collector-fr-query-raw`, `cpt-cf-usage-collector-fr-query-aggregation`), and individual event deactivation including the depth-1 cascade to active compensations referencing a deactivated usage record (`cpt-cf-usage-collector-fr-event-deactivation`). The operator selects the active backend via configuration (see `cpt-cf-usage-collector-fr-pluggable-storage`).
**Consumed / Provided Data**: consumes usage persistence, raw and aggregated query, and deactivation requests; provides persistence acknowledgements, raw usage views, and aggregated usage results.
**Availability / Fallback**: backend-bound — the SPI's availability tracks the selected storage backend per `cpt-cf-usage-collector-nfr-availability`. There is no parallel storage path in the Usage Collector.
**Breaking Change Policy**: Plugin contract versioned with the gear per `cpt-cf-usage-collector-nfr-plugin-contract-stability`; breaking trait changes require a coordinated release with every plugin implementation. The platform supports one previous major version of the Plugin SPI concurrently to give plugin authors a migration window.
See DESIGN.md for the trait signature.

<!-- cpt-cf-id-content -->

#### REST API

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-interface-rest-api`

**Actors**: `cpt-cf-usage-collector-actor-usage-source`, `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-platform-operator`, `cpt-cf-usage-collector-actor-tenant-admin`

<!-- cpt-cf-id-content -->

**Type**: HTTP REST API
**Stability**: stable (V1)
**Description**: HTTP API consumed by remote usage sources, operator tooling, and downstream consumers. This REST surface is the full product operation surface for the gear. Capability categories:

- Ingestion of usage and compensation records — `cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-usage-compensation`, `cpt-cf-usage-collector-fr-idempotency`.
- Raw query — `cpt-cf-usage-collector-fr-query-raw`
- Aggregated query — `cpt-cf-usage-collector-fr-query-aggregation`
- Individual event deactivation — `cpt-cf-usage-collector-fr-event-deactivation`
- Metric registration and lifecycle (create, list, get, delete) — `cpt-cf-usage-collector-fr-metric-registration`, `cpt-cf-usage-collector-fr-metric-deletion`, `cpt-cf-usage-collector-fr-metric-existence-and-kind`
- Health

The detailed wire contract is authored in `usage-collector-v1.yaml` (sibling to DESIGN.md) and the endpoint enumeration is in DESIGN §3.3 Endpoints Overview; the yaml is authoritative for wire schemas and the canonical error envelope shape. Per-endpoint stability for v1 is captured in the DESIGN §3.3 Endpoints Overview table; the major-version stability contract is declared in the yaml info description. Technical API details are intentionally not duplicated here.

**Consumed / Provided Data**: consumes usage submissions, raw and aggregated query requests, deactivation requests, Metric lifecycle requests, and health requests; provides ingestion acknowledgements, raw usage views, aggregated usage results, Metric catalog state, health visibility, and platform-standard errors.
**Availability / Fallback**: served behind the platform API gateway; authentication is performed by the platform gateway upstream of the collector, and PDP authorization is on the critical path per `cpt-cf-usage-collector-contract-authz-resolver`. Read availability and degradation behavior follow `cpt-cf-usage-collector-nfr-availability` and `cpt-cf-usage-collector-nfr-graceful-degradation`.
**Breaking Change Policy**: Major version bump required (v1 → v2) for endpoint removal or incompatible request / response schema changes; within v1, only additive changes (new endpoints, new optional fields). The platform supports one previous major version of the REST API concurrently to give remote consumers a migration window, consistent with `cpt-cf-usage-collector-nfr-plugin-contract-stability`.
See DESIGN.md for endpoint contracts.

<!-- cpt-cf-id-content -->

### 7.2 External Integration Contracts

The Usage Collector requires two platform services as outbound dependencies — Platform PDP and platform registry/orchestration services for storage extension selection — and provides two outward contracts: a Storage Plugin Contract for storage extensions and a Downstream Usage Reader Contract for billing, quota enforcement, dashboards, and platform monitoring consumers. Caller authentication is performed by the ToolKit gateway upstream of the collector and is not an outbound dependency declared by this gear.

#### Platform PDP Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-authz-resolver`

<!-- cpt-cf-id-content -->

**Direction**: required from `authz-resolver`
**Protocol/Format**: Platform PDP authorization decisions for every ingestion, query, and operator-write operation.
**Consumed / Provided Data**: consumes caller identity and product-level operation context; receives permit/deny decisions and any authorized read-scope constraints.
**Availability / Fallback**: PDP authorization is on the critical path for every ingestion, query, and operator-write call; there is no fallback or cached-decision path. When the PDP is unreachable, all authorized operations fail closed (denied) with a deterministic platform-authorization error; the Usage Collector does not serve cached decisions or invent a permissive fallback.
**Compatibility**: Contract follows the platform authorization protocol; changes require coordinated release.

<!-- cpt-cf-id-content -->

#### Platform Registry / Orchestration Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-gts-registry`

<!-- cpt-cf-id-content -->

**Direction**: required from client
**Protocol/Format**: Platform registry and orchestration services support operator-selected storage extension resolution and lifecycle.
**Consumed / Provided Data**: consumes the operator-selected storage extension identity; receives the active storage extension needed for persistence and query capability.
**Availability / Fallback**: Storage extension resolution is required for gear readiness. When the required registry or orchestration dependency is unavailable during startup, the Usage Collector does not advertise readiness.
**Compatibility**: Selector identifiers follow the platform registry and orchestration protocols; changes require a coordinated release with the registry, the orchestrator, and every plugin implementation.

<!-- cpt-cf-id-content -->

#### Storage Plugin Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-storage-plugin`

<!-- cpt-cf-id-content -->

**Direction**: provided by library (Plugin SPI offered to plugin authors implementing storage backends)
**Protocol/Format**: Storage Plugin SPI (`cpt-cf-usage-collector-interface-plugin`) implemented by storage backends selected by operators.
**Consumed / Provided Data**: the Usage Collector dispatches persistence, raw query, aggregated query, and individual deactivation requests; plugins return acknowledgements and usage results. Plugins **MUST NOT** invent records.
**Availability / Fallback**: A plugin's availability is its own concern; the Usage Collector treats plugin unavailability per `cpt-cf-usage-collector-nfr-availability` and `cpt-cf-usage-collector-nfr-graceful-degradation`. There is no parallel local storage path in the Usage Collector.
**Compatibility**: The Plugin SPI follows `cpt-cf-usage-collector-nfr-plugin-contract-stability` — a plugin built against the initial released major version continues working against every minor and patch release of the same major version; breaking changes are expressed as a new major version that coexists with the prior major version during a migration window. Plugins ship on independent release schedules from the Usage Collector itself.

<!-- cpt-cf-id-content -->

#### Downstream Usage Reader Contract

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-contract-downstream-usage-reader`

<!-- cpt-cf-id-content -->

**Direction**: provided by library (read-only usage views consumed by downstream readers: billing, quota enforcement, dashboards, and platform monitoring)
**Protocol/Format**: Public REST API `cpt-cf-usage-collector-interface-rest-api` for out-of-process readers and, for in-process platform gears, the SDK trait `cpt-cf-usage-collector-interface-sdk-client`.
**Consumed / Provided Data**: downstream readers submit raw and aggregated query requests and health requests where applicable; the Usage Collector returns raw usage views, aggregated usage results, and health visibility. Business logic (pricing, rating, invoice generation, quota enforcement decisions) **MUST NOT** be performed inside the Usage Collector; it is the responsibility of the downstream reader.
**Availability / Fallback**: Query availability and latency follow `cpt-cf-usage-collector-nfr-query-latency` and `cpt-cf-usage-collector-nfr-availability`. PDP authorization is on the critical path per `cpt-cf-usage-collector-contract-authz-resolver` and is fail-closed. Downstream readers **MUST NOT** invent usage state when the Usage Collector is unavailable.
**Compatibility**: Read shapes follow the Usage Collector's public versioning policy per `cpt-cf-usage-collector-nfr-plugin-contract-stability` — at most one prior major version of the REST API and SDK trait is supported concurrently to give downstream readers a migration window. Additive changes within a major version do not break existing readers.

<!-- cpt-cf-id-content -->

### 7.3 Endpoints Summary

The canonical endpoint surface is defined in `usage-collector-v1.yaml` (sibling file) and mirrored in DESIGN §3.3 Endpoints Overview.

## 8. Use Cases

#### Emit Usage Records

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-emit`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Preconditions**:

- Actor is an authenticated usage source
- PDP authorization policies declare which Metrics the source is permitted to emit and for which tenants

**Main Flow**:

1. Usage source emits a usage record attributed to a tenant, resource, optional subject, source gear, and a registered Metric
2. System authorizes the emission via PDP and validates the record against registered Metric and kind rules. Any failure is returned immediately to the caller before any record is accepted.
3. System accepts the record
4. Record becomes available for querying in the Usage Collector

**Postconditions**:

- Authorized, valid records are persisted in the storage backend and available for aggregation queries
- An exact-equality re-submission under an already-accepted idempotency key is silently deduplicated (no duplicate record); a same-key submission whose content differs is rejected with an actionable conflict error rather than silently dropped (cross-reference `cpt-cf-usage-collector-fr-idempotency`)

**Alternative Flows**:

- **Authorization denied**: System returns an error immediately; no record is accepted for delivery
- **Validation failed**: System returns an actionable error immediately; no record is accepted for delivery

#### Query Aggregated Usage

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-query-aggregated`

**Actor**: `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-tenant-admin`

**Preconditions**:

- Actor is authenticated with a valid SecurityContext

**Main Flow**:

1. Consumer sends an aggregation query specifying a time range, Metric, and desired grouping or rollup
2. System authorizes the query via PDP; PDP-returned constraints define the authorization boundary and user-supplied filters are applied in addition, only further narrowing the result set
3. System returns aggregated results scoped to the intersection of PDP-authorized scope and user-supplied filters

**Postconditions**:

- Consumer receives aggregated usage data within the intersection of PDP-authorized scope and user-supplied filters

**Alternative Flows**:

- **No data in range or scope**: System returns empty result set (not an error)
- **PDP denial or empty constraints**: System rejects the query immediately; no data is returned

#### Register Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-register-metric`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Preconditions**:

- Actor is authenticated with a valid SecurityContext with operator-level permissions
- The Metric identifier is unique across the deployment

**Main Flow**:

1. Operator defines the registration payload: the GTS `gts_id` and the closed `metadata_fields` list (array of strings naming every metadata key the Metric will accept)
2. Operator submits the definition via the API
3. System authorizes the request via PDP and validates: (a) `gts_id` is well-formed; (b) `gts_id` begins with a reserved kind prefix — otherwise the request is rejected with an actionable validation error; (c) `metadata_fields` is well-formed (an array of unique non-empty strings); (d) the `gts_id` is not already present in the catalog
4. System persists the Metric type in the catalog
5. Operator configures PDP authorization policies declaring which source gears are permitted to emit records referencing this Metric, and for which tenants
6. System confirms successful registration

**Postconditions**:

- The new Metric is immediately available for ingestion across all tenants; source gears can emit records referencing it by `gts_id`
- PDP policies are in effect; unauthorized source gears are rejected when attempting to emit records referencing this Metric

**Alternative Flows**:

- **Duplicate Metric identifier**: System rejects registration with an actionable conflict error; no Metric is created
- **Bad kind prefix**: System rejects registration with an actionable validation error when `gts_id` does not begin with a reserved kind prefix; no Metric is created
- **Invalid `metadata_fields`**: System rejects registration with an actionable validation error when `metadata_fields` is malformed (non-array, contains duplicates, contains empty strings); no Metric is created
- **PDP denial**: System rejects the registration before any change is made

#### Delete Metric

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-delete-metric`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Preconditions**:

- Actor is authenticated with a valid SecurityContext with operator-level permissions
- A Metric with the specified `gts_id` exists in the catalog
- The deletion is referential — it cannot proceed while any usage row still references the target Metric

**Main Flow**:

1. Operator submits a deletion request specifying the Metric's `gts_id`
2. System authorizes the request via PDP
3. System removes the Metric from the catalog; deletion is blocked while any usage row still references the target Metric
4. System confirms successful deletion

**Postconditions**:

- The Metric's `gts_id` is no longer registered; any subsequent ingestion attempt referencing it is rejected by `cpt-cf-usage-collector-fr-metric-existence-and-kind`
- The Metric's `gts_id` becomes available for re-registration

**Alternative Flows**:

- **Metric not found**: System returns an actionable not-found error
- **Metric still referenced**: System returns an actionable conflict error (metric still referenced by usage records); the Metric remains in the catalog
- **PDP denial**: System rejects the deletion before any change is made

#### Query Raw Usage Records

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-usecase-query-raw`

**Actor**: `cpt-cf-usage-collector-actor-usage-consumer`, `cpt-cf-usage-collector-actor-tenant-admin`

**Preconditions**:

- Actor is authenticated with a valid SecurityContext

**Main Flow**:

1. Consumer sends a raw-record query specifying a mandatory time range and optional product-level narrowing criteria
2. System authorizes the query via PDP; PDP-returned constraints define the authorization boundary and user-supplied filters are applied in addition, only further narrowing the result set
3. System returns a page of raw records when authorized records exist

**Postconditions**:

- Consumer receives raw records within the intersection of PDP-authorized scope and user-supplied filters
- Additional pages are available through the paging behavior defined by the public contract

**Alternative Flows**:

- **No data in range or scope**: System returns an empty page (not an error)
- **PDP denial or empty constraints**: System rejects the query immediately; no data is returned
- **Invalid paging request**: System returns an actionable error

#### Deactivate a Usage Event

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-deactivate-event`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Preconditions**:

- Actor is authenticated with a valid SecurityContext with operator-level permissions
- The target usage event exists and is active

**Main Flow**:

1. Operator submits a deactivation request identifying the target event
2. System authorizes the request via PDP
3. System transitions the event's `status` to `inactive`; no other property is modified

**Postconditions**:

- The event carries `status = inactive`; all other properties (including `tenant`, `timestamp`, `idempotency_key`, `value`, referenced Metric, resource, subject, and metadata) are unchanged
- Inactive events remain queryable and are distinguishable from active records by downstream consumers

**Alternative Flows**:

- **Event not found**: System returns a not-found error
- **Target event already inactive**: System rejects the request with an actionable error; deactivation is one-way and not applicable to an already-inactive record
- **PDP denial**: System rejects the request before any change is made

#### Compensate Previously Reported Usage

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-usecase-compensate-previously-reported-usage`

**Actor**: `cpt-cf-usage-collector-actor-usage-source`

**Preconditions**:

- The source gear is an authenticated usage source with PDP authorization to emit records for the target `(tenant_id, metric_gts_id)`
- A prior original usage row `R` exists for the target `(tenant_id, metric_gts_id)` on a `counter`-kind Metric, and `R.status = active`

**Trigger**: The source gear observes a real give-back of measured consumption (e.g., capacity refund, partial revocation, corrective downward adjustment) that partially reverses the value of `R` but does not justify a whole-row retraction of `R`.

**Main Flow**:

1. Source gear constructs a new compensation record pointing at `R` with a strictly-negative `value`, the same `(tenant_id, metric_gts_id)` as `R`, an idempotency key (mandatory), and the platform-resolved `SecurityContext`.
2. Source gear submits the record via the **same ingestion path** used for `usage` rows — there is no separate compensate endpoint (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`).
3. System authorizes the emission via PDP and validates the record against the metric-kind × row-classification matrix and the compensation pointer contract (referenced row exists, is an original usage row, shares `(tenant_id, metric_gts_id)`, and is `active`).
4. System accepts the compensation record and appends it to the store.
5. The record becomes part of aggregation results: `SUM(value)` for `(tenant_id, metric_gts_id)` over `active` rows is reduced by `|value|`; `COUNT` / `MIN` / `MAX` / `AVG` continue to operate over original usage rows only and are unaffected by the compensation.

**Postconditions**:

- A new active compensation record is persisted; `R` remains active and unchanged.
- The net `SUM(value)` for `(tenant_id, metric_gts_id)` is reduced by `|value|`.
- The compensation record is queryable through the raw and aggregated query surfaces and is part of the audit history.

**Alternative Flows**:

- **Gauge-compensation rejected**: the referenced Metric is `gauge`-kind; ingestion rejects the record with an actionable error (compensation is counter-only).
- **Invalid compensation pointer rejected**: the referenced row is missing, is itself a compensation row, belongs to a different `(tenant_id, metric_gts_id)`, or is `inactive`; ingestion rejects the record with an actionable error.
- **Deactivating-row rejected (concurrency)**: the referenced original usage row is being deactivated concurrently; the "must be active" check rejects the compensation without distributed coordination.
- **Non-negative compensation value rejected**: the supplied `value` is zero or positive on a `counter`-kind Metric; ingestion rejects the record with an actionable error.
- **Authorization denied**: PDP denies the emission; the record is rejected immediately and never persisted.
- **Idempotency conflict / retry**: an exact-equality re-submission under the same idempotency key is silently deduplicated; a same-key submission whose content differs is rejected with an actionable conflict error (cross-reference `cpt-cf-usage-collector-fr-idempotency`).
- **Cascade on later deactivation of `R`**: if `R` is subsequently deactivated by an operator, the depth-1 cascade defined by `cpt-cf-usage-collector-fr-event-deactivation` flips this compensation row to `inactive` in the same one-way step.

## 9. Acceptance Criteria

The following definitions apply to every numeric acceptance criterion in this section that references a load condition or a latency tolerance. They replace the prior informal terms "normal load", "normal operation", and "linear throughput scaling" across the PRD and anchor every test condition on a single, deterministic envelope.

- **Load envelope ("normal load" / "normal operation")** — the steady-state operating envelope defined by `cpt-cf-usage-collector-nfr-throughput-profile`: sustained ingestion ≥ 10,000 records/sec, ≥ 100 concurrent aggregation queries, ≥ 700,000,000 accepted ingestion calls per 24-hour day, with no active burst in progress unless a criterion explicitly references the burst case. The burst case is ≤ 30,000 records/sec for ≤ 5 minutes per 60-minute window.
- **Steady-state measurement window** — a contiguous window of ≥ 30 minutes during which the load envelope above is sustained; p95 figures are computed over this window and the trailing 30-minute window is reported alongside any single-sample p95.
- **Latency tolerance** — every p95 latency criterion in [§9](#9-acceptance-criteria) carries a measurement tolerance of ±10% on the stated p95 value, applied per steady-state measurement window; the trailing 30-minute trend **MUST** remain at or below the stated p95 value.
- **Linear scaling efficiency** — for `cpt-cf-usage-collector-nfr-scalability`, scaling is "linear" when the per-instance ingestion efficiency ratio stays at ≥ 0.8 relative to the launch single-instance baseline for fleet sizes N ∈ {1×, 2×, 3×, 4×} of the launch fleet, with `cpt-cf-usage-collector-nfr-ingestion-latency` and `cpt-cf-usage-collector-nfr-query-latency` p95 bounds (with the ±10% tolerance) maintained at every fleet-size step.
- **Burst tolerance** — for the burst case of `cpt-cf-usage-collector-nfr-throughput-profile`, the p95 ingestion-latency bound (200ms with ±10% tolerance) applies for the duration of the burst (≤ 5 minutes) and the trailing 60-minute window MUST contain at most one burst event.

The functional and non-functional acceptance bullets below evaluate the requirements defined in [§5](#5-functional-requirements) and [§6](#6-non-functional-requirements) against the load envelope and measurement rules established above.

- [ ] Authenticated usage sources can submit usage records attributed to a tenant, resource, optional subject, source gear, and a registered Metric; an accepted record becomes durably retained and queryable through the raw and aggregated query surfaces (cross-reference `cpt-cf-usage-collector-fr-ingestion`)
- [ ] Gauge-kind records are stored as-is without monotonicity enforcement and without delta accumulation; consecutive gauge values for the same `(tenant, metric)` may rise or fall arbitrarily; idempotent dedup by idempotency key still applies; querying a gauge-kind Metric returns the persisted point-in-time values rather than an accumulated total (cross-reference `cpt-cf-usage-collector-fr-gauge-semantics`)
- [ ] An exact-equality re-submission under the same idempotency key results in a single stored record (silent dedup), while a same-key submission whose content differs is rejected with a duplicate-submission conflict signal rather than silently dropped; the dedup key tuple is preserved across retention so the window stays unbounded (cross-reference `cpt-cf-usage-collector-fr-idempotency`)
- [ ] Records submitted without an idempotency key are rejected with an actionable error
- [ ] Counter records with negative values are rejected at ingestion
- [ ] Incoming usage records include an explicit tenant attribute; the platform PDP validates that the authenticated caller is authorized to emit records for the specified tenant before the record is accepted, and the gateway independently validates tenant attribution on ingest as a defense-in-depth check
- [ ] Every usage record includes resource attribution (resource ID and type); records without either field are rejected
- [ ] Usage records can optionally include an explicit subject attribute (subject ID and type); when present, the platform PDP validates that the authenticated caller is authorized to emit records attributed to the specified subject before the record is accepted; when absent, PDP subject validation is skipped
- [ ] Authorization failures are surfaced immediately to the caller; no record is persisted on denial
- [ ] Tenant isolation is enforced via PDP: a caller never receives a tenant's usage data — for reads or writes — without an explicit PDP authorization for that tenant; same-tenant, parent→subtenant, and platform-administrative scopes are each authorized independently
- [ ] Aggregation queries require exactly one Metric and a time range; requests omitting the Metric or supplying more than one Metric are rejected with an actionable error
- [ ] Aggregation queries return correct results for the specified metric and time range, with correct additional filtering by tenant (optional), subject, resource, and source gear when specified
- [ ] Aggregation results can be grouped by any combination of time bucket, tenant, subject, resource, and source gear
- [ ] Raw usage queries support filtering by time range (mandatory) and optionally by tenant, metric, subject, and resource
- [ ] Query authorization is enforced via PDP decision and constraint enforcement; unauthorized queries are rejected and PDP-returned constraints narrow the result scope
- [ ] The gear works with any registered plugin (e.g., ClickHouse, TimescaleDB) without code changes to the core gear
- [ ] Metadata attached to a usage record is persisted as-is and returned in query results without modification
- [ ] Usage records with metadata exceeding the configured size limit are rejected with an actionable error
- [ ] Individual usage events can be deactivated: `status` transitions from active to `inactive` with no other property changes; inactive events remain queryable and are distinguishable from active records by downstream consumers; deactivation is one-way (no reactivation operation is exposed) and rejects already-inactive targets
- [ ] Compensation entries are accepted only on `counter`-kind Metrics; a compensation entry referencing a `gauge`-kind Metric is rejected at ingestion with an actionable error (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] Compensation entries on a `counter`-kind Metric require `value < 0`; non-negative `value` (zero or positive) is rejected at ingestion with an actionable error (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] The compensation pointer to the corrected usage row is validated at ingestion: the referenced record MUST exist, MUST be an original (non-compensation) row classification, MUST share `(tenant_id, metric_gts_id)` with the incoming compensation, and MUST be `active`; any failure rejects the compensation with an actionable error (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] Deactivating a `usage` row cascades depth-1 to its active referencing `compensation` rows: those compensations are flipped to `inactive` in the same one-way step so the post-cascade `SUM` returns to the state held before either the usage record or its compensations were accepted (cross-reference `cpt-cf-usage-collector-fr-event-deactivation`, `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] Concurrency safety: a compensation referencing a `usage` row that is concurrently being deactivated is rejected by the L1 "referenced record must be active" check; no distributed coordination is required (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] Every compensation ingestion call carries a mandatory idempotency key per `cpt-cf-usage-collector-fr-idempotency`: an exact-equality re-submission is silently deduplicated and a same-key content mismatch is rejected with a duplicate-submission conflict signal, with the dedup key tuple preserved across retention (cross-reference `cpt-cf-usage-collector-fr-usage-compensation`)
- [ ] Usage records whose `metric` field does not match a registered Metric are rejected immediately with an actionable error before any record is accepted for delivery
- [ ] Metrics can be registered via API without code changes or service redeployment; the Metric identifier uniquely identifies a Metric and duplicate identifiers are rejected; registration is PDP-authorized
- [ ] Metric registration validates the supplied closed `metadata_fields` list (array of strings; ingest rejects records carrying any metadata key not in the list with an unknown metadata key signal) and validates the `gts_id` prefix against the reserved kind prefixes — any other prefix is rejected at registration with an invalid metric-kind signal; Metric Kind is derived from the `gts_id` prefix and is not a separate registration field, trait, or catalog column
- [ ] Metrics can be deleted via API; deletion is blocked while referenced by any usage record (active or inactive, in any tenant); deletion is PDP-authorized
- [ ] The system maintains 99.95% monthly availability for ingestion endpoints
- [ ] The system sustains ingestion of at least 10,000 records/sec sample-mean under the `cpt-cf-usage-collector-nfr-throughput-profile` load envelope, with every 1-minute sample-mean ≥ 9,500 records/sec, measured over a ≥ 30-minute steady-state window
- [ ] Usage record ingestion completes within 200ms at p95 under the `cpt-cf-usage-collector-nfr-throughput-profile` load envelope, with the ±10% tolerance defined in §9.0 (single-window p95 ≤ 220ms accepted only when the trailing 30-minute p95 remains ≤ 200ms)
- [ ] Aggregation queries over a 30-day range for a single tenant complete within 500ms at p95 under the `cpt-cf-usage-collector-nfr-throughput-profile` load envelope, with the ±10% tolerance defined in §9.0 (single-window p95 ≤ 550ms accepted only when the trailing 30-minute p95 remains ≤ 500ms)
- [ ] Ingestion p95 latency remains within the bound from `cpt-cf-usage-collector-nfr-ingestion-latency` (p95 ≤ 200ms with the §9.0 ±10% tolerance) while ≥ 100 concurrent aggregation queries are executing inside the `cpt-cf-usage-collector-nfr-throughput-profile` envelope
- [ ] All API operations require authentication; unauthenticated requests are rejected before any operation is performed
- [ ] Authorization is enforced on all read and write operations; unauthorized requests are rejected and no data is exposed or modified
- [ ] Usage records submitted by a `cpt-cf-usage-collector-actor-usage-source` are accepted only after PDP authorizes the authenticated caller for the supplied tenant, resource, subject (if any), source gear, and referenced Metric; unauthenticated or unauthorized submissions are rejected immediately with no partial persistence
- [ ] Throughput scales linearly per §9.0 "Linear scaling efficiency": for fleet sizes N ∈ {1×, 2×, 3×, 4×} of the launch fleet, sustained ingestion throughput ≥ 0.8 × N × launch single-instance baseline with ingestion p95 ≤ 200ms and aggregation query p95 ≤ 500ms (each with the §9.0 ±10% tolerance) maintained at every fleet-size step over a ≥ 30-minute steady-state window
- [ ] Plugin SPI, SDK trait, and REST API public surfaces remain stable within a major version: a consumer compiled or wired against major version N **MUST** continue to function unchanged against every minor and patch release of major version N; at most one prior major version is supported concurrently per surface; within a major version only additive changes (new endpoints, new optional fields, new methods with defaults) are accepted (cross-reference `cpt-cf-usage-collector-nfr-plugin-contract-stability`)
- [ ] Ingestion continues uninterrupted when downstream consumers (billing, monitoring) are unavailable
- [ ] Developer first-ingestion walkthrough using published SDK or REST examples completes within 30 minutes for a platform developer with valid credentials and tenant context
- [ ] Operator Metric registration and visibility verification completes within 5 minutes using published API documentation
- [ ] User, admin, API, training, and help-system documentation obligations are complete or explicitly marked non-applicable with reasoning before production release candidate
- [ ] Support readiness covers support tier, SLA expectations, self-service troubleshooting, diagnostics, owner routing, and common failure classes before production release candidate
- [ ] All authentication is performed by the ToolKit gateway upstream of the collector; the gear does not implement local credential validation, MFA, SSO/federation, session management, or credential issuance, does not consume any credential-resolution contract, and rejects every REST or SDK call that arrives without a platform-resolved `SecurityContext`
- [ ] Persisted gear data is limited to opaque platform identifiers, operational telemetry, and opaque caller-supplied metadata; the gear performs no decoding of identifiers to natural persons, and integrator-facing documentation states the prohibition on placing PII, payment, health, or credential data in metadata
- [ ] Every API operation contributes a correlation identifier that reconciles gear activity with platform gateway access logs and platform audit infrastructure; no gear-local audit log is maintained in v1
- [ ] Every accepted ingestion, query, deactivation, and Metric lifecycle operation is attributable to an authenticated caller identity recorded in the platform audit trail; anonymous and synthesized identities are rejected
- [ ] Privacy by Design principles are applied at PRD level (data minimization, purpose limitation, storage limitation delegated to plugin, privacy by default through PDP, pseudonymization via opaque identifiers) and documented for downstream review
- [ ] Data-ownership model is recorded: tenant administrator owns tenant usage data, platform operator stewards the Metric catalog and storage-plugin selection, and the Usage Collector gear acts as custodian; third-party access flows exclusively through PDP-authorized public read surfaces
- [ ] Data-quality guarantees are verifiable: kind-invariant enforcement, mandatory attribution, ingestion-ack latency bounded by `cpt-cf-usage-collector-nfr-ingestion-latency`, queryability governed separately by `cpt-cf-usage-collector-nfr-query-freshness` (plugin-bound; no read-your-writes assumption against the query surfaces; ack is the surface for same-request outcome), gateway-level validation, and absence of in-gear amendment (corrections expressed as deactivation plus re-emission)
- [ ] The query-freshness consistency contract is verifiable: the gear floor publishes ingestion ack durability and dedup-tuple visibility on the ingestion path, declares the Query SPI (raw, aggregated, catalog) eventually consistent with no upper bound at the gear floor, and obliges every active plugin's deployment guide to publish its actual consistency profile (`cpt-cf-usage-collector-nfr-query-freshness`); plugin-specific ceilings are verified against each plugin's published profile separately
- [ ] Data-lifecycle delegation is documented: retention, archival, purging, migration, and historical access are governed by the active storage plugin's deployment profile and the platform governance layer; the gear's surface preserves historical query access within the plugin-provided retention window
- [ ] Standards, legal, and compliance applicability is declared at PRD level: alignment with the platform security baseline and OpenAPI 3 interoperability; PCI DSS, HIPAA, and SOX explicitly not applicable; consent management, data-subject-rights, terms-of-service, and privacy-policy duties delegated to the platform identity, legal, and governance layers; data residency delegated to platform topology and operator-selected plugin deployment profile
- [ ] Sustained ingestion of ≥ 10,000 records/sec and burst ingestion of ≥ 30,000 records/sec for ≤ 5 minutes per 60-minute window are sustainable without breaching ingestion p95 latency; ≥ 100 concurrent aggregation queries are sustainable without breaching query p95 latency or degrading ingestion p95; ≥ 700,000,000 accepted ingestion calls per 24-hour day are sustainable at the sustained rate
- [ ] 12-month 3× and 24-month 6× growth headroom is sustainable without breaking-version change at the REST API, SDK trait, or Plugin SPI; ≥ 10,000 concurrent tenants and ≥ 10,000 registered Metrics are supported without breaching ingestion p95 latency
- [ ] Ingestion, query, deactivation, and Metric lifecycle endpoints operate 24/7; gear-driven planned maintenance is announced ≥ 7 calendar days in advance and counted against the 99.95% monthly availability budget; acknowledged records remain retrievable through raw and aggregated query surfaces for the active plugin's retention window; gear-level recovery is achieved by replacing stateless instances while physical backup/restore/RPO/RTO remain delegated per [§6.2](#62-nfr-exclusions)
- [ ] Batched ingestion submissions of up to 100 records complete within 500ms at p95; 90-day single-tenant report-style aggregations across up to two groupings returning ≤ 100,000 rows complete within 5 seconds at p95; 24-hour raw-query pages of up to 1,000 records complete within 1 second at p95
- [ ] Gear releases ship at most every 14 calendar days; any released version is rollbackable to the prior version within ≤ 10 minutes with zero acknowledged-record loss; a canary stage of ≥ 5% of ingestion traffic is supported before fleet-wide promotion; the same gear artifact is used across dev, stage, and prod with environment-specific configuration overrides only
- [ ] The seven listed observable signals (ingestion latency, ingestion throughput, query latency, PDP error rate, plugin acceptance error rate, plugin readiness, Metric-catalog freshness) are exposed; every accepted and rejected API operation emits a correlation identifier; gear-emitted structured log records are retained ≥ 30 calendar days; the five alert categories (ingestion-latency breach, throughput cliff, availability-budget burn, query-latency breach, plugin-unready) are configured; capacity utilization against the throughput profile and capacity headroom is projectable from the exposed signals
- [ ] Every API error response carries a documented cause category and retryability flag; retryable errors are safe to retry under idempotency without duplicate accumulation; non-retryable errors do not advise retry; when an inbound request arrives without a platform-resolved `SecurityContext`, or when the platform PDP or the active storage plugin is structurally unavailable, the gear fails closed with a retryable error and synthesizes no identities, decisions, or records (scoped client lookup is not required at the PRD layer); caller-facing errors route to the owners documented in support readiness

## 10. Dependencies

| Dependency     | Description                                                                            | Criticality |
| -------------- | -------------------------------------------------------------------------------------- | ----------- |
| authz-resolver | Platform PDP; authorizes every ingestion, query, and operator-write operation          | p1          |
| gts-registry   | Platform registry/orchestration dependency used for active storage extension selection | p1          |

## 11. Assumptions

| Assumption                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     | Owner                                                                      | Validation                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| At least one plugin (e.g., a ClickHouse or TimescaleDB storage backend) is deployed alongside the gear                                                                                                                                                                                                                                                                                                                                                                                                                       | Platform Infrastructure / Operator                                         | Verified at gear startup via platform storage-extension resolution; readiness fails if no active plugin resolves      |
| Platform documentation and operations channels are available for publishing Usage Collector quickstarts, API references, and support runbooks before release candidate                                                                                                                                                                                                                                                                                                                                                         | Usage Collector Maintainers / Platform Documentation / Platform Operations | Verified during release-readiness review                                                                                |
| The gateway delivers an authenticated security context to the usage-collector gear on every call; the gear rejects any request that arrives without a platform-resolved security context                                                                                                                                                                                                                                                                                                                                   | Platform Identity / Platform Security                                      | Verified by gateway integration tests against the usage-collector gear                                                |
| Platform gateway access logs and platform audit infrastructure are available to record authentication, authorization, ingestion, query, and operator-write outcomes and accept correlation identifiers emitted by the Usage Collector                                                                                                                                                                                                                                                                                          | Platform Operations / Platform Audit Owner                                 | Verified by end-to-end correlation between gear logs and platform audit records before release candidate              |
| Operator-selected storage plugin deployment topology meets the deployment's data residency, sovereignty, retention, and disaster-recovery obligations for tenant usage data                                                                                                                                                                                                                                                                                                                                                    | Platform Operator / Plugin Authors                                         | Verified during operator onboarding and at storage-plugin readiness review                                              |
| Initial release establishes the launch capacity baseline (10,000 records/sec sustained, 30,000 records/sec burst, 100 concurrent aggregation queries, 10,000 tenants, 10,000 registered Metrics); no prior historical growth data exists at launch and capacity-headroom verification is forward-looking against the 12-month and 24-month growth multiples                                                                                                                                                                    | Usage Collector Maintainers / Platform Operations                          | Validated by launch load tests against representative plugin backends; quarterly capacity-utilization review thereafter |
| Platform monitoring infrastructure, log infrastructure, and deployment tooling are available to host the observable signals, structured logs with ≥ 30-day retention, alert categories, and ≤ 10-minute rollback expected by the operational visibility and deployment-operations NFRs                                                                                                                                                                                                                                         | Platform Operations                                                        | Verified during operations readiness review before production release candidate                                         |
| The §9.0 load and measurement definitions (load envelope anchored on `cpt-cf-usage-collector-nfr-throughput-profile`, ≥ 30-minute steady-state measurement window, ±10% latency tolerance, linear-scaling efficiency ratio ≥ 0.8 for N ∈ {1×…4×} launch fleet) are the single source of truth for every numeric acceptance criterion in [§9](#9-acceptance-criteria) and supersede the prior informal terms "normal load", "normal operation", and "linear throughput scaling" wherever they appeared in earlier PRD revisions | Usage Collector Maintainers / Platform Operations                          | Verified during load-test plan review and release-readiness review                                                      |

## 12. Risks

| Risk                                                                                                                                                                                                                                                                                                                     | Impact                                                                                                                                 | Mitigation                                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| High-cardinality aggregation exceeds 500ms p95 query latency                                                                                                                                                                                                                                                             | Slow dashboard/billing queries                                                                                                         | See DESIGN.md for storage-extension acceleration and workload-isolation strategy                                                                                                                                                                                                                                                                            |
| v1 lacks gear-emitted audit events for operator-write paths (Metric registration, Metric deletion, individual event deactivation); reliance is on platform gateway access logs and platform audit infrastructure with gear-emitted correlation identifiers until the deferred audit-emission capability is delivered | Reduced gear-local forensic detail for operator writes; downstream compliance reporting depends on platform-level audit completeness | Document the deferral, surface correlation identifiers, and track the deferred audit-emission capability against the [§4.2](#42-out-of-scope) Audit Events item for a future phase                                                                                                                                                                          |
| Data residency or sovereignty obligations could be violated if the operator-selected storage plugin is deployed outside the permitted region or topology                                                                                                                                                                 | Compliance and contractual breach for tenants subject to residency commitments                                                         | Operator onboarding documents the residency expectations; plugin deployment profile reviewed at readiness; cross-reference `cpt-cf-usage-collector-fr-standards-compliance` and `cpt-cf-usage-collector-fr-data-lifecycle`                                                                                                                                  |
| Sustained platform growth exceeds the 24-month 6× capacity-headroom envelope before a major version of the REST API, SDK trait, or Plugin SPI is released                                                                                                                                                                | Forced unplanned breaking-version release; coordination overhead with downstream consumers and plugin authors                          | Quarterly capacity-utilization review against `cpt-cf-usage-collector-nfr-throughput-profile` and `cpt-cf-usage-collector-nfr-capacity-headroom`; capacity-monitoring signals from `cpt-cf-usage-collector-nfr-operational-visibility` surface utilization trends; major-version planning triggered when 24-month projections exceed 80% of the 6× envelope |
| Rollback exceeds the 10-minute target due to platform deployment tooling regression or plugin-coupled state                                                                                                                                                                                                              | Extended ingestion or query degradation during an incident; acknowledged-record-loss risk if rollback is bypassed                      | Rollback drill before each major release; deployment-operations NFR records the 10-minute bound so platform tooling owners share the budget; cross-reference `cpt-cf-usage-collector-nfr-deployment-operations` and `cpt-cf-usage-collector-nfr-availability-boundary`                                                                                      |

## 13. Open Questions

No open questions.

## 14. Traceability

**Design**: [DESIGN.md](./DESIGN.md)

**ADRs**: see DESIGN §5 ADR Inventory

**Artifact Changelog**

| Version | Date              | Change                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------- | ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.9.1   | 2026-06-03        | Trim-residual cleanup of the v0.9.0 restructure cascade: (a) replaced the §6.1 `cpt-cf-usage-collector-nfr-query-freshness` Rationale's inline `ADR-0011` reference + ADR-file link with a routing pointer to DESIGN §5.1 (consistency-contract ADR), restoring the v0.9.0 "ADR links → DESIGN §5 only" rule; (b) removed the §9 `cpt-cf-usage-collector-entity-entry-type` cross-ref from the compensation-pointer-validation acceptance row (entities are DESIGN territory per DATA-PRD-NO-001; the FR ID already carries the semantic); (c) dropped the §12 "Optimistic rate limit enforcement" risk row because §4.2 lists Rate Limiting as out of scope for phase 1, so the row named guarantees the v1 gear does not provide. No structural rewrite; no FR/NFR/use-case/actor IDs minted or retired.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 0.9.0   | 2026-06-03        | Strict trim — removed forbidden-content categories A–F across §5–§13; collapsed ADR links to a single §14 pointer to DESIGN §5; deleted §7.3 endpoints table; abstracted §8 use-case error envelopes; replaced §9 wire/error names with product-level invariants.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| 0.8.1   | 2026-06-02        | Flattened metric metadata to a closed `metadata_fields` list and derived Metric Kind from the `gts_id` prefix. Updates land in [§1.4](#14-glossary) Glossary, [§4.1](#41-in-scope) per-record metadata in-scope bullet, [§5.1](#51-usage-ingestion) record metadata FR (closed-shape model with an unknown metadata key signal), [§5.7](#57-metrics) Metrics FRs (payload reduced to `gts_id` + `metadata_fields`; register-time prefix validation), [§8](#8-use-cases) Register Metric use case, and [§9](#9-acceptance-criteria) Acceptance Criteria. Preserved: every other section, all NFRs, §5.6 compensation invariants, §5.4 idempotency tuple, and §5.5 query aggregation.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| 0.8.0   | 2026-06-02        | Metric-catalog simplification: the plugin-DB catalog (managed via SDK/REST) becomes the sole metric catalog; usage records reference metrics by `gts_id`; metric types are flat for v1; every registered Metric is concrete and queryable on its declared shape, with indexing strategy left to the storage plugin. Edits land in [§1.4](#14-glossary) Glossary, [§5.4](#54-pluggable-storage) Pluggable Storage Scope, [§5.7](#57-metrics) Metrics FRs, [§5.8](#58-security-and-data-governance) Data Ownership, [§8](#8-use-cases) Register Metric and Delete Metric use cases, [§9](#9-acceptance-criteria) Acceptance Criteria, and [§14](#14-traceability) Traceability. Preserved: §5.6 compensation invariants, §5.4 idempotency tuple, §5.5 query aggregation, §3.1 environment constraints, and the catalog-freshness observability signals in §6.1.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 0.7.0   | 2026-05-30        | Metric metadata and catalog-ownership rework: reworded [§5.1](#51-usage-ingestion) record-metadata FR to express a typed metadata model; replaced [§5.4](#54-pluggable-storage) pluggable-storage scope clause so both usage rows and the metric catalog are reached through the storage plugin with native referential integrity; reworked [§5.7](#57-metrics) Metrics FRs to point at the plugin-owned catalog; reworked metric-deletion from unconditional to referential (deletion is blocked while referenced by any usage row); preserved FR IDs and acceptance-criterion keys.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 0.6.0   | 2026-05-29        | Usage-compensation primitive (counter value-reversal) + re-scoped corrections framing: minted new FR `cpt-cf-usage-collector-fr-usage-compensation` under a renamed [§5.6](#56-corrections-event-deactivation--usage-compensation) "Corrections (Event Deactivation & Usage Compensation)" covering counter-only append-only negative entries that ride the existing ingestion path with PDP attribution + mandatory idempotency key; extended `cpt-cf-usage-collector-fr-event-deactivation` to apply uniformly to any `entry_type` and document the depth-1 cascade flipping active referencing compensations to `inactive` when a `usage` row is deactivated; added [§1.4](#14-glossary) "Compensation" glossary entry citing the new FR; added [§8](#8-use-cases) use case `cpt-cf-usage-collector-usecase-compensate-previously-reported-usage` with gauge / invalid-`corrects_id` / concurrency / non-negative / authz / idempotency / cascade alternative flows; updated the [§7.1](#71-public-api-surface) SDK, Plugin SPI, and REST API capability surfaces to state compensation rides the same ingestion path / persist call (no dedicated compensate endpoint, SDK method, or SPI call); added six [§9](#9-acceptance-criteria) acceptance criteria for the new FR (counter-only validation, negative-value validation, `corrects_id` L1 validation, cascade on deactivation, concurrency, mandatory idempotency); fixed the [§5.8](#58-security-and-data-governance) "Cleansing" data-quality bullet to state corrections are realized by both deactivation and compensation, cross-referencing the `EntryType` domain entity and ADR-0008 (PRD-to-ADR ID references are prohibited by kit constraints; PRD points to the ADR files under [ADR/](./ADR/) and DESIGN.md carries the ADR ID cross-link). Cascades ADR-0005 narrowing and ADR-0008 ([ADR/0008-usage-compensation.md](./ADR/0008-usage-compensation.md)) into the PRD. Backend-agnostic; no schema, API shape, or storage-engine details introduced. |
| 0.5.5   | 2026-05-28        | Idempotency conflict-semantics and unbounded-window refinement: refined `cpt-cf-usage-collector-fr-idempotency` so a same-key submission splits into exact-equality retry (silent dedup) vs canonical-field mismatch (actionable duplicate-submission conflict signal, never silently dropped); named the dedup match-key tuple `(tenant_id, metric_gts_id, idempotency_key)`; stated the idempotency window is unbounded; refined [§4.2](#42-out-of-scope) Retention Policy exclusion, [§5.8](#58-security-and-data-governance) Privacy storage-limitation bullet, and `cpt-cf-usage-collector-fr-data-lifecycle` retention bullets so the unbounded idempotency window is distinct from data retention; aligned [§1.4](#14-glossary) Idempotency Key glossary, [§8](#8-use-cases) Emit Usage postcondition, and [§9](#9-acceptance-criteria) duplicate-key acceptance criterion.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| 0.5.3   | 2026-05-26        | AuthN-PDP rework (Phase 2): rewrote `cpt-cf-usage-collector-fr-authn-delegation`, `cpt-cf-usage-collector-fr-audit-trail`, `cpt-cf-usage-collector-fr-non-repudiation`, and `cpt-cf-usage-collector-nfr-authentication` to describe the collector as accepting only platform-resolved `SecurityContext` values (ToolKit gateway upstream on REST; `&SecurityContext` first-parameter on the SDK trait) and never synthesizing identity; dropped the retired contract-authn-resolver block from [§7.2](#72-external-integration-contracts) External Integration Contracts and removed the [§10](#10-dependencies) Dependencies row for `authn-resolver`; updated [§4.1](#41-in-scope) scope bullet, [§5.6](#56-event-deactivation) deactivation justification audit-trail wording, [§5.8](#58-security-and-data-governance) third-party data-usage clause, [§6.1](#61-gear-specific-nfrs) observability signal list, [§6.1](#61-gear-specific-nfrs) error/recovery degraded-mode clause, [§7.2](#72-external-integration-contracts) intro and [§7.1](#71-public-api-surface) REST availability clause, [§9](#9-acceptance-criteria) acceptance criteria, and [§11](#11-assumptions) assumption to remove every `authn-resolver` consumer reference; PDP authorization remains required per `cpt-cf-usage-collector-contract-authz-resolver`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| 0.5.4   | 2026-05-27        | Cross-spec cleanup: removed the mandatory operator-justification requirement from the Usage Collector deactivation flow. Deleted the [§5.6](#56-event-deactivation) paragraph mandating an operator-supplied justification string under `cpt-cf-usage-collector-fr-event-deactivation`, and stripped the trailing `and a mandatory justification` clause from [§8](#8-use-cases) `cpt-cf-usage-collector-usecase-deactivate-event` Main Flow step 1 so it now reads `Operator submits a deactivation request identifying the target event`. Rationale: the platform access trail (gateway / PDP decision logs) already records who/when/what, the justification value was never persisted on the record, and there is no stakeholder requirement compelling the collector spec to mandate a "why was this corrected?" field; if a future compliance need surfaces, the requirement will be re-introduced with proper semantics. DESIGN [§3.5](./DESIGN.md#35-deactivation-handler) / [§3.6](./DESIGN.md#36-deactivation-sequence) / [§3.9.5](./DESIGN.md#395-audit-trail) deactivation-handler, sequence, and audit prose; DECOMPOSITION §2.5 deactivation invariants; plugin-spi.md exclusions; sdk-trait.md overview, method table, deactivate contract, and Validation error taxonomy; usage-collector-v1.yaml deactivate operation description plus `DeactivateRecordRequest` schema; features/event-deactivation.md Justification Validation process plus DoDs/ACs/prose/coverage-matrix entries; and domain-model.md §2.9 `DeactivationStatus` invariant are updated in lockstep in subsequent phases (2–7) of this plan.                                                                                                                                                                                                                                                                                                                                                                                               |
| 0.5.2   | 2026-05-23        | Bounded fix: enriched `cpt-cf-usage-collector-fr-audit-trail`, `cpt-cf-usage-collector-nfr-throughput-profile`, `cpt-cf-usage-collector-nfr-capacity-headroom`, `cpt-cf-usage-collector-nfr-batch-and-report-timing`, and `cpt-cf-usage-collector-nfr-operational-visibility` rows in DESIGN.md [§5](#5-functional-requirements) to anchor the correlation-ID across ingestion, query, deactivation, and Metric lifecycle paths; ingestion-and-query throughput envelope; tenant fan-out, 24-month REST/SDK/Plugin-SPI contract-stability, and Metric-catalog headroom; ingestion-side batch; and AuthN/PDP signal realizations respectively.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 0.5.1   | 2026-05-23        | Bounded fix: replaced Rust/async-trait wording in `cpt-cf-usage-collector-fr-standards-compliance` with a language-neutral gear-binding statement; renamed the prior privacy-by-design FR to `cpt-cf-usage-collector-fr-privacy-controls` so the textual kind segment matches the parser-inferred kind; expanded PII / SPI / RPO / RTO / OWASP ASVS / DSR on first use and added matching [§1.4](#14-glossary) Glossary rows; folded the §2.3 Actor Permissions H3 into an unheaded shared block under [§2](#2-actors); collapsed §9.0 / §9.1 H3s into an unheaded preamble + bullets directly under [§9](#9-acceptance-criteria); demoted §14.1 to a bold paragraph label under [§14](#14-traceability); added explicit "Not applicable" [§6.2](#62-nfr-exclusions) entries for Device/Platform Requirements (UX-PRD-004) and Inclusivity Requirements (UX-PRD-005); added the 19 missing PRD → DESIGN traceability rows in DESIGN.md [§5](#5-functional-requirements).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 0.5.0   | 2026-05-23        | Phase 5 remediation: added §9.0 load and measurement definitions (load envelope, steady-state window, ±10% latency tolerance, linear-scaling efficiency ratio, burst tolerance); added acceptance criteria for `cpt-cf-usage-collector-fr-ingestion`, `cpt-cf-usage-collector-fr-gauge-semantics`, and `cpt-cf-usage-collector-nfr-plugin-contract-stability`; replaced "normal load" / "normal operation" / "linear throughput scaling" in `cpt-cf-usage-collector-nfr-throughput`, `cpt-cf-usage-collector-nfr-ingestion-latency`, `cpt-cf-usage-collector-nfr-query-latency`, `cpt-cf-usage-collector-nfr-scalability`, `cpt-cf-usage-collector-nfr-batch-and-report-timing`, and matching [§9](#9-acceptance-criteria) acceptance bullets with numeric profiles, tolerances, and references to the §9.0 definitions; added [§11](#11-assumptions) assumption pinning the §9.0 definitions as the single source of truth.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 0.4.0   | 2026-05-23        | Phase 4 remediation: added [§3.1](#31-gear-specific-environment-constraints) gear-specific environment posture; added seven gear-specific NFRs covering throughput profile, capacity headroom, availability boundary, batch and report timing, deployment operations, operational visibility, and error/recovery experience with measurable thresholds; added matching acceptance criteria in [§9](#9-acceptance-criteria), capacity-baseline and platform-operations assumptions in [§11](#11-assumptions), and capacity-headroom-overrun and rollback-risk rows in [§12](#12-risks).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 0.3.0   | 2026-05-23        | Phase 3 remediation: added [§5.8](#58-security-and-data-governance) Security and Data Governance with authentication delegation, data classification, audit trail delegation, non-repudiation, Privacy by Design application, data ownership/stewardship, data quality preservation, data lifecycle delegation, and standards/legal/compliance applicability requirements; expanded [§6.2](#62-nfr-exclusions) exclusions for consent management, DSR, and data sovereignty; added supporting assumptions, risks, glossary terms, and acceptance criteria.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 0.2.0   | 2026-05-23        | Phase 2 remediation: added measurable success baselines, targets, and timeframes; expanded GTS; added developer/operator experience, documentation, and support readiness requirements.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 0.1.0   | Before 2026-05-23 | Pre-remediation PRD baseline before phase 2 business, glossary, experience, documentation, and support updates.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
