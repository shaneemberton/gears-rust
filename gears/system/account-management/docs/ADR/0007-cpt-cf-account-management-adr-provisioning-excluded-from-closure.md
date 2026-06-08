---
status: accepted
date: 2026-04-23
decision-makers: Constructor Fabric Steering Committee
---

# ADR-0007: Exclude Provisioning Tenants from `tenant_closure`

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [Option A — Closure contains only SDK-visible tenants (drop `descendant_status` entirely)](#option-a--closure-contains-only-sdk-visible-tenants-drop-descendant_status-entirely)
  - [Option B — Closure contains only SDK-visible tenants (keep `descendant_status` for visible states)](#option-b--closure-contains-only-sdk-visible-tenants-keep-descendant_status-for-visible-states)
  - [Option C — Keep the original design: provisioning rows live in closure, plugin filters them](#option-c--keep-the-original-design-provisioning-rows-live-in-closure-plugin-filters-them)
- [More Information](#more-information)
- [Traceability](#traceability)

<!-- /toc -->

**ID**: `cpt-cf-account-management-adr-provisioning-excluded-from-closure`

## Context and Problem Statement

`tenant_closure` is AM's canonical transitive-ancestry table, read today by the Tenant Resolver Plugin and planned for future replication to business gears that need subtree/barrier awareness without cross-gear calls to AM.

The original design wrote a closure row for every `tenants` row from the moment it was inserted during the tenant-create saga, including the transient `provisioning` state. `descendant_status` carried the `provisioning` value on those rows, and the Tenant Resolver Plugin applied an unconditional `descendant_status <> 'provisioning'` predicate on every closure-driven read to hide them from SDK responses.

This works for a single-reader model but creates two problems once the closure becomes a publication contract:

1. **Replication surface leak.** Every replica consumer must know about `provisioning` and filter it out. Internal AM saga state flows across the replication boundary even though only SDK-visible tenants are consumed.
2. **Burden on every future reader.** Business gears integrating against a replicated `tenant_closure` inherit the provisioning-exclusion obligation. Missing the filter yields silently wrong subtree queries.

Review feedback (external) raised this explicitly: *"Did you consider an option to don't store provisioning records in the tenant_closure table? In the future we will use the same tenant_closure table in the business gears and implement replication mechanism for it. We will have to handle those records with the provisioning state separately."*

## Decision Drivers

- The closure must be a **clean publication contract** — consumers should receive only SDK-visible state.
- AM must remain the sole authority for tenant lifecycle, including the transient `provisioning` window.
- Hot-path query performance for `get_descendants` with caller-supplied status filtering must be preserved (this was the reason `descendant_status` was denormalized into the closure in the first place).
- Transactional consistency between `tenants` and `tenant_closure` must be preserved — readers never observe divergent state.
- Change scope should be small enough to apply without rewriting the tenant-create saga.

## Considered Options

- **Option A** — Closure contains only SDK-visible tenants; drop `descendant_status` entirely (force JOIN to `tenants` for status filtering).
- **Option B** — Closure contains only SDK-visible tenants; keep `descendant_status` but restrict its domain to `{active, suspended, deleted}`.
- **Option C** — Keep the original design (provisioning rows live in closure; plugin filters them on every read).

## Decision Outcome

Chosen option: **Option B** — provisioning tenants are absent from `tenant_closure` entirely; `descendant_status` remains but with a restricted 3-value domain.

Closure rows are inserted in a single transaction with the `provisioning → active` transition at the end of the tenant-create saga (saga step 3), and removed in a single transaction with hard-deletion. During provisioning, the tenant row exists in `tenants` with `status = 'provisioning'` and nothing in `tenant_closure`. Compensation (provisioning reaper rolling back a stuck provisioning row) deletes the `tenants` row only; no closure cleanup is needed because nothing was ever written.

### Consequences

- **Good**: The closure becomes a clean publication contract. Any future consumer — the Tenant Resolver Plugin today, business gear replicas tomorrow — never observes provisioning state and carries no provisioning-specific filtering obligation.
- **Good**: `tenant_closure.descendant_status` CHECK tightens to `{active, suspended, deleted}`, eliminating one internal-state value from the schema surface.
- **Good**: The Tenant Resolver Plugin's unconditional `descendant_status <> 'provisioning'` filter goes away. Provisioning invisibility becomes structural — closure-driven reads cannot surface provisioning tenants by construction. Plugin-side provisioning filtering remains only as defense-in-depth on direct `tenants` reads (existence probes, bulk-by-ids, ancestor hydration JOINs).
- **Good**: Hot-path performance is preserved — `descendant_status` remains denormalized for fast status-filtered subtree reads; no JOIN regression on `get_descendants`.
- **Neutral**: The tenant-create saga shifts closure insertion from step 1 to step 3. Step 1 becomes purely a `tenants` insert; step 3 inserts the `tenants` status update plus all closure rows in one transaction. The transactional shape is unchanged (both saga steps are short transactions bracketing the IdP call), only the row allocation between them shifts.
- **Neutral**: Any existing AM internal code that needed to resolve hierarchy for a `provisioning` tenant via `tenant_closure` must fall back to walking `tenants.parent_id` instead. Audit needed before implementation; in v1 the saga itself does not need hierarchy for provisioning tenants beyond the direct `parent_id` already stored on the row.
- **Bad**: One more invariant to enforce — "no provisioning rows in closure" — joining the existing list of closure invariants. Integrity checks must verify the absence, not just presence.

### Confirmation

Verified by integration tests covering:

1. Tenant created and left in `provisioning` → no closure rows exist for it; Tenant Resolver Plugin returns `TenantNotFound` for every SDK method targeting the id.
2. Provisioning reaper compensating a stuck row → `tenants` row removed; no closure cleanup needed; no orphan rows left behind.
3. Successful activation (`provisioning → active`) → closure rows appear atomically with the status transition; visible to Tenant Resolver Plugin immediately.
4. Schema CHECK rejects `descendant_status = 0` (provisioning code) in `tenant_closure`.
5. `TenantService` activation path is the only code path that inserts self-row + strict-ancestor rows.

## Pros and Cons of the Options

### Option A — Closure contains only SDK-visible tenants (drop `descendant_status` entirely)

The cleanest shape: if a row exists in `tenant_closure`, the tenant is SDK-visible; if not, it is provisioning or hard-deleted. No denormalization at all; every status-filtered subtree read JOINs `tenants`.

- Good, because the closure becomes a pure structural projection with no denormalized state.
- Good, because status updates on `tenants` no longer require closure writes.
- Bad, because every `get_descendants` call with a status filter needs a JOIN to `tenants` — write-amplification trade-off flips direction and the hot path pays for every read.
- Bad, because index coverage needs to change: `tenant_closure(ancestor_id, barrier, descendant_status)` → something like `tenant_closure(ancestor_id, barrier)` with `tenants(id, status)` covering status filtering on the JOIN side.

### Option B — Closure contains only SDK-visible tenants (keep `descendant_status` for visible states)

Same provisioning-absence guarantee as Option A, but `descendant_status` stays as a denormalized column carrying only `{active, suspended, deleted}`.

- Good, because it satisfies the replication hygiene goal (no provisioning in closure) with minimum churn.
- Good, because hot-path status-filtered subtree reads keep the single-index path they have today — no JOIN regression.
- Good, because the Tenant Resolver Plugin's unconditional provisioning filter becomes a no-op and can be removed, simplifying the query builder.
- Bad, because there is still a denormalized column to maintain transactionally on status transitions (but only between 3 states, not 4).

### Option C — Keep the original design: provisioning rows live in closure, plugin filters them

The status quo before this ADR.

- Good, because it is the smallest change (none).
- Good, because the "every tenant has a self-row + strict-ancestor rows" invariant is simple and universal.
- Bad, because it cannot satisfy the replication use case without duplicating the provisioning-filter obligation to every future consumer.
- Bad, because it exposes internal AM saga state on a storage contract that is intended to become a publication boundary.
- Bad, because the plugin must carry an unconditional filter on every SDK read that exists solely to hide saga state from consumers — a cross-cutting concern that fights the "closure = canonical hierarchy" framing.

## More Information

This ADR affects:

- [AM PRD §5.2 `cpt-cf-account-management-fr-tenant-closure`](../PRD.md) — closure-maintenance contract rewritten to specify "insert on `provisioning → active`, remove on hard-delete", and `descendant_status` domain tightened to SDK-visible states only.
- [AM DESIGN §2 `cpt-cf-account-management-principle-barrier-as-data`](../DESIGN.md) + §3 closure invariants table + §3.6 tenant-create saga diagram + §3.7 `Table: tenant_closure` — all updated to reflect the new allocation.
- [migration.sql](../migration.sql) — `tenant_closure.descendant_status` CHECK tightened from `IN (0,1,2,3)` to `IN (1,2,3)`; column COMMENT updated.
- [tr-plugin PRD `cpt-cf-tr-plugin-fr-provisioning-invisibility`](../tr-plugin/PRD.md) — split into structural (AM closure contract) + defense-in-depth (plugin `tenants` reads) layers.
- [tr-plugin DESIGN](../tr-plugin/DESIGN.md) — unconditional `descendant_status <> 'provisioning'` filter removed from query-builder descriptions; provisioning invisibility now described as structural on closure-driven reads.

Related ADRs:

- [ADR-0003 — Conversion Approval](./0003-cpt-cf-account-management-adr-conversion-approval.md) — sibling dual-consent state machine; closure barrier updates live in the same transaction as `self_managed` flips, separate from the provisioning/activation allocation this ADR addresses.
- [ADR-0004 — Reject RG as Canonical Tenant Hierarchy Store](./0004-cpt-cf-account-management-adr-resource-group-tenant-hierarchy-source.md) — establishes AM as the sole owner of `tenants` and `tenant_closure`; this ADR tightens what lives in the latter.

## Traceability

| Traces to | Type | Notes |
|-----------|------|-------|
| `cpt-cf-account-management-fr-tenant-closure` | PRD FR | Closure-maintenance contract rewritten per this decision |
| `cpt-cf-account-management-principle-barrier-as-data` | DESIGN principle | Principle paragraph updated to call out closure exclusion of provisioning |
| `cpt-cf-tr-plugin-fr-provisioning-invisibility` | Plugin PRD FR | Split into structural + defense-in-depth layers |
| `cpt-cf-tr-plugin-fr-get-descendants` | Plugin PRD FR | Filter predicate on `descendant_status` for provisioning no longer needed |
