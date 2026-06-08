---
status: accepted
date: 2026-05-29
decision-makers: Constructor Fabric Steering Committee
---

# Usage compensation as a signed negative entry on the unified ingestion path

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Signed compensation entry on the unified ingestion path](#signed-compensation-entry-on-the-unified-ingestion-path)
  - [Adjustment-by-reference / record amendment](#adjustment-by-reference--record-amendment)
  - [Downstream-only correction](#downstream-only-correction)
- [More Information](#more-information)
- [Traceability](#traceability)

<!-- /toc -->

**ID**: `cpt-cf-usage-collector-adr-usage-compensation`

## Context and Problem Statement

Counter-kind usage records accumulate via `SUM` aggregation, and operational realities — refunds, partial releases, credit-style adjustments — require the net total to decrease without retracting the original event. The retraction primitive in `cpt-cf-usage-collector-adr-monotonic-deactivation` (file `./0005-monotonic-deactivation.md`) handles whole-row error retraction, but it cannot express a partial reduction: it flips the entire row to `inactive`, removing all of its value from the net total. A second correction primitive is therefore needed for the value-reversal case, and the question is how to expose it. The decision must preserve three invariants already accepted across the Usage Collector spec: the append-only invariant on stored records (no in-place mutation), the backend-agnostic posture (no concrete storage-engine assumptions), and the recording-not-computing posture (the collector records caller-supplied quantities; it does not derive business logic). It must also remain compatible with the mandatory idempotency contract (`cpt-cf-usage-collector-adr-mandatory-idempotency`, file `./0004-mandatory-idempotency.md`) and with Policy Decision Point (PDP) attribution at the source gear.

## Decision Drivers

- Append-only invariant: stored records must not be mutated in place.
- Backend-agnostic posture: the primitive must not assume any particular storage engine or transactional model beyond what the Plugin SPI already requires.
- PDP attribution at the source: corrections must carry the same caller-supplied attribution as the original emission, gated by the same PDP boundary.
- Mandatory idempotency: every correction must carry an idempotency key on the unified ingestion path (per `cpt-cf-usage-collector-adr-mandatory-idempotency`).
- `SUM`-nets aggregation: the net `SUM(value)` over `active` rows must equal the corrected net total without per-record reconciliation logic in the collector.
- Counter-only scope: gauges, `COUNT`, `MIN`, `MAX`, and `AVG` cannot be netted by appending a signed entry — those cases are owned by retraction (`cpt-cf-usage-collector-adr-monotonic-deactivation`).
- No L2 remaining-amount tracking: the collector does not track per-record outstanding balances or lot/FIFO-LIFO state — that is downstream-ledger work.
- Concurrency safety vs. deactivation: a compensation that arrives while its target row is being deactivated must not race past the deactivation; the ingestion-time L1 validation must reject it cleanly.

## Considered Options

- Signed compensation entry on the unified ingestion path — the source gear emits an entry with `entry_type=compensation`, a negative `value`, and `corrects_id` pointing at the original `usage` row, through the existing emit endpoint / SDK method / SPI persist call. Same PDP attribution, same mandatory idempotency key, same validation lane (extended by `(kind × entry_type)` matrix), same storage row shape.
- Adjustment-by-reference / record amendment — mutate the original `usage` row in place (for example, add a `corrected_value` column or rewrite `value`).
- Downstream-only correction — the Usage Collector never records corrections; consumers reconcile via their own ledgers.

## Decision Outcome

Chosen option: "Signed compensation entry on the unified ingestion path", because it preserves the append-only invariant, reuses the existing emit / PDP / idempotency machinery without adding a parallel ingestion surface, and yields `SUM`-based netting deterministically with no business-logic computation inside the collector. The source gear emits a new row with `entry_type=compensation`, a negative `value`, and a `corrects_id` field carrying the identifier of the original `usage` row; the row travels through the existing emit operation, REST endpoint, SDK method, and Plugin SPI persist call, with the same caller-supplied attribution and the same mandatory idempotency key. No dedicated `compensate` REST path, SDK method, or SPI call is introduced.

The Usage Collector spec now defines two complementary correction primitives. **Deactivation** (`cpt-cf-usage-collector-adr-monotonic-deactivation`) is cross-kind error retraction: a whole-row, one-way `active → inactive` latch, operator-only, applying to any `entry_type`, with a depth-1 cascade from a deactivated `usage` row to its referencing `active` compensations. **Compensation** (this ADR) is counter value-reversal: an append-only negative-value entry that reduces `SUM`, counter-only, source-gear-emitted on the ingestion path with PDP attribution and a mandatory idempotency key. The two primitives are disjoint by purpose (retraction versus value-reversal) and complementary by aggregation contract: deactivation removes a row from every aggregation; compensation reduces the netted `SUM` only.

The validation matrix governing what is accepted on ingestion is:

| kind \ entry_type | `usage`      | `compensation` |
| ----------------- | ------------ | -------------- |
| `counter`         | `value >= 0` | `value < 0`    |
| `gauge`           | any value    | REJECTED       |

The aggregation contract is: `SUM(value)` over `active` rows nets across `usage` and `compensation` signed entries — `SUM(value)` is the net total. `COUNT`, `MIN`, `MAX`, and `AVG` operate over `usage` entries only — compensation entries adjust `SUM`; they are not events.

The L1 validation of `corrects_id` performed at ingestion is: the referenced row MUST exist, MUST have `entry_type=usage`, MUST share `(tenant_id, metric_gts_id)` with the incoming compensation, and MUST be `active`. There is no L2 layer: the collector does not track per-record remaining amounts, lot/FIFO-LIFO state, or whether multiple compensations together exceed the original value. Concurrency between compensation and deactivation is resolved at the L1 check: a compensation referencing a row that is currently being deactivated is rejected by the "referenced record must be active" clause; the deactivation cascade then flips any already-accepted compensations depth-1, leaving the net total consistent. Compensating a compensation is a non-goal — this is why the deactivation cascade is bounded at depth 1 by construction.

### Consequences

- The ingestion contract (SDK, REST, Plugin SPI persist) gains two fields on the request: `entry_type` (`usage` | `compensation`) and `corrects_id` (required when `entry_type=compensation`, absent otherwise). The `value` field becomes signed; the existing non-negative invariant moves into the `(kind × entry_type)` matrix.
- The validation matrix is applied at the ingestion boundary: `counter + compensation` requires `value < 0`; `gauge + compensation` is rejected; `counter + usage` keeps `value >= 0`; `gauge + usage` accepts any value.
- `SUM(value)` over `active` rows is the net total — callers and storage plugins must understand that `SUM` nets signed entries. `COUNT`, `MIN`, `MAX`, and `AVG` continue to operate over `entry_type=usage` rows only; the query layer filters `entry_type` before applying those aggregations.
- A compensation referencing a row that is currently being deactivated is rejected by the L1 "referenced record must be active" check; this provides concurrency safety without requiring distributed coordination.
- The depth-1 cascade in `cpt-cf-usage-collector-adr-monotonic-deactivation` is sufficient because compensating a compensation is a non-goal — no second-order chains exist.
- The Usage Collector does not validate non-negative net totals and does not emit negative-net detection signals; "net went negative" is a downstream concern, not a collector responsibility.
- The Usage Collector does not compute refunds, credits, credit-notes, quota, or lot/FIFO-LIFO depletion; recording a caller-supplied negative quantity is recording, not computing. Per-record remaining-amount tracking (an "L2" lane) is explicitly out of scope.
- Callers must compute the negative `value` themselves; reviewers must remember that `SUM(value)` is the net total; tooling that reads raw rows must understand `entry_type` and `corrects_id` semantics.

### Confirmation

Compliance is confirmed through (a) a Plugin SPI contract test that persists a signed compensation entry and asserts `SUM(value)` over `active` rows nets correctly across `usage` and `compensation`, (b) a validation matrix contract test covering the four `(kind × entry_type)` cells — `counter+usage` accepts `value >= 0`, `counter+compensation` accepts `value < 0`, `gauge+usage` accepts any value, `gauge+compensation` is rejected — (c) a concurrency contract test asserting that a compensation referencing a row that is being deactivated is rejected by the L1 "must be active" check, (d) a depth-1 cascade test (also covered by `cpt-cf-usage-collector-adr-monotonic-deactivation`) confirming that deactivating a `usage` row flips its `active` compensations to `inactive` in the same atomic step, and (e) `corrects_id` L1 validation tests asserting `entry_type=usage`, matching `(tenant_id, metric_gts_id)`, and `active` status on the referenced row.

## Pros and Cons of the Options

### Signed compensation entry on the unified ingestion path

The source gear emits an `entry_type=compensation` row with a negative `value` and `corrects_id`, through the same emit endpoint / SDK method / SPI persist call already in use for `entry_type=usage`.

- Good, because it preserves the append-only invariant — the original `usage` row is never mutated; the compensation is a new row that nets in `SUM`.
- Good, because it reuses the existing PDP attribution, mandatory idempotency, and Plugin SPI contract without introducing a parallel ingestion surface — the contract change is additive (two new request fields) rather than a new path.
- Good, because `SUM`-based netting is deterministic, backend-agnostic, and requires no business-logic computation inside the collector.
- Good, because the depth-1 deactivation cascade composes naturally — deactivating the target `usage` row also flips its compensations, keeping the net total consistent under retraction.
- Neutral, because callers must compute the negative `value` themselves; the collector does not derive it from a refund-percentage or release-quantity parameter.
- Neutral, because reviewers of stored data must remember that `SUM(value)` is the net total — raw-row inspection without aggregation can be misleading.
- Bad, because tooling that reads raw rows must learn the `entry_type` and `corrects_id` columns to interpret aggregation correctly; non-aware consumers can misinterpret a negative `value`.
- Bad, because the `value` field becomes signed, which subtly broadens the contract — a typed-language SDK must surface this clearly to avoid accidental positive compensations.

### Adjustment-by-reference / record amendment

Mutate the original `usage` row in place (for example, add a `corrected_value` column or rewrite `value`).

- Good, because the net total can be read from a single column without understanding signed entries.
- Bad, because it breaks the append-only invariant that the rest of the storage substrate relies on.
- Bad, because it conflicts with deactivation's whole-row latch — what is the state of an "amended-and-then-deactivated" row?
- Bad, because it complicates idempotency: an amendment with the same key as the original is ambiguous (replay or amendment?), and an amendment with a fresh key creates a non-atomic two-row history that downstream consumers cannot reason about.
- Bad, because computing a partial reduction (refund, partial release) inside the collector pushes it into business-logic territory — the collector becomes a mini-ledger, contradicting the recording-not-computing posture.
- Bad, because plugin authors must implement in-place mutation atomically across read and write paths, which is harder to enforce uniformly across plausible backends.

### Downstream-only correction

The Usage Collector never records corrections; consumers reconcile via their own ledgers.

- Good, because the collector contract stays minimal: only `entry_type=usage` rows ever exist.
- Bad, because the value-reversal primitive lives outside the source of record, leaving a permanent gap in the audit trail.
- Bad, because every downstream consumer must build its own reconciliation layer to interpret `SUM`, multiplying integration cost and creating divergence between consumers.
- Bad, because PRD-level capabilities that depend on `SUM`-based net totals (billing reads, dashboard sums) become unreliable without consumer-side reconciliation, breaking the contract the PRD already commits to.
- Bad, because operators lose the ability to express refunds and partial releases at the source — a regression versus what the deactivation primitive already provides for whole-row retraction.

## More Information

Related decisions:

- `cpt-cf-usage-collector-adr-monotonic-deactivation` (file `./0005-monotonic-deactivation.md`) — the complementary cross-kind error retraction primitive; deactivation cascades depth-1 to `active` compensations referencing a deactivated `usage` row, keeping the net total consistent under retraction. The two ADRs jointly define the Usage Collector's correction model.
- `cpt-cf-usage-collector-adr-mandatory-idempotency` (file `./0004-mandatory-idempotency.md`) — compensation rides the same unified ingestion path and therefore carries a mandatory idempotency key with the same exact-equality-versus-conflict semantics. Same-key replay of a compensation is deduplicated; same-key reuse with different content is rejected as `idempotency_conflict`.
- `cpt-cf-usage-collector-adr-pluggable-storage` (file `./0002-pluggable-storage.md`) — the Plugin SPI seam through which the signed `value`, `entry_type`, and `corrects_id` columns are persisted and `SUM`-nets aggregation is realized.

Non-goals explicitly out of scope for this ADR:

- Compensating a compensation (the depth-1 cascade in `cpt-cf-usage-collector-adr-monotonic-deactivation` is sufficient by construction because of this exclusion).
- Positive or otherwise-signed compensations beyond the locked `value < 0` rule for `counter + compensation`.
- L2 enforcement of per-record remaining amounts, outstanding balances, or any lot / FIFO-LIFO tracking.
- Negative-net detection, alerting, or rejection inside the Usage Collector.
- Computing refunds, credits, credit-notes, or quota inside the Usage Collector.

## Traceability

- **PRD**: [PRD.md](../PRD.md)
- **DESIGN**: [DESIGN.md](../DESIGN.md)
- **Related ADRs**: [`./0005-monotonic-deactivation.md`](./0005-monotonic-deactivation.md) (`cpt-cf-usage-collector-adr-monotonic-deactivation`), [`./0004-mandatory-idempotency.md`](./0004-mandatory-idempotency.md) (`cpt-cf-usage-collector-adr-mandatory-idempotency`), [`./0002-pluggable-storage.md`](./0002-pluggable-storage.md) (`cpt-cf-usage-collector-adr-pluggable-storage`).

This decision directly addresses or constrains the following requirements and design elements (IDs marked **forward** are minted by Phases 2–4 of the active plan and become canonical when those phases land):

- `cpt-cf-usage-collector-fr-usage-compensation` — the FR carrying the counter value-reversal capability surface (**forward**, minted in Phase 3).
- `cpt-cf-usage-collector-fr-counter-semantics` — counter accumulation semantics; `SUM` nets signed entries across `usage` and `compensation`.
- `cpt-cf-usage-collector-fr-gauge-semantics` — gauge semantics; gauge + compensation is rejected by the validation matrix.
- `cpt-cf-usage-collector-fr-idempotency` — mandatory idempotency key on the unified ingestion path; compensation rides the same contract.
- `cpt-cf-usage-collector-fr-ingestion` — the unified ingestion capability extended to carry `entry_type` and `corrects_id`.
- `cpt-cf-usage-collector-fr-ingestion-authorization` — PDP attribution applies to compensation identically to usage.
- `cpt-cf-usage-collector-fr-query-aggregation` — `SUM` nets signed; `COUNT`/`MIN`/`MAX`/`AVG` remain usage-only.
- `cpt-cf-usage-collector-fr-event-deactivation` — depth-1 cascade interaction with the retraction primitive.
- `cpt-cf-usage-collector-entity-entry-type` — the entity carrying the `usage` | `compensation` discriminator (**forward**, minted in Phase 2).
- `cpt-cf-usage-collector-entity-usage-record` — the record entity gains `entry_type` and `corrects_id` fields.
- `cpt-cf-usage-collector-dbtable-usage-records` — the storage shape gains the two new columns; `SUM(value)` nets across `entry_type` rows.
- `cpt-cf-usage-collector-seq-emit-usage` — the emission sequence extends to carry the new fields on the unified ingestion path.
- `cpt-cf-usage-collector-usecase-emit` — the emit use case covers both `entry_type=usage` and `entry_type=compensation` ingestion.
