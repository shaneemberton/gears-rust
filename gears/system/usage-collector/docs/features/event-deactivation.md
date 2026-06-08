# Feature: Event Deactivation

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
  - [1.5 Explicit Non-Applicability](#15-explicit-non-applicability)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
  - [Deactivate Record](#deactivate-record)
  - [Depth-1 Cascade on Usage-Row Deactivation](#depth-1-cascade-on-usage-row-deactivation)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Operator PDP Authorization](#operator-pdp-authorization)
  - [Monotonic Transition Dispatch](#monotonic-transition-dispatch)
  - [Atomic Transition Outcome Mapping](#atomic-transition-outcome-mapping)
  - [Atomic Cascade Flip](#atomic-cascade-flip)
  - [Cascade-vs-Compensation Concurrency Guard](#cascade-vs-compensation-concurrency-guard)
- [4. States (CDSL)](#4-states-cdsl)
  - [Usage Record Deactivation Lifecycle State Machine](#usage-record-deactivation-lifecycle-state-machine)
- [5. Definitions of Done](#5-definitions-of-done)
  - [FR: Event Deactivation](#fr-event-deactivation)
  - [FR: Usage Compensation (Cascade Cross-Link)](#fr-usage-compensation-cascade-cross-link)
  - [FR: Data Quality](#fr-data-quality)
  - [FR: Data Lifecycle](#fr-data-lifecycle)
  - [FR: Audit Trail](#fr-audit-trail)
  - [NFR: Authorization](#nfr-authorization)
  - [NFR: Availability](#nfr-availability)
  - [Principle: Monotonic Deactivation](#principle-monotonic-deactivation)
  - [Principle: Fail Closed](#principle-fail-closed)
  - [ADR: Monotonic Deactivation](#adr-monotonic-deactivation)
  - [ADR: Usage Compensation (Cascade Companion)](#adr-usage-compensation-cascade-companion)
  - [Constraint: No Business Logic](#constraint-no-business-logic)
  - [Component: Deactivation Handler](#component-deactivation-handler)
  - [Sequence: Deactivate Usage Event](#sequence-deactivate-usage-event)
  - [Data: usage_records Status Column](#data-usage_records-status-column)
  - [Entity: Usage Record](#entity-usage-record)
  - [Entity: Deactivation Status](#entity-deactivation-status)
  - [Entity: Entry Type](#entity-entry-type)
  - [Entity: Security Context](#entity-security-context)
  - [API: POST /usage-collector/v1/records/{id}/deactivate](#api-post-usage-collectorv1recordsiddeactivate)
  - [§2.5-item → DoD-ID Coverage Matrix](#25-item--dod-id-coverage-matrix)
- [6. Acceptance Criteria](#6-acceptance-criteria)

<!-- /toc -->

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-featstatus-event-deactivation`

<!-- reference to DECOMPOSITION entry -->

- [ ] `p2` - `cpt-cf-usage-collector-feature-event-deactivation`

## 1. Feature Context

### 1.1 Overview

Provides the PDP-authorized **cross-kind error retraction** path that **voids any erroneous row regardless of `entry_type`** — both `cpt-cf-usage-collector-entity-usage-record` rows where `entry_type = usage` and rows where `entry_type = compensation` (per `cpt-cf-usage-collector-entity-entry-type`) — by atomically flipping the targeted row's `status` column from `active` to `inactive` without mutating any other property, realizing immutability-via-deactivation rather than in-place edits or hard deletion. When the targeted row is `entry_type = usage`, the same atomic transition cascades **depth-1**: every active `compensation` row whose `corrects_id` references the targeted row is flipped to `inactive` in the same one-shot step, so `SUM` returns to the state it held before either the usage record or its compensations were accepted. The `cpt-cf-usage-collector-component-deactivation-handler` accepts the operator's `cpt-cf-usage-collector-entity-security-context` (resolved upstream by the ToolKit gateway on REST or supplied verbatim by the in-process caller on the SDK trait surface) and authorizes the deactivation through the per-component `authz_scope` helper that wraps `cpt-cf-usage-collector-contract-authz-resolver` fail-closed, then issues a status-only atomic transition (with depth-1 cascade when applicable) through the Plugin SPI's `transition_active_to_inactive` capability so the plugin enforces monotonicity and cascade atomicity at the storage layer. Inactive records remain queryable through the §2.4 Query Gateway, preserving auditable history for downstream consumers while the substrate stays free of mutable-record patterns.

**Atomicity scope (plugin-transaction invariant, NOT a cross-path guarantee).** The depth-1 cascade documented above commits as one **plugin backend transaction**: the primary row and every active referencing compensation row are flipped together inside a single backend transaction with no cross-replica protocol. That atomicity is the invariant `cpt-cf-usage-collector-adr-monotonic-deactivation` and `cpt-cf-usage-collector-adr-usage-compensation` bind on the Plugin SPI's `transition_active_to_inactive` capability. It is **NOT** a promise that a subsequent Query SPI read against any read pool observes the post-cascade state — visibility through `cpt-cf-usage-collector-feature-usage-query` is governed separately by `cpt-cf-usage-collector-nfr-query-freshness` and `cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011): eventually consistent with no upper bound at the gear floor, plugin-bound by the active plugin's published ceiling. Operators that need the post-cascade state for an immediate decision MUST consume the `DeactivationAck` returned by this feature (which carries `cascaded_compensation_ids`); they MUST NOT round-trip through a follow-up aggregated or raw query for that purpose. Full contract: DESIGN [§3.10.8](../DESIGN.md#3108-consistency-contract).

### 1.2 Purpose

This feature exists so that **error retraction** of previously accepted records — across both `entry_type` values (`usage` and `compensation`) — is expressed as a one-way `active → inactive` status transition (per `cpt-cf-usage-collector-adr-monotonic-deactivation`) rather than as in-place mutation, hard deletion, or reactivation, keeping the metering substrate free of mutable-record semantics that would break audit guarantees, retroactive query reproducibility, and idempotency-keyed re-emission. The single-row, status-only, atomic transition (with depth-1 cascade from a deactivated `usage` row to its active referencing compensations) is the only path that can mutate `cpt-cf-usage-collector-dbtable-usage-records.status` after acceptance. Deactivation is the **only** correction primitive for `gauge` records and for the `COUNT`/`MIN`/`MAX`/`AVG` aggregations on any kind; counter value-reversal that nets inside `SUM` is owned by the complementary compensation primitive (`cpt-cf-usage-collector-fr-usage-compensation`) on the unified ingestion path documented inline in `usage-emission.md`, not by this feature.

**Requirements**: `cpt-cf-usage-collector-fr-event-deactivation`, `cpt-cf-usage-collector-fr-usage-compensation`, `cpt-cf-usage-collector-fr-data-quality`, `cpt-cf-usage-collector-fr-data-lifecycle`, `cpt-cf-usage-collector-fr-audit-trail`, `cpt-cf-usage-collector-nfr-authorization`, `cpt-cf-usage-collector-nfr-availability`

**Principles**: `cpt-cf-usage-collector-principle-monotonic-deactivation`, `cpt-cf-usage-collector-principle-fail-closed`

### 1.3 Actors

| Actor                                            | Role in Feature                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-actor-platform-operator` | Authenticated platform operator who issues the deactivation request against a single previously emitted `cpt-cf-usage-collector-entity-usage-record` by supplying the target record `id` (path parameter) through `POST /usage-collector/v1/records/{id}/deactivate` or through the in-process SDK `deactivate_usage_record` operation; the operator's authority to deactivate the targeted record is verified by `cpt-cf-usage-collector-flow-foundation-pdp-authorize` against the resolved `cpt-cf-usage-collector-entity-security-context` per `cpt-cf-usage-collector-fr-event-deactivation` and PRD §8 `cpt-cf-usage-collector-usecase-deactivate-event`. |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md) -- Individual Event Deactivation §5.6 (`cpt-cf-usage-collector-fr-event-deactivation`), Data Quality Preservation §5.8 (`cpt-cf-usage-collector-fr-data-quality`), Data Lifecycle Delegation §5.8 (`cpt-cf-usage-collector-fr-data-lifecycle`), Audit Trail §5.8 (`cpt-cf-usage-collector-fr-audit-trail`), Authorization §6.2 (`cpt-cf-usage-collector-nfr-authorization`), Availability §6.1 (`cpt-cf-usage-collector-nfr-availability`), Deactivate a Usage Event use case §8 (`cpt-cf-usage-collector-usecase-deactivate-event`), Actor catalog §2 (Platform Operator)
- **Design**: [DESIGN.md](../DESIGN.md) -- Deactivation Handler component (§3.5 `cpt-cf-usage-collector-component-deactivation-handler`), Monotonic Deactivation principle (§2.1 `cpt-cf-usage-collector-principle-monotonic-deactivation`), Fail-closed principle (§2.1 `cpt-cf-usage-collector-principle-fail-closed`), Deactivate Usage Event sequence `cpt-cf-usage-collector-seq-deactivate-event` (§3.6), `usage_records` row shape and `status` column (§3.7 `cpt-cf-usage-collector-dbtable-usage-records`), Domain Model entities `UsageRecord` / `DeactivationStatus` / `SecurityContext` (§3.1), Endpoints Overview row for `POST /usage-collector/v1/records/{id}/deactivate` (§3.3), PRD→DESIGN realization rows for `fr-event-deactivation`, `fr-data-quality`, `fr-data-lifecycle`, `fr-audit-trail`, `nfr-authorization`, `nfr-availability` (§5.3)
- **Decomposition**: [DECOMPOSITION.md](../DECOMPOSITION.md) -- §2.5 Event Deactivation
- **Foundation feature**: [foundation.md](./foundation.md) -- SecurityContext acceptance at the surface boundaries (REST `Extension<SecurityContext>` from ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK trait methods accepting `ctx: &SecurityContext` as the first parameter), PDP enforcement via the per-component `authz_scope` helper (`cpt-cf-usage-collector-flow-foundation-pdp-authorize`), audit-correlation propagation (`cpt-cf-usage-collector-algo-foundation-audit-correlation`), plugin host binding, tenant isolation, fail-closed posture (reused, not re-defined)
- **Usage Emission feature**: [usage-emission.md](./usage-emission.md) -- sole writer of all columns of `cpt-cf-usage-collector-dbtable-usage-records` other than `status` (the `status` column is owned by Event Deactivation per DESIGN §3.7); also hosts the **inlined compensation flow** (counter value-reversal: `entry_type = compensation` + negative `value` + `corrects_id`) per `cpt-cf-usage-collector-fr-usage-compensation` and `cpt-cf-usage-collector-adr-usage-compensation` — deactivation targets exactly one row that the emission feature (either path) previously accepted, and cascades depth-1 to active compensations emitted through that inlined flow (reused, not re-defined)
- **Plugin SPI reference**: [plugin-spi.md](../plugin-spi.md) -- Method 5 (`transition_active_to_inactive`) atomic monotonic transition capability with depth-1 set-flip semantics and the `DeactivationOutcome` taxonomy (`Transitioned`, `AlreadyInactive`, `NotFound`)
- **SDK trait reference**: [sdk-trait.md](../sdk-trait.md) -- Method 5 (`deactivate_usage_record`) in-process operation, `DeactivationAck` shape (carrying `id` + `cascaded_compensation_ids`), and the `Authentication` / `Authorization` / `Validation` / `NotFound` / `AlreadyInactive` / `PluginUnavailable` / `PluginTimeout` / `PluginFailure` error variants
- **REST contract**: [usage-collector-v1.yaml](../usage-collector-v1.yaml) -- `POST /usage-collector/v1/records/{id}/deactivate` path, `DeactivateRecordRequest` / `DeactivateRecordResponse` schemas (response carries `id` + `cascaded_compensation_ids`), `context.reason="already_inactive"` discriminator
- **Domain model**: [domain-model.md](../domain-model.md) -- §2.2 `EntryType` (`cpt-cf-usage-collector-entity-entry-type`), §2.10 `DeactivationStatus` invariants (`active -> inactive` monotonicity, atomic transition, depth-1 cascade to active referencing compensations)
- **ADR cross-references**: `cpt-cf-usage-collector-adr-monotonic-deactivation` (rescoped to any-`entry_type` retraction with depth-1 cascade) and `cpt-cf-usage-collector-adr-usage-compensation` (the complementary counter value-reversal primitive that compensations cascade-deactivate alongside); `cpt-cf-usage-collector-adr-consistency-contract` (ADR-0011) — clarifies that the cascade atomicity recorded in §1.1 above is a plugin-transaction invariant, NOT a cross-path guarantee against subsequent Query SPI reads (see DESIGN [§3.10.8](../DESIGN.md#3108-consistency-contract))
- **Dependencies**: `cpt-cf-usage-collector-feature-foundation`, `cpt-cf-usage-collector-feature-usage-emission` (hosts the inlined compensation flow whose active rows are the cascade targets)

### 1.5 Explicit Non-Applicability

- **UX** (`UX-FDESIGN-001` user journey, `UX-FDESIGN-002` accessibility): Not applicable because the event-deactivation feature is a backend operator surface (`POST /usage-collector/v1/records/{id}/deactivate` plus the in-process SDK `deactivate_usage_record` operation routed through the same `cpt-cf-usage-collector-component-deactivation-handler`); there is no human-facing UI in this gear, the only direct caller is the authenticated platform operator (`cpt-cf-usage-collector-actor-platform-operator`), and any operator-facing tooling that surfaces deactivation lives outside this feature's scope. Operator developer experience is encoded through the deterministic `Problem` error envelopes published by `usage-collector-v1.yaml` (`already_inactive`, canonical `NotFound`, canonical `Unauthenticated`, canonical `PermissionDenied`, `plugin_readiness` for SPI faults).
- **Counter value-reversal (refunds, credits, credit-notes, partial releases)**: Not applicable to this feature. Deactivation is **error retraction**, not value-reversal — it voids a whole row from every aggregation. Caller-driven counter value-reversal (an append-only signed-negative entry that reduces `SUM` without retracting the original event) is owned by the **compensation primitive**, whose flow is **inlined into `features/usage-emission.md`** (no separate FEATURE file exists; compensation rides the same unified ingestion path as ordinary emission). See PRD FR `cpt-cf-usage-collector-fr-usage-compensation` and ADR `cpt-cf-usage-collector-adr-usage-compensation` for the contract; computing refunds, credits, credit-notes, or quota balances remains a downstream-consumer responsibility per the §3.10.3 un-policed-net note in DESIGN.
- **Bulk-by-query deactivation**: Not applicable per DECOMPOSITION §2.5 Out of scope — every deactivation targets exactly one record by `id`; multi-record selection by filter is explicitly out of scope and any such request shape is rejected by the OpenAPI contract before handler dispatch. (The depth-1 cascade flips multiple rows in a single atomic step, but the request still targets exactly one explicit `id`; cascaded compensation rows are selected by `corrects_id` referential identity, not by an arbitrary query filter.)
- **Compensating a compensation**: Not applicable per `cpt-cf-usage-collector-adr-usage-compensation` non-goals — `corrects_id` MUST reference a row with `entry_type = usage`, so a `compensation -> compensation` reference is structurally impossible; deactivating an `entry_type = compensation` row is therefore a **single-row, no-cascade** operation by construction.
- **Reactivation (`inactive → active`)**: Not applicable per `cpt-cf-usage-collector-adr-monotonic-deactivation` and `cpt-cf-usage-collector-principle-monotonic-deactivation` — the Usage Collector does not provide a reactivation operation, and the SPI capability surface deliberately exposes only the one-way `transition_active_to_inactive` per `plugin-spi.md` Method 5. The latch applies uniformly to both `entry_type = usage` and `entry_type = compensation` rows and to any compensation rows flipped by the depth-1 cascade.
- **Field edits**: Not applicable — no value, timestamp, metadata, tenant, resource, subject, Metric, idempotency-key, `entry_type`, `corrects_id`, or any column other than `status` is mutable after acceptance per DESIGN §3.7 ("`status` is transitioned exclusively by the Deactivation Handler via a status-only update; no other column is mutable after acceptance").
- **Negative-net detection / enforcement**: Not applicable. The Usage Collector does NOT validate non-negative `SUM` at write time and does NOT emit a negative-net signal when a depth-1 cascade leaves `SUM` at a non-negative value or when a future compensation drives `SUM` negative — see DESIGN §3.10.3 un-policed-net note. Downstream consumers own any "net can't be negative" policy.
- **Gear-local audit event emission for the deactivate operation**: Not applicable per DESIGN §3.9.5 and the §4 forward-looking note — authoritative audit is delegated to the platform gateway access log and PDP decision logs per `cpt-cf-usage-collector-fr-audit-trail`.

## 2. Actor Flows (CDSL)

### Deactivate Record

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:

- An authenticated platform operator submits a deactivation request for a previously emitted `cpt-cf-usage-collector-entity-usage-record` by `id` via `POST /usage-collector/v1/records/{id}/deactivate` or via the SDK `deactivate_usage_record(ctx, ...)` operation. The target record MAY be either `entry_type = usage` or `entry_type = compensation` per `cpt-cf-usage-collector-entity-entry-type` — the surface is identical and the operator does not pre-declare the entry type. On the REST surface the handler receives `Extension<SecurityContext>` populated upstream by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and delegates to the `UsageCollectorClientV1` SDK trait; on the in-process SDK surface the caller passes `ctx: &SecurityContext` as the first argument directly. Both entry points converge on `cpt-cf-usage-collector-component-deactivation-handler`. `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization` invokes the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) to authorize the deactivation, and `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch` invokes the Plugin SPI Method 5 `transition_active_to_inactive` capability against the target `id`; the capability runs the depth-1 cascade atomically per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` and returns `DeactivationOutcome::Transitioned` carrying `{ primary_id, cascaded_compensation_ids: [...] }`. The handler surfaces HTTP `200` with `DeactivateRecordResponse { id, cascaded_compensation_ids, status: "inactive" }` per `usage-collector-v1.yaml`. The `status` column of the targeted row AND every cascade-target row is now `inactive`; every other column on every affected row is byte-identical to its pre-call value. When the target row is `entry_type = compensation`, `cascaded_compensation_ids` is empty by construction (no row references a compensation).

**Error Scenarios**:

- Request arrives without a resolved `SecurityContext` (REST handler never invoked by the gateway middleware because authentication failed upstream, or SDK trait called without a `ctx` argument) — whole-request rejection via the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml` default response; the collector never synthesizes identity and no SPI dispatch occurs.
- PDP denies the operator's deactivation request — surfaced as the canonical `PermissionDenied` `Problem` envelope per `usage-collector-v1.yaml` default response; no SPI dispatch occurs and no state change.
- The plugin returns `DeactivationOutcome::AlreadyInactive` — surfaced as the actionable `Problem` envelope with `context.reason="already_inactive"` per `usage-collector-v1.yaml` and the SDK `AlreadyInactive` variant per `sdk-trait.md` Method 5; no state change.
- The plugin returns `DeactivationOutcome::NotFound` — surfaced as the canonical `NotFound` `Problem` envelope per `usage-collector-v1.yaml` and the SDK `NotFound` variant; no state change.
- Plugin SPI transport / readiness / persistence error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) — surfaced as the canonical `Problem` envelope with `context.reason="plugin_readiness"` and the audit-correlation context preserved through `cpt-cf-usage-collector-algo-foundation-audit-correlation`; no state change.

**Steps**:

1. [ ] - `p1` - Operator submits a deactivation request — on REST through `POST /usage-collector/v1/records/{id}/deactivate` (with the target `cpt-cf-usage-collector-entity-usage-record.id` as the path parameter); the REST handler receives `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and W3C audit-correlation headers — or on the SDK through `UsageCollectorClientV1::deactivate_usage_record(ctx, ...)` with `ctx: &SecurityContext` as the first parameter per `sdk-trait.md` Method 5 - `inst-deactivate-record-submit`
2. [ ] - `p1` - **IF** the REST handler receives no `Extension<SecurityContext>` (gateway middleware rejected the call upstream) or the SDK trait is invoked without a `ctx` argument **RETURN** the canonical `Unauthenticated` `Problem` envelope per `usage-collector-v1.yaml` default response; the collector never synthesizes identity per `cpt-cf-usage-collector-principle-fail-closed` - `inst-deactivate-record-missing-ctx`
3. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization` to authorize the deactivation through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper wrapping `cpt-cf-usage-collector-contract-authz-resolver`) against the inbound `cpt-cf-usage-collector-entity-security-context` and the deactivation attribution tuple (operator identity, target record `id`) - `inst-deactivate-record-pdp`
4. [ ] - `p1` - **IF** the operator-PDP-authorization algorithm returns `deny` **RETURN** the canonical `PermissionDenied` `Problem` envelope per `usage-collector-v1.yaml` default response without any further dispatch — no SPI dispatch occurs - `inst-deactivate-record-pdp-deny`
5. [ ] - `p1` - **TRY** dispatch the validated request via `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`, which invokes the Plugin SPI Method 5 `transition_active_to_inactive` capability against the target `id`; the capability runs the depth-1 cascade atomically per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` and returns a single `DeactivationOutcome` (`Transitioned { primary_id, cascaded_compensation_ids }`, `AlreadyInactive`, or `NotFound`) per `plugin-spi.md` Method 5 - `inst-deactivate-record-spi-dispatch`
6. [ ] - `p1` - **CATCH** Plugin SPI transport / readiness / persistence error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) - `inst-deactivate-record-spi-catch`
   1. [ ] - `p1` - **RETURN** the canonical `Problem` envelope with `context.reason="plugin_readiness"` per `usage-collector-v1.yaml` while preserving the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`; no state change occurs - `inst-deactivate-record-spi-fail`
7. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping` against the returned `DeactivationOutcome` to compose the response - `inst-deactivate-record-outcome-map`
8. [ ] - `p1` - **IF** the outcome-mapping algorithm returns `transitioned` **RETURN** HTTP `200` with `DeactivateRecordResponse { id, cascaded_compensation_ids, status: "inactive" }` per `usage-collector-v1.yaml` — `id` is the explicitly-deactivated row id (the SPI outcome's `primary_id`), `cascaded_compensation_ids` lists the active `entry_type = compensation` rows whose `corrects_id` referenced `id` and that were flipped to `inactive` in the same atomic step (empty when `id` is a compensation, or when no active compensations referenced it); propagate the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation` - `inst-deactivate-record-success`
9. [ ] - `p1` - **ELSE IF** the outcome-mapping algorithm returns `already-inactive` **RETURN** the `Problem` envelope with `context.reason="already_inactive"` per `usage-collector-v1.yaml` and the SDK `AlreadyInactive` variant per `sdk-trait.md` Method 5; no state change occurs - `inst-deactivate-record-already-inactive`
10. [ ] - `p1` - **ELSE** the outcome-mapping algorithm returns `not-found`; **RETURN** the canonical `NotFound` `Problem` envelope per `usage-collector-v1.yaml` and the SDK `NotFound` variant per `sdk-trait.md` Method 5; no state change occurs - `inst-deactivate-record-not-found`

### Depth-1 Cascade on Usage-Row Deactivation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-flow-event-deactivation-cascade`

**Actor**: `cpt-cf-usage-collector-actor-platform-operator`

**Success Scenarios**:

- An authenticated platform operator deactivates an `entry_type = usage` row R that has one or more active `entry_type = compensation` rows whose `corrects_id` references R. The Plugin SPI Method 5 `transition_active_to_inactive(R.id)` capability executes `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`: in a **single atomic transition** at the storage layer, R is flipped from `active` to `inactive` AND every active referencing compensation C with `C.corrects_id = R.id ∧ C.entry_type = compensation ∧ C.status = active ∧ same (tenant_id, metric_gts_id)` is flipped from `active` to `inactive`. The handler surfaces HTTP `200` with `DeactivateRecordResponse { id: R.id, cascaded_compensation_ids: [C1.id, C2.id, ...], status: "inactive" }` per `usage-collector-v1.yaml`. Post-cascade `SUM(value)` over `(tenant_id, metric_gts_id)` returns to the state it held before either R or its compensations were accepted; `COUNT`/`MIN`/`MAX`/`AVG` (which operate over `entry_type = usage` rows only) also no longer include R.
- The same operator surface, applied to an `entry_type = compensation` row C: the capability flips C only — **single-row, no cascade** — and surfaces `DeactivateRecordResponse { id: C.id, cascaded_compensation_ids: [], status: "inactive" }`. The depth-1 bound is structural: by `cpt-cf-usage-collector-adr-usage-compensation`, no row may reference a compensation via `corrects_id`, so there is no second hop.
- The same operator surface, applied to an `entry_type = usage` row with no active referencing compensations: the capability flips only that row and surfaces `cascaded_compensation_ids: []`.

**Error Scenarios**:

- The cascade transition fails partway in the storage layer (a single compensation flip rejected by an underlying constraint or a transient transport fault mid-step). The Plugin SPI Method 5 capability MUST surface this as `PluginUnavailable` / `BackendError` / `ContractViolation` per `plugin-spi.md` Method 5 atomicity obligation; the entire set-flip is reverted (or never committed), no row's `status` changes, and the handler returns `context.reason="plugin_readiness"` per `usage-collector-v1.yaml`. Partial cascades are structurally impossible because the cascade is one transaction.
- Concurrent compensation submission referencing R arriving while R is mid-deactivation: rejected by the L1 "referenced record must be active" check per `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`; the cascade itself observes only the set of compensations that were committed-active at transaction-start. See §3 Concurrency Guard.

**CDSL outcome shape** (logical; surface-specific spellings owned by sdk-trait.md / plugin-spi.md / usage-collector-v1.yaml per DESIGN §3.3):

```text
deactivate(<id>) -> {
  id:            <id>,                       # the explicitly-deactivated row (SPI outcome's `primary_id`)
  cascaded_compensation_ids:  [<compensation-id>, ...]    # depth-1 only:
                                             #   - non-empty iff entry_type(primary) = usage
                                             #     AND at least one active compensation
                                             #     references the primary id via corrects_id
                                             #   - empty list otherwise (incl. entry_type = compensation
                                             #     primary, which is single-row, no cascade)
}
```

**Steps**:

1. [ ] - `p1` - Receive the explicitly-deactivated `id` from `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record` after PDP `allow` - `inst-cascade-receive-id`
2. [ ] - `p1` - Invoke the Plugin SPI Method 5 capability `transition_active_to_inactive(id)` exactly once; the capability is the atomic boundary that scopes the cascade per `plugin-spi.md` Method 5 - `inst-cascade-spi-call`
3. [ ] - `p1` - **IF** `entry_type(primary) = usage` — the capability MUST atomically flip the primary row AND every active compensation C with `C.corrects_id = primary.id ∧ C.entry_type = compensation ∧ C.status = active ∧ same (tenant_id, metric_gts_id)` from `active` to `inactive` in the same transition - `inst-cascade-usage-set-flip`
   1. [ ] - `p1` - **RETURN** `DeactivationOutcome::Transitioned { primary_id: primary.id, cascaded_compensation_ids: [C.id for each cascaded compensation] }` — `cascaded_compensation_ids` is the (possibly empty) list of compensation ids flipped in the same step - `inst-cascade-usage-return`
4. [ ] - `p1` - **ELSE IF** `entry_type(primary) = compensation` — the capability flips ONLY the primary row; no cascade target search is performed because no row may reference a compensation via `corrects_id` per `cpt-cf-usage-collector-adr-usage-compensation` - `inst-cascade-compensation-single`
   1. [ ] - `p1` - **RETURN** `DeactivationOutcome::Transitioned { primary_id: primary.id, cascaded_compensation_ids: [] }` - `inst-cascade-compensation-return`
5. [ ] - `p1` - **CATCH** any storage-layer failure during the set-flip — partial cascade is structurally impossible because the transition is one transaction - `inst-cascade-fail`
   1. [ ] - `p1` - Propagate `PluginUnavailable` | `Timeout` | `BackendError` | `ContractViolation` per `plugin-spi.md` Method 5; the handler surfaces `context.reason="plugin_readiness"` and no row's `status` changes - `inst-cascade-fail-propagate`

## 3. Processes / Business Logic (CDSL)

### Operator PDP Authorization

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`

**Input**: an inbound `POST /usage-collector/v1/records/{id}/deactivate` REST request carrying the gateway-resolved `Extension<SecurityContext>`, the target `cpt-cf-usage-collector-entity-usage-record.id` (path parameter), and audit-correlation headers; OR an SDK `deactivate_usage_record(ctx, ...)` invocation carrying `ctx: &SecurityContext` as the first parameter.

**Output**: Either the structured envelope-level rejection code `deny` (when PDP denies the operator's deactivation request — the canonical `PermissionDenied` envelope is propagated), or `allow` with the (`cpt-cf-usage-collector-entity-security-context`, `cpt-cf-usage-collector-entity-pdp-decision`) pair attached for downstream stages; the audit-correlation context is propagated. This algorithm MUST NOT re-implement PDP logic — it invokes the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) and forwards the operator deactivation attribution tuple (operator identity from `SecurityContext`, target record `id`) to the PDP. Authentication is owned by the ToolKit gateway upstream of the REST handler and by the in-process caller on the SDK trait surface; the collector NEVER synthesizes identity and NEVER consults an authentication contract.

**Steps**:

1. [ ] - `p1` - Receive the inbound `SecurityContext` at the `cpt-cf-usage-collector-component-deactivation-handler` boundary — on REST as `Extension<SecurityContext>` from the gateway middleware, on SDK as the `ctx: &SecurityContext` first argument — along with the target record `id` - `inst-algo-pdp-receive-ctx`
2. [ ] - `p1` - **IF** no `SecurityContext` is present at the boundary **RETURN** `unauthenticated` per `cpt-cf-usage-collector-principle-fail-closed`; the collector never synthesizes identity and never forwards an unauthenticated request to the PDP - `inst-algo-pdp-no-ctx`
3. [ ] - `p1` - Invoke `cpt-cf-usage-collector-algo-foundation-audit-correlation` to open the server span and capture the `request-id` correlation pair so PDP and Plugin SPI dispatches share a single trace, reading `correlation_id` from the inbound `SecurityContext` per `cpt-cf-usage-collector-fr-audit-trail` - `inst-algo-pdp-correlate`
4. [ ] - `p1` - Compose the deactivation attribution tuple from the inbound `cpt-cf-usage-collector-entity-security-context` (operator principal and operator's tenant scope) and the request envelope (target record `id`); the source-gear / Metric `gts_id` fields of the standard attribution tuple are not applicable to operator deactivation and are omitted - `inst-algo-pdp-compose-tuple`
5. [ ] - `p1` - Invoke `cpt-cf-usage-collector-flow-foundation-pdp-authorize` via the per-component `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) with the (`cpt-cf-usage-collector-entity-security-context`, deactivation attribution tuple) pair to obtain the `cpt-cf-usage-collector-entity-pdp-decision` (`permit` or `deny`) per `cpt-cf-usage-collector-nfr-authorization` - `inst-algo-pdp-call`
6. [ ] - `p1` - **IF** the PDP helper returns `unreachable` **RETURN** `deny` per `cpt-cf-usage-collector-principle-fail-closed`; no cached decision is consulted and no permissive fallback is applied - `inst-algo-pdp-fail-closed`
7. [ ] - `p1` - **IF** the PDP decision is `deny` **RETURN** `deny` carrying the propagated platform-authorization envelope (canonical `PermissionDenied`) - `inst-algo-pdp-deny`
8. [ ] - `p1` - **RETURN** `allow` with the (`cpt-cf-usage-collector-entity-security-context`, `cpt-cf-usage-collector-entity-pdp-decision`) pair attached, propagating the audit-correlation context for the next stage - `inst-algo-pdp-allow`

### Monotonic Transition Dispatch

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`

**Input**: the validated target `cpt-cf-usage-collector-entity-usage-record.id` and the propagated audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`; the algorithm runs only after `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization` returned `allow`.

**Output**: Either a single `DeactivationOutcome` from the Plugin SPI Method 5 capability (`Transitioned { primary_id, cascaded_compensation_ids }`, `AlreadyInactive`, `NotFound` per `plugin-spi.md` Method 5) forwarded for outcome mapping, or a Plugin SPI error variant (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) propagated to the surrounding `CATCH` branch for `context.reason="plugin_readiness"` rejection. The depth-1 cascade (primary `entry_type = usage` row plus all active referencing compensations flipped together) is owned by `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` inside the SPI capability — this dispatch algorithm does not iterate, does not query for cascade targets, and does not split the call across multiple SPI invocations. The algorithm MUST NOT perform any local state cache, MUST NOT re-query the row for a status pre-check before dispatch (the SPI capability is the atomic boundary).

**Steps**:

1. [ ] - `p1` - Resolve the ClientHub-scoped Plugin SPI client through `cpt-cf-usage-collector-component-plugin-host` for the configured GTS instance binding owned by `cpt-cf-usage-collector-feature-foundation` - `inst-algo-dispatch-resolve-plugin`
2. [ ] - `p1` - Invoke the Plugin SPI Method 5 capability `transition_active_to_inactive(id)` exactly once with the target `id`; trace context is propagated via the ambient `tracing::Span` / OpenTelemetry context (no explicit `TraceContext` parameter,) per `plugin-spi.md` Method 5 - `inst-algo-dispatch-spi-call`
3. [ ] - `p1` - **TRY** await the single `DeactivationOutcome` from the plugin per `plugin-spi.md` Method 5 - `inst-algo-dispatch-await`
4. [ ] - `p1` - **CATCH** Plugin SPI error variant `PluginUnavailable` | `Timeout` | `BackendError` | `ContractViolation` per `plugin-spi.md` Method 5 (the SPI exposes no `Unready` variant; structural unavailability surfaces as `PluginUnavailable`) - `inst-algo-dispatch-catch`
   1. [ ] - `p1` - Propagate the error variant up to the surrounding `CATCH` in `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record` so the handler maps it to `context.reason="plugin_readiness"` per `usage-collector-v1.yaml` while preserving the audit-correlation context - `inst-algo-dispatch-propagate-error`
5. [ ] - `p1` - **RETURN** the `DeactivationOutcome` verbatim (one of `Transitioned { primary_id, cascaded_compensation_ids }`, `AlreadyInactive`, `NotFound`) to the calling flow for outcome mapping — `cascaded_compensation_ids` is populated by the storage-layer set-flip per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`; this algorithm does not compute it - `inst-algo-dispatch-return-outcome`

### Atomic Transition Outcome Mapping

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping`

**Input**: a single `DeactivationOutcome` returned by the Plugin SPI Method 5 capability (`Transitioned { primary_id, cascaded_compensation_ids }`, `AlreadyInactive`, or `NotFound` per `plugin-spi.md` Method 5) plus the target `cpt-cf-usage-collector-entity-usage-record.id` carried from the original request.

**Output**: a deterministic mapping into the calling flow's response branch — `transitioned` (compose HTTP `200` + `DeactivateRecordResponse { id, status, cascaded_compensation_ids }` where `id` is the SPI outcome's `primary_id`, per `usage-collector-v1.yaml`), `already-inactive` (compose `Problem` envelope with `context.reason="already_inactive"` per `usage-collector-v1.yaml` and the SDK `AlreadyInactive` error variant per `sdk-trait.md` Method 5), or `not-found` (compose canonical `NotFound` `Problem` envelope and the SDK `NotFound` error variant). The mapping MUST be 1:1 with the SPI outcome taxonomy — no other outcomes are recognized, and any unexpected variant is treated as `ContractViolation` at the dispatch stage rather than mapped here. `cascaded_compensation_ids` is forwarded verbatim from the SPI outcome; this algorithm does not enumerate, sort, or modify it.

**Steps**:

1. [ ] - `p1` - **IF** the `DeactivationOutcome` is `Transitioned { primary_id, cascaded_compensation_ids }` **RETURN** `transitioned` so the calling flow surfaces `DeactivateRecordResponse { id, status, cascaded_compensation_ids }` (where `id` is the SPI outcome's `primary_id`) per `usage-collector-v1.yaml`; this branch is the only path that may report a successful `active → inactive` transition, and `cascaded_compensation_ids` is forwarded verbatim - `inst-algo-outcome-transitioned`
2. [ ] - `p1` - **ELSE IF** the `DeactivationOutcome` is `AlreadyInactive` **RETURN** `already-inactive`; the calling flow composes the `Problem` envelope with `context.reason="already_inactive"` per `usage-collector-v1.yaml` and the SDK `AlreadyInactive` error variant per `sdk-trait.md` Method 5, preserving the no-reactivation invariant per `cpt-cf-usage-collector-principle-monotonic-deactivation` - `inst-algo-outcome-already-inactive`
3. [ ] - `p1` - **ELSE** the `DeactivationOutcome` is `NotFound`; **RETURN** `not-found`; the calling flow composes the canonical `NotFound` `Problem` envelope and the SDK `NotFound` error variant per `sdk-trait.md` Method 5 - `inst-algo-outcome-not-found`

### Atomic Cascade Flip

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`

**Input**: the explicitly-deactivated record id forwarded by `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch` to the Plugin SPI Method 5 capability. The primary row's `entry_type` is read inside the capability's atomic transition; the operator does not pre-declare it.

**Output**: a single `DeactivationOutcome` per `plugin-spi.md` Method 5:

- `Transitioned { primary_id, cascaded_compensation_ids }` — the primary row was flipped from `active` to `inactive` in this transition. `cascaded_compensation_ids` is the (possibly empty) list of `entry_type = compensation` row ids that were flipped from `active` to `inactive` in the **same** transition because their `corrects_id` referenced `primary_id`. The list is empty when `entry_type(primary) = compensation` (single-row, no cascade by construction) or when no active compensations referenced the primary.
- `AlreadyInactive` — the primary row was already `status = inactive` at transaction-start; no row's `status` changes; no cascade evaluation is performed.
- `NotFound` — no row with the given id exists in `(tenant_id, metric_gts_id)` scope visible to this transaction; no row's `status` changes.

The algorithm is a single atomic set-flip; no row's `status` may change without all of them changing together. Partial cascade is structurally impossible.

**Steps**:

1. [ ] - `p1` - **TRY** the following inside a single storage-layer atomic transition (the SPI Method 5 capability is the atomic boundary; no row's `status` may change in isolation) - `inst-algo-cascade-tx-begin`
   1. [ ] - `p1` - Read the primary row by `id`; **IF** absent **RETURN** `NotFound` - `inst-algo-cascade-read-primary`
   2. [ ] - `p1` - **IF** primary.status = `inactive` **RETURN** `AlreadyInactive` (no state change; no cascade evaluation; preserves the one-way `active → inactive` latch per `cpt-cf-usage-collector-principle-monotonic-deactivation`) - `inst-algo-cascade-already-inactive`
   3. [ ] - `p1` - Flip primary.status from `active` to `inactive` - `inst-algo-cascade-flip-primary`
   4. [ ] - `p1` - **IF** primary.entry_type = `usage` — select every row C such that `C.corrects_id = primary.id ∧ C.entry_type = compensation ∧ C.status = active ∧ C.tenant_id = primary.tenant_id ∧ C.metric_gts_id = primary.metric_gts_id`; flip each selected row's `status` from `active` to `inactive` **in the same transition** - `inst-algo-cascade-flip-companions`
      1. [ ] - `p1` - Collect the ids of the flipped compensations into `cascaded_compensation_ids` (order is unspecified; downstream consumers MUST NOT depend on ordering) - `inst-algo-cascade-collect-ids`
   5. [ ] - `p1` - **ELSE** (primary.entry_type = `compensation`) — set `cascaded_compensation_ids = []`; no companion lookup is performed because no row may reference a compensation per `cpt-cf-usage-collector-adr-usage-compensation` - `inst-algo-cascade-compensation-no-companions`
2. [ ] - `p1` - **CATCH** any storage-layer fault during the transaction — abort the entire transaction; no row's `status` is committed - `inst-algo-cascade-fail`
   1. [ ] - `p1` - **RETURN** the corresponding Plugin SPI error variant (`PluginUnavailable` | `Timeout` | `BackendError` | `ContractViolation`) per `plugin-spi.md` Method 5; the dispatch algorithm propagates this to the surrounding flow's `CATCH` branch - `inst-algo-cascade-fail-propagate`
3. [ ] - `p1` - **RETURN** `Transitioned { primary_id: primary.id, cascaded_compensation_ids }` — the transition committed atomically; every cascade target observed `status = active` at transaction-start, and every cascade target's `status` is `inactive` at transaction-commit - `inst-algo-cascade-return`

### Cascade-vs-Compensation Concurrency Guard

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`

**Input**: any compensation ingestion request submitted concurrently with the deactivation of the `usage` row R it references via `corrects_id`. The guard is documented from the deactivation feature's vantage point but the L1 check is enforced inside the ingestion path inlined in `usage-emission.md` (the request never reaches this feature's handler).

**Output**: the verbatim guarantee that no compensation submission can be admitted after R leaves `active`, even under concurrent submission and deactivation.

**Concurrency rule (verbatim, from the locked decision in plan.toml)**:

> A compensation submission referencing R that arrives while R is being deactivated is rejected by the L1 "referenced record must be active" check; state ordering and atomicity of the cascade transition guarantee that no compensation can be admitted after R leaves `active`.

**Steps**:

1. [ ] - `p1` - The L1 `corrects_id` referential check on the ingestion path inlined in `usage-emission.md` (per `cpt-cf-usage-collector-fr-usage-compensation` and `cpt-cf-usage-collector-adr-usage-compensation`) reads the referenced row's `(entry_type, status, tenant_id, metric_gts_id)` and admits the compensation only when `exists ∧ entry_type = usage ∧ same (tenant_id, metric_gts_id) ∧ status = active`. A row mid-deactivation either still reports `status = active` (the deactivation transaction has not yet committed) or already reports `status = inactive` (the deactivation transaction has committed). The L1 check observes one of these two states; there is no observable intermediate state - `inst-algo-concurrency-l1`
2. [ ] - `p1` - **IF** the L1 check observes `status = inactive` (deactivation already committed) the compensation is rejected per `cpt-cf-usage-collector-fr-usage-compensation`; no row mutation occurs - `inst-algo-concurrency-reject-inactive`
3. [ ] - `p1` - **IF** the L1 check observes `status = active` but the deactivation transaction is still in flight — the storage layer's transactional ordering ensures one of two terminal outcomes: either (a) the compensation insert serialises **before** the deactivation transaction commits and the deactivation's cascade query observes that compensation as `active` and includes it in `cascaded_compensation_ids`, or (b) the compensation insert serialises **after** the deactivation commit and the L1 re-read (or the storage-layer concurrency control) sees `status = inactive` and rejects the compensation. There is no third option: no compensation can be admitted referencing a row that has already left `active` - `inst-algo-concurrency-serialise`
4. [ ] - `p1` - **RETURN** the locked invariant: state ordering and atomicity of the cascade transition guarantee that no compensation can be admitted after R leaves `active`. This guard adds no new lock or coordinator — it depends only on the L1 check and the atomicity of `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` - `inst-algo-concurrency-return`

## 4. States (CDSL)

### Usage Record Deactivation Lifecycle State Machine

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**States**: `Active`, `Inactive`

**Initial State**: `Active` (every accepted `cpt-cf-usage-collector-entity-usage-record` — whether `entry_type = usage` or `entry_type = compensation` — enters `Active` on ingestion per the unified ingestion path inlined in `features/usage-emission.md`; the emission feature is the only writer that creates rows in `cpt-cf-usage-collector-dbtable-usage-records`).

**Transition table** (cascade-aware; a single atomic SPI transition may flip multiple rows together):

| Source rows                                                                                      | Trigger                                                         | Atomic effect (one transition)                                                                                                                                                             | Returned outcome                                                    |
| ------------------------------------------------------------------------------------------------ | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| Primary row P (any `entry_type`); P.status = `Active`                                            | `deactivate(P.id)` — Plugin SPI Method 5 returns `Transitioned` | P.status flips `Active → Inactive`; if `entry_type(P) = usage`, every active referencing compensation C (`C.corrects_id = P.id`) ALSO flips `Active → Inactive` in the **same** transition | `Transitioned { id: P.id, cascaded_compensation_ids: [C.id, ...] }` |
| Primary row P; P.status = `Active`; `entry_type(P) = usage`; no active referencing compensations | `deactivate(P.id)`                                              | P.status flips `Active → Inactive` only; no cascade selection yields rows                                                                                                                  | `Transitioned { id: P.id, cascaded_compensation_ids: [] }`          |
| Primary row P; P.status = `Active`; `entry_type(P) = compensation`                               | `deactivate(P.id)`                                              | P.status flips `Active → Inactive`; no cascade evaluation (no row references a compensation per `cpt-cf-usage-collector-adr-usage-compensation`)                                           | `Transitioned { id: P.id, cascaded_compensation_ids: [] }`          |
| Primary row P; P.status = `Inactive`                                                             | `deactivate(P.id)`                                              | No row's `status` changes (one-way latch); no cascade evaluation                                                                                                                           | `AlreadyInactive`                                                   |
| No row with given id in the operator's tenant scope                                              | `deactivate(<id>)`                                              | No row's `status` changes                                                                                                                                                                  | `NotFound`                                                          |

**Transitions** (CDSL):

1. [ ] - `p1` - **FROM** `Active` **TO** `Inactive` **WHEN** the Plugin SPI Method 5 `transition_active_to_inactive` capability returns `DeactivationOutcome::Transitioned` for the target primary `id`; the transition is atomic at the storage layer per `plugin-spi.md` Method 5 atomicity obligation and the depth-1 cascade defined by `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`, and no other column of `cpt-cf-usage-collector-dbtable-usage-records` is mutated on any row per DESIGN §3.7 ("`status` is transitioned exclusively by the Deactivation Handler via a status-only update; no other column is mutable after acceptance") - `inst-state-active-to-inactive`
2. [ ] - `p1` - **FROM** `Active` **TO** `Inactive` (**CASCADE COMPANIONS**, same atomic transition as the primary flip) **WHEN** the primary is `entry_type = usage` and the storage-layer set-flip selects companion rows by `C.corrects_id = primary.id ∧ C.entry_type = compensation ∧ C.status = active ∧ same (tenant_id, metric_gts_id)`; every selected companion's `status` flips `Active → Inactive` in the **same** atomic transition as the primary, and the ids of the flipped companions are returned in `cascaded_compensation_ids`. Partial cascade is structurally impossible — the entire set-flip commits together or not at all - `inst-state-cascade-companions`
3. [ ] - `p1` - **FROM** `Inactive` **TO** `Inactive` **WHEN** a subsequent deactivation request targets the same `id` — the Plugin SPI Method 5 capability MUST return `DeactivationOutcome::AlreadyInactive` (no state change; no cascade re-evaluation) per `plugin-spi.md` Method 5, and the handler surfaces `context.reason="already_inactive"` per `usage-collector-v1.yaml`; this is the no-op self-edge that realizes monotonicity at the SPI boundary per `cpt-cf-usage-collector-principle-monotonic-deactivation` and applies uniformly to both `entry_type` values - `inst-state-inactive-self-loop`
4. [ ] - `p1` - **NO TRANSITION FROM** `Inactive` **TO** `Active` exists for any `entry_type` — the Usage Collector does not provide a reactivation operation per `cpt-cf-usage-collector-adr-monotonic-deactivation`, the Plugin SPI Method 5 capability surface deliberately exposes only the one-way `transition_active_to_inactive` per `plugin-spi.md` Method 5, the one-way latch applies to primary rows AND to cascade-flipped compensation rows alike, and any caller-side attempt to re-introduce the inverse path is structurally impossible on the contract surface published by `usage-collector-v1.yaml` and `sdk-trait.md` - `inst-state-no-reactivation`

## 5. Definitions of Done

### FR: Event Deactivation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-fr-event-deactivation`

The system **MUST** support deactivating an individual `cpt-cf-usage-collector-entity-usage-record` by `id` — regardless of `entry_type` (`usage` or `compensation` per `cpt-cf-usage-collector-entity-entry-type`) — through `POST /usage-collector/v1/records/{id}/deactivate` (REST) and the SDK `deactivate_usage_record` operation (in-process) — both routed through `cpt-cf-usage-collector-component-deactivation-handler` — by transitioning the target row's `status` column from `active` to `inactive` while leaving every other column byte-identical to its pre-call value. When the target row is `entry_type = usage`, the same atomic transition cascades depth-1 to every active `entry_type = compensation` row whose `corrects_id` references the target, flipping every selected row's `status` from `active` to `inactive` in the **same** atomic step per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` and returning `cascaded_compensation_ids` in the response. Deactivation MUST be one-way (no reactivation operation exists for any `entry_type`) and second deactivation against an already-inactive record MUST be rejected with `context.reason="already_inactive"` per `usage-collector-v1.yaml`.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-flow-event-deactivation-cascade`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`

**Constraints**: `cpt-cf-usage-collector-fr-event-deactivation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records` (status column only)
- Entities: `UsageRecord`, `DeactivationStatus`, `EntryType`, `SecurityContext`

### FR: Usage Compensation (Cascade Cross-Link)

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-fr-usage-compensation`

The system **MUST** honor the cascade obligation that `cpt-cf-usage-collector-fr-usage-compensation` imposes on the deactivation feature: when an operator deactivates a `usage` row R that has one or more active `entry_type = compensation` rows referencing it via `corrects_id`, the Plugin SPI Method 5 capability MUST flip R **and** every such active compensation from `active` to `inactive` in the **same** atomic transition per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`, and the response MUST surface the list of flipped compensation ids as `cascaded_compensation_ids` so callers can reconcile their downstream ledgers without re-querying. The compensation primitive itself (counter value-reversal: caller-driven, append-only, signed-negative `value` on the unified ingestion path) is **not implemented by this feature** — its flow is inlined into `features/usage-emission.md` per the `feature_doc_shape = inline-in-emission` decision; this DoD only realises the cascade leg that deactivation owes to compensation rows. Compensating a compensation is a non-goal per `cpt-cf-usage-collector-adr-usage-compensation`, so deactivating an `entry_type = compensation` row is structurally single-row (no cascade).

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-cascade`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`
- `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-fr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records` (status column only; for primary row + all cascade-flipped companion compensation rows)
- Entities: `UsageRecord`, `DeactivationStatus`, `EntryType`

### FR: Data Quality

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-fr-data-quality`

The system **MUST** preserve the data-quality posture of the metering substrate on the deactivation path by keeping corrections monotonic via the one-way `active → inactive` status transition rather than via in-place mutation, hard deletion, or reactivation per `cpt-cf-usage-collector-principle-monotonic-deactivation`; `cpt-cf-usage-collector-component-deactivation-handler` MUST NOT mutate any column of `cpt-cf-usage-collector-dbtable-usage-records` other than `status`, MUST NOT silently amend the targeted record's `value`, `timestamp`, attribution tuple (`tenant_id`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source_gear`, `metric_gts_id`), `idempotency_key`, or `metadata`, and MUST surface every rejected request (PDP deny, already-inactive target, not-found target, plugin readiness fault) as a deterministic `Problem` envelope without leaving the row in a partially-updated state per the SPI atomicity obligation in `plugin-spi.md` Method 5.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-fr-data-quality`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### FR: Data Lifecycle

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-fr-data-lifecycle`

The system **MUST** preserve queryability of inactive records after the `active → inactive` transition — `cpt-cf-usage-collector-component-deactivation-handler` MUST NOT delete the row, MUST NOT hide it from the §2.4 Query Gateway, and MUST NOT exempt it from the active storage plugin's retention / archival / purge policies per `cpt-cf-usage-collector-fr-data-lifecycle` and `cpt-cf-usage-collector-adr-pluggable-storage`; the §2.4 Query Gateway reads both `active` and `inactive` rows from `cpt-cf-usage-collector-dbtable-usage-records` within the PDP-authorized scope, and distinguishing the two values is the caller's responsibility per DECOMPOSITION §2.4 "Active-and-inactive record visibility" and the `DeactivationStatus` invariants in `domain-model.md` §2.9. Physical retention, archival, and purge are owned by the active storage plugin's deployment profile, not by the deactivation handler.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-fr-data-lifecycle`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### FR: Audit Trail

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-fr-audit-trail`

The system **MUST** propagate a request-level correlation identifier (per `cpt-cf-usage-collector-fr-audit-trail`) through the deactivation flow so the platform gateway access log and the `cpt-cf-usage-collector-contract-authz-resolver` PDP decision logs can be reconciled with gear-level deactivation activity; `cpt-cf-usage-collector-component-deactivation-handler` MUST invoke `cpt-cf-usage-collector-algo-foundation-audit-correlation` at the request boundary to capture the `request-id` correlation pair (reading `correlation_id` from the inbound `cpt-cf-usage-collector-entity-security-context` supplied by the ToolKit gateway on REST or by the in-process caller on the SDK trait). Gear-local audit-ledger emission for the deactivate operation is deferred per DESIGN §3.9.5 and the §4 forward-looking note.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-fr-audit-trail`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `SecurityContext`

### NFR: Authorization

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-nfr-authorization`

The system **MUST** authorize every deactivation request through the per-component `authz_scope` helper inside `cpt-cf-usage-collector-component-deactivation-handler` (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) against the inbound `cpt-cf-usage-collector-entity-security-context` and the deactivation attribution tuple (operator identity, target record `id`) before any Plugin SPI dispatch per `cpt-cf-usage-collector-nfr-authorization` and `cpt-cf-usage-collector-principle-pdp-centric-authorization`; PDP denials surface as the canonical `PermissionDenied` `Problem` envelope per `usage-collector-v1.yaml` and no SPI dispatch occurs. The handler MUST NOT cache PDP decisions across requests, MUST NOT synthesize operator identities when a `SecurityContext` is absent (the REST handler rejects any call without `Extension<SecurityContext>` from the gateway middleware; the SDK trait requires `ctx: &SecurityContext` as the first parameter), and MUST NOT permit any deactivation-specific bypass of the per-component authorization gate.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-nfr-authorization`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- Component: `cpt-cf-usage-collector-component-deactivation-handler`
- Entities: `SecurityContext`, `PdpDecision`

### NFR: Availability

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-nfr-availability`

The system **MUST** keep the deactivation endpoint available within the PRD-declared availability budget (99.95% monthly per `cpt-cf-usage-collector-nfr-availability`) by running `cpt-cf-usage-collector-component-deactivation-handler` inside the same stateless `cpt-cf-usage-collector-topology-gear-runtime` instances that serve ingestion and query, by reaching durable state exclusively through the ClientHub-bound plugin via `cpt-cf-usage-collector-component-plugin-host`, and by surfacing every Plugin SPI transport / readiness / persistence error as a deterministic `Problem` envelope with `context.reason="plugin_readiness"` so callers can retry idempotently — the same `id` re-submitted after a transient SPI fault is structurally idempotent because the Plugin SPI Method 5 capability returns `DeactivationOutcome::AlreadyInactive` (not `Transitioned`) on the retry that follows a successful prior transition. The handler MUST NOT serve a parallel cache and MUST NOT invent a binding when the plugin host is unreachable.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`

**Constraints**: `cpt-cf-usage-collector-nfr-availability`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### Principle: Monotonic Deactivation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-principle-monotonic-deactivation`

The system **MUST** realize `cpt-cf-usage-collector-principle-monotonic-deactivation` end-to-end on the deactivation path — `cpt-cf-usage-collector-component-deactivation-handler` MUST issue exactly the one-way `Active → Inactive` `status` transition through the Plugin SPI Method 5 capability, MUST NOT mutate any other column of `cpt-cf-usage-collector-dbtable-usage-records`, MUST NOT expose any reactivation operation in either the REST surface (`usage-collector-v1.yaml`) or the SDK trait surface (`sdk-trait.md`), and MUST reject second deactivation against an already-inactive record with `context.reason="already_inactive"` per `usage-collector-v1.yaml` — preserving the substrate's freedom from mutable-record semantics so storage plugins, query consumers, and aggregation pipelines can reason about active/inactive as a first-class monotonic lifecycle event.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping`

**Constraints**: `cpt-cf-usage-collector-principle-monotonic-deactivation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### Principle: Fail Closed

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-principle-fail-closed`

The system **MUST** realize `cpt-cf-usage-collector-principle-fail-closed` on the deactivation path — `cpt-cf-usage-collector-component-deactivation-handler` MUST treat the absence of an inbound `cpt-cf-usage-collector-entity-security-context` as `unauthenticated` (returning the canonical `Unauthenticated` `Problem` envelope; on REST this occurs when the ToolKit gateway middleware did not populate `Extension<SecurityContext>`, on SDK it occurs when the trait method was invoked without a `ctx` argument), MUST treat `cpt-cf-usage-collector-contract-authz-resolver` unavailability as `deny` (returning the canonical `PermissionDenied` `Problem` envelope) without consulting any cached decision and without applying any permissive fallback, MUST treat Plugin SPI unavailability (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) as `context.reason="plugin_readiness"` rejection without inferring a successful transition, and MUST NEVER synthesize an operator identity, invent a plugin binding, or fabricate a `DeactivationOutcome` when any downstream collaborator is unreachable per DECOMPOSITION §2.5 "Fail-closed posture".

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`

**Constraints**: `cpt-cf-usage-collector-principle-fail-closed`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- Component: `cpt-cf-usage-collector-component-deactivation-handler`, `cpt-cf-usage-collector-component-plugin-host`
- Entities: `SecurityContext`, `PdpDecision`

### ADR: Monotonic Deactivation

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-adr-monotonic-deactivation`

The system **MUST** honor `cpt-cf-usage-collector-adr-monotonic-deactivation` by exposing exactly one lifecycle transition (`active → inactive`) through exactly one capability surface (`POST /usage-collector/v1/records/{id}/deactivate` plus the SDK `deactivate_usage_record` operation) routed through exactly one component (`cpt-cf-usage-collector-component-deactivation-handler`) backed by exactly one Plugin SPI capability (`transition_active_to_inactive` per `plugin-spi.md` Method 5); the system MUST NOT introduce a reactivation operation, MUST NOT introduce a bulk-by-query deactivation operation, MUST NOT introduce a field-edit operation that mutates any column other than `status`, and MUST NOT introduce a hard-delete operation for `cpt-cf-usage-collector-dbtable-usage-records` rows — the storage plugin owns physical retention / archival / purge per `cpt-cf-usage-collector-adr-pluggable-storage`, and corrections beyond the monotonic deactivation pattern are expressed as a deactivation plus a fresh idempotency-keyed re-emission per DESIGN §3.9.5 correction posture.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-adr-monotonic-deactivation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### ADR: Usage Compensation (Cascade Companion)

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-adr-usage-compensation`

The system **MUST** honor `cpt-cf-usage-collector-adr-usage-compensation` on the deactivation path by recognising that compensations are independent first-class rows (`entry_type = compensation`, signed-negative `value`, `corrects_id` referencing an active `entry_type = usage` row in the same `(tenant_id, metric_gts_id)`) ingested through the unified path inlined in `features/usage-emission.md` — and by flipping every active referencing compensation alongside a deactivated `usage` row in the depth-1 cascade per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`. The feature MUST NOT introduce a dedicated compensate REST path, SDK method, or Plugin SPI call (the unified ingestion path is the sole compensation surface per `cpt-cf-usage-collector-adr-usage-compensation`), MUST NOT validate or enforce non-negative `SUM` at deactivation time (the un-policed-net posture per DESIGN §3.10.3 is preserved), and MUST NOT permit a `compensation -> compensation` reference (deactivating a compensation is single-row, no cascade, per the ADR's compensating-a-compensation non-goal).

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-cascade`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`
- `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-adr-usage-compensation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`, `EntryType`

### Constraint: No Business Logic

- [ ] `p2` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-constraint-no-business-logic`

The system **MUST** keep the deactivation path free of billing, pricing, quota enforcement, per-Metric accounting transforms, and per-tenant business-rule interpretation per `cpt-cf-usage-collector-constraint-no-business-logic`; `cpt-cf-usage-collector-component-deactivation-handler` MUST NOT consult any per-Metric or per-tenant pricing table, MUST NOT trigger a counter rollback or gauge recomputation as a side-effect of deactivation (downstream consumers MUST recompute aggregates by excluding `inactive` rows themselves), and MUST NOT mutate the `value` column or any other column other than `status` on the targeted row or on any cascade-flipped compensation row. Business logic — billing reversal, quota credit, customer-facing notifications — is owned by source gears and downstream consumers, never by the metering substrate.

**Recording-not-computing (symmetric with `+value` recording, cross-reference to the compensation primitive)**: deactivation **records** a caller-supplied retraction action (an operator-initiated `Active → Inactive` flip plus the depth-1 cascade derived deterministically from `corrects_id` referential identity); it does **not** compute the financial consequence of that retraction. The same recording-not-computing posture governs the complementary compensation primitive on the unified ingestion path: a caller-supplied `entry_type = compensation` row with a strictly-negative `value` is **recorded** verbatim (symmetric with a `+value` `entry_type = usage` row) and the collector does NOT validate non-negative net at write time and does NOT emit a negative-net detection signal. See `cpt-cf-usage-collector-fr-usage-compensation`, `cpt-cf-usage-collector-adr-usage-compensation`, and the §3.10.3 un-policed-net note in DESIGN. The compensation flow is **inlined into `features/usage-emission.md`** — no separate FEATURE file exists.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-flow-event-deactivation-cascade`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`

**Constraints**: `cpt-cf-usage-collector-constraint-no-business-logic`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### Component: Deactivation Handler

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-component-deactivation-handler`

The system **MUST** realize `cpt-cf-usage-collector-component-deactivation-handler` as the sole synchronous entry point for status-only deactivation of `cpt-cf-usage-collector-entity-usage-record` rows (REST and SDK), owning the deactivation contract end-to-end — SecurityContext acceptance at both entry points (REST handler with `Extension<SecurityContext>` from ToolKit gateway middleware via `OperationBuilder::authenticated()`; SDK trait `deactivate_usage_record(ctx, ...)` with `ctx: &SecurityContext` as the first parameter), per-component PDP enforcement via the `authz_scope` helper (`PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`), Plugin SPI Method 5 dispatch, atomic-outcome mapping into `DeactivateRecordResponse` or the actionable error envelopes — while delegating persistence to `cpt-cf-usage-collector-component-plugin-host`, with no field-edit capabilities, no reactivation path, no record deletion, no PDP-decision caching, no synthesized identities, and no invented plugin bindings per DESIGN §3.5 Deactivation Handler component description.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping`

**Constraints**: `cpt-cf-usage-collector-component-deactivation-handler`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### Sequence: Deactivate Usage Event

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-seq-deactivate-event`

The system **MUST** implement the `cpt-cf-usage-collector-seq-deactivate-event` sequence end-to-end per DESIGN §3.6: operator surface (REST handler receiving `Extension<SecurityContext>` from ToolKit gateway middleware, or SDK trait `deactivate_usage_record(ctx, ...)` with `ctx: &SecurityContext` first) → Deactivation Handler PDP authorization via the per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver` → Deactivation Handler dispatch → Plugin Host → storage plugin `transition_active_to_inactive` against the target `id` → atomic outcome (`Transitioned` | `already-inactive` | `not-found`) → deterministic operator response; PDP denial, already-inactive target, not-found target, and SPI errors all reject before any column other than `status` is touched, and inactive records remain queryable through the §2.4 Query Gateway as required by the sequence description and the `cpt-cf-usage-collector-fr-data-lifecycle` cross-reference.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-seq-deactivate-event`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`

### Data: usage_records Status Column

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-dbtable-usage-records`

The system **MUST** touch `cpt-cf-usage-collector-dbtable-usage-records` from the deactivation path exclusively via a status-only atomic update against a single row identified by `id` — transitioning the `status` enum column from `active` to `inactive` per DESIGN §3.7 ("`status` is transitioned exclusively by the Deactivation Handler via a status-only update; no other column is mutable after acceptance") — and MUST NOT mutate, append to, or rewrite any other column (`tenant_id`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, `metadata`), preserving the row's append-only-after-acceptance invariant declared by `cpt-cf-usage-collector-feature-usage-emission` (the sole writer of all other columns). The atomicity obligation is enforced at the storage layer per `plugin-spi.md` Method 5 so two concurrent deactivation requests cannot both observe `Active` and both proceed.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`

**Constraints**: `cpt-cf-usage-collector-dbtable-usage-records`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### Entity: Usage Record

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-entity-usage-record`

The system **MUST** treat `cpt-cf-usage-collector-entity-usage-record` on the deactivation path as an append-only-after-acceptance entity whose only mutable surface is the `status` column governed by `cpt-cf-usage-collector-entity-deactivation-status` per DESIGN §3.1; `cpt-cf-usage-collector-component-deactivation-handler` MUST NOT instantiate, re-validate, or rewrite any other field of the targeted entity, MUST NOT generate a new `id` (the SPI capability accepts the existing `id` as input), and MUST forward exactly `id` through the Plugin SPI Method 5 capability per `plugin-spi.md` Method 5 ("Structural inputs: the target `UsageRecord.id`"). The persisted post-call row carries the same `tenant_id`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, and `metadata` it carried before the call.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-algo-event-deactivation-monotonic-transition-dispatch`

**Constraints**: `cpt-cf-usage-collector-entity-usage-record`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`

### Entity: Deactivation Status

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-entity-deactivation-status`

The system **MUST** treat `cpt-cf-usage-collector-entity-deactivation-status` per DESIGN §3.1 and `domain-model.md` §2.9 as a closed two-valued lifecycle marker (`active`, `inactive`) bound to a `cpt-cf-usage-collector-entity-usage-record` whose only permitted transition is `active → inactive`; `cpt-cf-usage-collector-component-deactivation-handler` MUST set the value via the Plugin SPI Method 5 atomic capability (no client-side write, no read-modify-write loop), MUST surface `Active → Inactive` as `DeactivationStatus.inactive` in `DeactivateRecordResponse.status` per `usage-collector-v1.yaml`, MUST surface `AlreadyInactive` outcomes as the actionable `context.reason="already_inactive"` error envelope (preserving the no-reactivation invariant per `cpt-cf-usage-collector-principle-monotonic-deactivation`), and MUST NEVER produce a `DeactivationStatus.active` value as the post-call state of a successful transition.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-outcome-mapping`

**Constraints**: `cpt-cf-usage-collector-entity-deactivation-status`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `DeactivationStatus`

### Entity: Entry Type

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-entity-entry-type`

The system **MUST** treat `cpt-cf-usage-collector-entity-entry-type` per domain-model.md §2.2 as the discriminator that determines deactivation's behaviour on the targeted row: when `entry_type = usage`, the depth-1 cascade selects active referencing compensations and flips them in the same atomic transition; when `entry_type = compensation`, deactivation is single-row and no cascade evaluation is performed. The discriminator is read **inside** the Plugin SPI Method 5 atomic transition by `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` — the operator does NOT pre-declare the entry type at the surface, and the handler does NOT branch on `entry_type` before SPI dispatch. The `entry_type` column itself is never mutated by this feature (the `Field edits` non-applicability bullet in §1.5 explicitly forbids it).

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-cascade`
- `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip`
- `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`

**Constraints**: `cpt-cf-usage-collector-entity-entry-type`

**Touches**:

- DB: `cpt-cf-usage-collector-dbtable-usage-records` (read-only on `entry_type`; write-only on `status`)
- Entities: `EntryType`, `UsageRecord`

### Entity: Security Context

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-entity-security-context`

The system **MUST** consume `cpt-cf-usage-collector-entity-security-context` per DESIGN §3.1 as the platform-resolved caller-identity envelope (operator principal, operator's tenant scope, auxiliary claims) — never owned, synthesized, or cached by `cpt-cf-usage-collector-component-deactivation-handler`. The handler MUST accept the `SecurityContext` exclusively at one of the two convention-bound entry points — on REST as `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and on the SDK trait as `ctx: &SecurityContext` passed as the first parameter to `deactivate_usage_record(ctx, ...)` — and pass it verbatim to `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper invoking `PolicyEnforcer::access_scope_with(ctx, ...)` against `cpt-cf-usage-collector-contract-authz-resolver`) so PDP authorizes the operator's identity against the deactivation attribution tuple (operator identity, target record `id`), and fail closed on missing `SecurityContext` or PDP unavailability per `cpt-cf-usage-collector-principle-fail-closed`. The `cpt-cf-usage-collector-entity-security-context` is the subject of PDP authorization for the deactivation request — no operator role table is held gear-local per DESIGN §3.9.4 ABAC-anchored authorization.

**Implements**:

- `cpt-cf-usage-collector-flow-foundation-pdp-authorize`
- `cpt-cf-usage-collector-algo-event-deactivation-operator-pdp-authorization`

**Constraints**: `cpt-cf-usage-collector-entity-security-context`

**Touches**:

- Component: `cpt-cf-usage-collector-component-deactivation-handler`
- Entities: `SecurityContext`

### API: POST /usage-collector/v1/records/{id}/deactivate

- [ ] `p1` - **ID**: `cpt-cf-usage-collector-dod-event-deactivation-api-post-records-id-deactivate`

The system **MUST** expose `POST /usage-collector/v1/records/{id}/deactivate` as the sole REST entry point for individual usage-record deactivation per `usage-collector-v1.yaml`, with the REST handler receiving `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`) and delegating to `UsageCollectorClientV1::deactivate_usage_record(ctx, ...)` (`ctx: &SecurityContext` as first parameter per `sdk-trait.md` Method 5), accepting a `DeactivateRecordRequest`, returning HTTP `200` with `DeactivateRecordResponse { id, status: "inactive" }` on successful transition, and surfacing deterministic `Problem` envelopes through the yaml's `default` response for every failure case — canonical `Unauthenticated` (no `SecurityContext` present at the handler boundary), canonical `PermissionDenied` (PDP `deny` from `cpt-cf-usage-collector-contract-authz-resolver`), `context.reason="already_inactive"` (Plugin SPI Method 5 returned `AlreadyInactive`), canonical `NotFound` (Plugin SPI Method 5 returned `NotFound`), and `context.reason="plugin_readiness"` (Plugin SPI transport / readiness / persistence error). The handler MUST NOT widen the contract beyond what is declared in the yaml and MUST NOT introduce alternative status-mutation routes outside this single endpoint. The runtime-emitted OpenAPI document produced by `OpenApiRegistryImpl` MUST remain drift-free against the yaml per DESIGN §3.3 D1.

**Implements**:

- `cpt-cf-usage-collector-flow-event-deactivation-deactivate-record`

**Constraints**: `cpt-cf-usage-collector-fr-event-deactivation`

**Touches**:

- API: `POST /usage-collector/v1/records/{id}/deactivate`
- DB: `cpt-cf-usage-collector-dbtable-usage-records`
- Entities: `UsageRecord`, `DeactivationStatus`

### §2.5-item → DoD-ID Coverage Matrix

Coverage of every DECOMPOSITION §2.5 catalog item:

| §2.5 Item                                                           | Kind              | DoD ID                                                                           |
| ------------------------------------------------------------------- | ----------------- | -------------------------------------------------------------------------------- |
| `cpt-cf-usage-collector-fr-event-deactivation`                      | FR                | `cpt-cf-usage-collector-dod-event-deactivation-fr-event-deactivation`            |
| `cpt-cf-usage-collector-fr-usage-compensation` (cascade leg)        | FR                | `cpt-cf-usage-collector-dod-event-deactivation-fr-usage-compensation`            |
| `cpt-cf-usage-collector-fr-data-quality`                            | FR                | `cpt-cf-usage-collector-dod-event-deactivation-fr-data-quality`                  |
| `cpt-cf-usage-collector-fr-data-lifecycle`                          | FR                | `cpt-cf-usage-collector-dod-event-deactivation-fr-data-lifecycle`                |
| `cpt-cf-usage-collector-fr-audit-trail`                             | FR                | `cpt-cf-usage-collector-dod-event-deactivation-fr-audit-trail`                   |
| `cpt-cf-usage-collector-nfr-authorization`                          | NFR               | `cpt-cf-usage-collector-dod-event-deactivation-nfr-authorization`                |
| `cpt-cf-usage-collector-nfr-availability`                           | NFR               | `cpt-cf-usage-collector-dod-event-deactivation-nfr-availability`                 |
| `cpt-cf-usage-collector-principle-monotonic-deactivation`           | Principle         | `cpt-cf-usage-collector-dod-event-deactivation-principle-monotonic-deactivation` |
| `cpt-cf-usage-collector-principle-fail-closed`                      | Principle         | `cpt-cf-usage-collector-dod-event-deactivation-principle-fail-closed`            |
| `cpt-cf-usage-collector-adr-monotonic-deactivation`                 | ADR               | `cpt-cf-usage-collector-dod-event-deactivation-adr-monotonic-deactivation`       |
| `cpt-cf-usage-collector-adr-usage-compensation` (cascade companion) | ADR               | `cpt-cf-usage-collector-dod-event-deactivation-adr-usage-compensation`           |
| `cpt-cf-usage-collector-constraint-no-business-logic`               | Design constraint | `cpt-cf-usage-collector-dod-event-deactivation-constraint-no-business-logic`     |
| `cpt-cf-usage-collector-component-deactivation-handler`             | Design component  | `cpt-cf-usage-collector-dod-event-deactivation-component-deactivation-handler`   |
| `cpt-cf-usage-collector-seq-deactivate-event`                       | Sequence          | `cpt-cf-usage-collector-dod-event-deactivation-seq-deactivate-event`             |
| `cpt-cf-usage-collector-dbtable-usage-records` (status column only) | Data              | `cpt-cf-usage-collector-dod-event-deactivation-dbtable-usage-records`            |
| `cpt-cf-usage-collector-entity-usage-record` (status only)          | Entity            | `cpt-cf-usage-collector-dod-event-deactivation-entity-usage-record`              |
| `cpt-cf-usage-collector-entity-deactivation-status`                 | Entity            | `cpt-cf-usage-collector-dod-event-deactivation-entity-deactivation-status`       |
| `cpt-cf-usage-collector-entity-entry-type` (cascade discriminator)  | Entity            | `cpt-cf-usage-collector-dod-event-deactivation-entity-entry-type`                |
| `cpt-cf-usage-collector-entity-security-context`                    | Entity            | `cpt-cf-usage-collector-dod-event-deactivation-entity-security-context`          |
| `POST /usage-collector/v1/records/{id}/deactivate`                  | API               | `cpt-cf-usage-collector-dod-event-deactivation-api-post-records-id-deactivate`   |

## 6. Acceptance Criteria

- [ ] `p1` - A well-formed deactivation request by an authorized platform operator through `POST /usage-collector/v1/records/{id}/deactivate` or through the SDK `deactivate_usage_record` operation transitions the targeted `cpt-cf-usage-collector-dbtable-usage-records` row's `status` from `active` to `inactive` via a single Plugin SPI Method 5 `transition_active_to_inactive` capability invocation that returns `DeactivationOutcome::Transitioned { primary_id, cascaded_compensation_ids }`; the post-call row's `tenant_id`, `resource_id`, `resource_type`, `subject_id`, `subject_type`, `source_gear`, `metric_gts_id`, `value`, `timestamp`, `idempotency_key`, `entry_type`, `corrects_id`, and `metadata` columns are byte-identical to the pre-call values, and the REST response is HTTP `200` with `DeactivateRecordResponse { id, cascaded_compensation_ids, status: "inactive" }` per `usage-collector-v1.yaml` (status-only transition per `cpt-cf-usage-collector-dod-event-deactivation-fr-event-deactivation` and `cpt-cf-usage-collector-dod-event-deactivation-api-post-records-id-deactivate`). The acceptance criterion applies uniformly when the target is `entry_type = usage` (cascade may flip companions) AND when the target is `entry_type = compensation` (single-row, `cascaded_compensation_ids = []`).
- [ ] `p1` - Deactivating an `entry_type = usage` row R that has N (N ≥ 1) active `entry_type = compensation` rows with `corrects_id = R.id ∧ same (tenant_id, metric_gts_id)` flips R AND all N referencing compensations from `active` to `inactive` in a **single atomic** Plugin SPI Method 5 transition per `cpt-cf-usage-collector-algo-event-deactivation-atomic-cascade-flip` and `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle`; the response carries `cascaded_compensation_ids` listing the N compensation ids, and a follow-up `SUM(value)` over `(tenant_id, metric_gts_id)` returns to the pre-acceptance baseline (depth-1 cascade per `cpt-cf-usage-collector-dod-event-deactivation-fr-usage-compensation` and `cpt-cf-usage-collector-dod-event-deactivation-adr-usage-compensation`).
- [ ] `p1` - Deactivating an `entry_type = compensation` row C flips ONLY C — no cascade target lookup is performed — and surfaces `cascaded_compensation_ids: []`; this is structural per the compensating-a-compensation non-goal in `cpt-cf-usage-collector-adr-usage-compensation` (single-row deactivation per `cpt-cf-usage-collector-flow-event-deactivation-cascade`).
- [ ] `p1` - A compensation ingestion submission referencing R that arrives while R is being deactivated is rejected by the L1 "referenced record must be active" check enforced on the ingestion path inlined in `features/usage-emission.md` per `cpt-cf-usage-collector-algo-event-deactivation-concurrency-guard`; either the compensation serialises before the deactivation commit and is included in `cascaded_compensation_ids`, or it serialises after the commit and is rejected — no compensation can be admitted referencing an `inactive` row, and no row's `status` changes outside the atomic cascade transition (concurrency safety without distributed coordination).
- [ ] `p1` - A second deactivation request targeting an already-inactive record (the Plugin SPI Method 5 capability returned `DeactivationOutcome::AlreadyInactive`) is surfaced as the `Problem` envelope with `context.reason="already_inactive"` per `usage-collector-v1.yaml` and the SDK `AlreadyInactive` error variant per `sdk-trait.md` Method 5; the row's `status` column remains `inactive` and no other column is mutated (monotonicity per `cpt-cf-usage-collector-dod-event-deactivation-principle-monotonic-deactivation` and `cpt-cf-usage-collector-dod-event-deactivation-adr-monotonic-deactivation`).
- [ ] `p1` - A deactivation request targeting a non-existent `id` (the Plugin SPI Method 5 capability returned `DeactivationOutcome::NotFound`) is surfaced as the canonical `NotFound` `Problem` envelope per `usage-collector-v1.yaml` and the SDK `NotFound` error variant per `sdk-trait.md` Method 5; no state change occurs (not-found handling per `cpt-cf-usage-collector-dod-event-deactivation-fr-event-deactivation`).
- [ ] `p1` - Every deactivation request accepts a resolved `cpt-cf-usage-collector-entity-security-context` at the handler boundary — on REST as `Extension<SecurityContext>` populated by ToolKit gateway middleware (`OperationBuilder::authenticated()`), on the SDK trait as `ctx: &SecurityContext` first parameter — and dispatches PDP authorization through `cpt-cf-usage-collector-flow-foundation-pdp-authorize` (per-component `authz_scope` helper against `cpt-cf-usage-collector-contract-authz-resolver`) against the deactivation attribution tuple (operator identity, target record `id`) before any Plugin SPI dispatch; absence of `SecurityContext` at the boundary surfaces the canonical `Unauthenticated` `Problem` envelope per the yaml `default` response, a PDP `deny` surfaces the canonical `PermissionDenied` `Problem` envelope per the yaml `default` response, and no row is mutated in either case (PDP-gated authorization per `cpt-cf-usage-collector-dod-event-deactivation-nfr-authorization` and `cpt-cf-usage-collector-dod-event-deactivation-principle-fail-closed`).
- [ ] `p1` - A Plugin SPI transport / readiness / persistence error (`PluginUnavailable`, `Timeout`, `BackendError`, `ContractViolation`) from the Method 5 capability surfaces as the canonical `Problem` envelope with `context.reason="plugin_readiness"` per `usage-collector-v1.yaml` while preserving the audit-correlation context owned by `cpt-cf-usage-collector-algo-foundation-audit-correlation`; the row's `status` column is unchanged, the operator can retry idempotently with the same `id`, and a retry after a successful prior transition is structurally idempotent because the SPI capability returns `DeactivationOutcome::AlreadyInactive` (not `Transitioned`) on the retry (fail-closed plus idempotent retry per `cpt-cf-usage-collector-dod-event-deactivation-principle-fail-closed` and `cpt-cf-usage-collector-dod-event-deactivation-nfr-availability`).
- [ ] `p1` - A successfully deactivated record remains visible to the §2.4 Query Gateway with `status="inactive"` — both the raw query path (`GET /usage-collector/v1/records`) and the aggregated query path (`POST /usage-collector/v1/records/aggregate`) return the row within the PDP-authorized scope per `cpt-cf-usage-collector-fr-data-lifecycle` and DECOMPOSITION §2.4 "Active-and-inactive record visibility"; the row is NEVER physically deleted by the deactivation handler — physical retention, archival, and purge are owned by the active storage plugin's deployment profile per `cpt-cf-usage-collector-adr-pluggable-storage` (queryability preservation per `cpt-cf-usage-collector-dod-event-deactivation-fr-data-lifecycle`).
- [ ] `p1` - Every accepted deactivation propagates the request-level correlation identifier (`request-id`) through `cpt-cf-usage-collector-algo-foundation-audit-correlation` so the platform gateway access log and the `cpt-cf-usage-collector-contract-authz-resolver` PDP decision logs reconcile with gear-level deactivation activity; a follow-up audit query against the platform access trail using the propagated `request-id` returns the operator identity and the target record `id` for the corresponding deactivation event (audit propagation per `cpt-cf-usage-collector-dod-event-deactivation-fr-audit-trail`).
- [ ] `p1` - No reactivation path exists in either the REST surface (`usage-collector-v1.yaml` has no `inactive → active` endpoint) or the SDK trait surface (`sdk-trait.md` has no reactivation method); the one-way `active → inactive` latch applies uniformly to primary rows AND to compensation rows flipped by the depth-1 cascade — any caller-side attempt to construct such a request is structurally impossible on the published contract surface, and any subsequent deactivation against the same `id` (whether primary or previously-cascaded compensation) returns `context.reason="already_inactive"` rather than re-entering the `Active` state (no-reactivation invariant per `cpt-cf-usage-collector-state-event-deactivation-record-lifecycle` and `cpt-cf-usage-collector-dod-event-deactivation-adr-monotonic-deactivation`).
