---
status: accepted
date: 2026-04-21
decision-makers: Constructor Fabric Steering Committee
---

# ADR-001: Tenant Hierarchy Closure Ownership

**ID**: `cpt-cf-tr-plugin-adr-p1-tenant-hierarchy-closure-ownership`

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Comparative Table](#comparative-table)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [AM tree plus Tenant Resolver closure](#am-tree-plus-tenant-resolver-closure)
  - [AM tree plus AM generic closure](#am-tree-plus-am-generic-closure)
  - [AM tree plus AM canonical closure](#am-tree-plus-am-canonical-closure)
  - [AM implements full Tenant Resolver trait logic](#am-implements-full-tenant-resolver-trait-logic)
- [More Information](#more-information)

<!-- /toc -->

## Context and Problem Statement

The platform needs a fast transitive hierarchy read model for tenant traversal, barrier-aware enforcement, and subtree membership checks. Account Management (AM) is the canonical owner of tenant hierarchy state (`id`, `parent_id`, `status`, `self_managed`, tenant type), while Tenant Resolver (TR) is being designed as the hot-path query gear consumed by AuthZ and other policy-facing flows.

Two platform-level facts frame the problem:

1. The `tenant_closure` table is standardized at the platform level in [TENANT_MODEL.md](../../../../../../docs/arch/authorization/TENANT_MODEL.md) with the canonical schema `(ancestor_id, descendant_id, barrier, descendant_status)`. The `barrier` column and denormalized `descendant_status` are part of the platform-canonical closure shape — they are not resolver-specific extensions.
2. AM already owns barrier state as data (`self_managed` boolean on the `tenants` row) under the `cpt-cf-account-management-principle-barrier-as-data` principle, and AM already performs barrier-aware ancestor walks on the hot path for metadata inheritance (see `cpt-cf-account-management-adr-metadata-inheritance` and `cpt-cf-account-management-fr-tenant-metadata-api`). Barrier semantics are already part of AM's internal domain, not an imported resolver concern.

The architectural question is whether to keep a split where TR owns a separate closure projection synchronized from AM, or to consolidate closure ownership into AM alongside the canonical tree, following the pattern already used by Resource Group (which owns both `resource_group` and `resource_group_closure`).

## Decision Drivers

- Preserve clear ownership of canonical tenant data (`cpt-cf-tr-plugin-principle-query-facade`)
- Avoid cross-gear synchronization boundaries when the underlying data has a single writer
- Align with the platform-canonical `tenant_closure` schema defined in `TENANT_MODEL.md`
- Meet hot-path traversal/query NFRs without introducing projection freshness windows
- Keep barrier traversal semantics aligned with the Tenant Resolver SDK source of truth
- Avoid coupling AM to resolver-specific query semantics that do not already belong to AM's domain
- Follow the ownership pattern established by Resource Group (canonical tree and canonical closure co-located)

## Considered Options

- **Option 1: AM tree + Tenant Resolver closure** — AM owns canonical tenant hierarchy; Tenant Resolver owns and rebuilds the closure projection from AM's sync-oriented contract.
- **Option 2: AM tree + AM generic closure** — AM owns canonical hierarchy and a generic closure table (`ancestor_id`, `descendant_id`, `depth`), while Tenant Resolver remains a facade for barrier/status/query semantics.
- **Option 3: AM tree + AM canonical closure** — AM owns canonical hierarchy and the platform-canonical `tenant_closure` including `barrier` and `descendant_status` as defined in `TENANT_MODEL.md`. Tenant Resolver becomes a query facade over AM-owned canonical storage.
- **Option 4: AM implements full Tenant Resolver trait logic** — AM owns canonical hierarchy and directly implements the full `TenantResolverPluginClient` behavior, making Tenant Resolver a facade-free or nearly facade-free compatibility layer.

## Decision Outcome

Chosen option: **Option 3 — AM tree + AM canonical closure**.

AM owns both the canonical tenant tree and the platform-canonical `tenant_closure` table `(ancestor_id, descendant_id, barrier, descendant_status)`. Closure maintenance occurs transactionally alongside tenant writes in AM. Tenant Resolver remains a distinct gear but is scoped to query facade responsibilities — `BarrierMode` application, ordering guarantees, pagination, and the SDK surface consumed by AuthZ and other clients. Tenant Resolver does not own a derived projection, does not run a `SyncEngine`/`ClosureWriter`, and does not consume a sync-oriented AM capability.

Two facts support consolidating closure ownership in AM:

1. The `tenant_closure` schema in `TENANT_MODEL.md` already includes `barrier` and `descendant_status` as canonical columns. The closure shape is platform-canonical; AM adopting it does not import resolver-specific concerns.
2. AM already encodes barrier semantics natively — `self_managed` is a tenant-row flag and AM already walks the hierarchy with barrier awareness for metadata inheritance. Maintaining the barrier flag in `tenant_closure` is a performance materialization of logic AM already runs.

The AM tenant write model is favorable to in-AM closure maintenance:

- **Create** — O(depth) inserts per new tenant.
- **Soft delete** — flips `tenants.status` to `'deleted'` and, because `tenant_closure.descendant_status` is denormalized from `tenants.status` (closure-status denormalization invariant), also performs O(depth) updates on `tenant_closure.descendant_status` for every row where `descendant_id = X`; no closure row is inserted or removed. This is the same shape as the generic Status change entry below — it is called out separately here so maintainers know the soft-delete path still carries the O(depth) closure-denormalization cost.
- **Hard delete** — restricted to leaves by `cpt-cf-account-management-fr-tenant-soft-delete`; O(depth) deletes.
- **Status change** — O(depth) updates to denormalized `tenant_closure.descendant_status` on rows where `descendant_id = X` (same mechanism as Soft delete; covers `active ↔ suspended` and the soft-delete flip to `deleted`).
- **Convert (self_managed toggle)** — O(strict_ancestors(X) × (1 + descendants(X))) updates to the `barrier` column. Bounded by depth and subtree size; convert is a rare dual-consent operation (`cpt-cf-account-management-fr-mode-conversion-approval`).
- **Subtree move** — not supported in AM; `update_tenant` mutates only `name` and `status` (`cpt-cf-account-management-fr-tenant-update`).

Consolidating closure ownership in AM removes the operational complexity that a split ownership model would require — drift detection, revision tokens, rebuild loops, projection freshness windows, and dual representation of hierarchy state across two gears.

### Consequences

- Hierarchy state exists in exactly one gear and one database. No cross-gear projection, no freshness window between commit and enforcement.
- AM owns closure maintenance transactionally with tenant writes. Write amplification is bounded: linear in depth for create/delete/status, bounded by ancestors × subtree size for convert.
- Tenant Resolver's role narrows to a query facade: `BarrierMode` application, ordering, pagination, SDK contract. It no longer owns a derived projection or a sync loop.
- Resource Group's co-located tree-plus-closure pattern becomes the uniform platform pattern for hierarchical domains.
- No AM-exposed sync capability (canonical enumeration + revision/change token) is required. AM's public contract remains request-oriented rather than sync-oriented.
- Tenant Resolver DESIGN and PRD must be updated: removal of `SyncEngine`, `ClosureWriter`, `tenant_closure` ownership, drift-detection contract, and associated NFRs. AM DESIGN must be updated to introduce the `tenant_closure` table and closure-maintenance responsibilities in `TenantService` and the conversion flow.
- A future event-driven design (VHP-460) can still be revisited, but its motivation shrinks because the primary driver — cross-gear projection lag — no longer exists.

### Confirmation

- AM DESIGN introduces `tenant_closure` with the platform-canonical schema `(ancestor_id, descendant_id, barrier, descendant_status)` and allocates closure maintenance to `TenantService` write paths and `ConversionService::approve`.
- Tenant Resolver DESIGN removes `SyncEngine`, `ClosureWriter`, `tenant_closure` ownership, revision-token contract, drift-detection FRs, and projection-availability NFR scoped to a derived model.
- Tenant Resolver DESIGN retains the SDK surface (`TenantResolverPluginClient`) and adds a direct data dependency on AM-owned storage (via a read-only database contract or a read-oriented client).
- Tenant Resolver connects to AM-owned storage through its own **dedicated SecureConn connection pool** bound to the read-only role — physically separate from AM's writer pool — so plugin hot-path read traffic and AM writer traffic are isolated at the pool layer and cannot starve one another. Pool sizing, saturation alerts (`tenant_resolver_db_pool_waiters` / `tenant_resolver_db_pool_utilization`), and per-statement `query_timeout` are owned by the plugin, not AM.
- AM does not expose a sync capability (canonical enumeration + revision/change token).
- No parallel projection of tenant hierarchy state exists outside AM.
- Integration tests verify closure invariants on create/delete/status/convert without an intervening sync step.

## Comparative Table

| Option | Ownership clarity | Write-path complexity | Read-path performance fit | Coupling to resolver semantics | Freshness model | Impact on Tenant Resolver role | Fit |
|--------|-------------------|-----------------------|----------------------------|-------------------------------|----------------|-------------------------------|--------|
| **AM tree + Tenant Resolver closure** | Dual ownership: AM owns source tree, TR owns derived read model | Medium in TR, low in AM, plus revision-token writes in AM | Strong once projection is fresh | Low-to-medium: AM exposes sync contract only | Poll-driven, bounded by revision-token check + rebuild | TR owns a derived projection and sync loop | Viable, but pays permanent sync overhead |
| **AM tree + AM generic closure** | AM owns tree and a generic transitive model; TR owns barrier/status/order semantics at query time | Low-medium in AM (closure only, no barrier materialization) | Good, but barrier filtering requires path-joins at query time | Low | Transactional with source writes; no sync lag | TR filters and shapes queries over AM's generic closure | Clean, but diverges from platform-canonical `tenant_closure` schema |
| **AM tree + AM canonical closure** | Single owner for tree and canonical closure | Bounded: O(depth) for create/delete/status; O(ancestors × subtree) for rare convert; no subtree moves | Strong: platform-canonical barrier-aware closure enables O(1)-JOIN filtering | Low: `barrier` and `status` are canonical tenant data, not resolver-imported concepts | Transactional with source writes; no sync lag | TR becomes a query facade over AM-owned canonical storage | **Best fit** |
| **AM implements full Tenant Resolver trait logic** | Single owner for tree, closure, and resolver behavior | Highest: AM owns write model, closure, and full resolver semantics | Potentially strong, but pushes hot-path concerns into AM | Very high: AM is directly coupled to the Tenant Resolver SDK contract | Transactional with source writes; no sync lag | TR becomes a compatibility shim or is eliminated | Overreach; collapses useful gear boundary |

## Pros and Cons of the Options

### AM tree plus Tenant Resolver closure

- Good, because canonical data ownership and hot-path read-model ownership are explicitly separated.
- Good, because Tenant Resolver can evolve projection internals independently of AM storage.
- Bad, because it introduces a permanent synchronization surface — revision tokens, poll loops, rebuild orchestration, drift detection — that has no corresponding business requirement.
- Bad, because hierarchy state is materialized twice across two gears, doubling the failure surface (projection staleness, rebuild failure, drift).
- Bad, because AM still pays write-side cost (revision-token maintenance on every hierarchy mutation) without gaining a simpler contract.
- Bad, because the closure projection shape TR would maintain is the platform-canonical shape; splitting its ownership from the source tree is structural duplication rather than separation of concerns.

### AM tree plus AM generic closure

- Good, because a generic closure is the minimal addition to AM and can be maintained transactionally with source writes.
- Good, because Tenant Resolver retains clear query-semantics ownership without consuming a derived projection.
- Bad, because it diverges from the platform-canonical `tenant_closure` schema in `TENANT_MODEL.md`, which already includes `barrier` and `descendant_status`.
- Bad, because barrier-aware queries require per-query path joins to check barriers along the path, pushing read-time cost back onto every caller.
- Bad, because `descendant_status` denormalization is already standard in the platform closure schema; omitting it forces TR or callers to re-derive what the platform already defines.

### AM tree plus AM canonical closure

- Good, because AM owns one coherent aggregate — tree, barrier, closure — with transactional consistency and no cross-gear sync.
- Good, because the closure shape matches `TENANT_MODEL.md` directly; there is no new or resolver-specific schema to define.
- Good, because barrier is already an AM-native concept (`cpt-cf-account-management-principle-barrier-as-data`) and AM already runs barrier-aware hierarchy walks on the hot path for metadata inheritance.
- Good, because the AM write surface is favorable: create/delete/status are O(depth), convert is rare and bounded, subtree moves are not supported.
- Good, because it aligns with the Resource Group pattern of co-located tree and closure ownership, making hierarchy ownership uniform across the platform.
- Good, because Tenant Resolver's role narrows to a query facade — a simpler, more cohesive responsibility — without becoming a compatibility shim.
- Bad, because AM takes on closure maintenance on hierarchy mutations, including non-trivial amplification on the rare convert operation.
- Bad, because Tenant Resolver DESIGN and PRD require non-trivial updates — removal of `SyncEngine`, `ClosureWriter`, drift-detection contract, and related NFRs.
- Bad, because TR depends directly on AM-owned storage (read-only), which narrows the AM/TR contract surface but requires explicit agreement on the shared schema and its evolution.

### AM implements full Tenant Resolver trait logic

- Good, because a single gear owns canonical state and traversal behavior with no intermediate abstraction.
- Bad, because AM becomes directly responsible for `TenantResolverPluginClient` semantics — `BarrierMode`, ordering, pagination, hot-path latency — which are genuinely resolver concerns unrelated to administrative correctness.
- Bad, because it collapses a useful gear boundary; AM starts absorbing responsibilities that do not belong to administrative source-of-truth ownership.
- Bad, because Tenant Resolver becomes a redundant abstraction or a legacy compatibility wrapper with no clear independent purpose.
- Bad, because it is the largest redesign and provides no incremental value over Option 3, where TR retains a meaningful query-facade role.

## More Information

- Impacted artifacts (updates applied as of 2026-04-22):
  - [PRD](../PRD.md) — rescoped to query-facade semantics; sync contract assumptions, drift detection, and projection ownership removed.
  - [DESIGN](../DESIGN.md) — `SyncEngine` / `ClosureWriter` / `tenant_closure` ownership removed; direct read dependency on AM storage documented; SDK surface retained.
  - [AM DESIGN](../../../docs/DESIGN.md) — `tenant_closure` introduced with platform-canonical schema; closure maintenance allocated to `TenantService` and `ConversionService::approve`; no sync-oriented capability introduced — see the **Barrier as Data** principle under §2.1 *Design Principles* and FR `cpt-cf-account-management-fr-tenant-closure` referenced from §1.2 *Architecture Drivers* (Functional Drivers mapping) for the integrated prose.
  - [AM PRD](../../../docs/PRD.md) — canonical closure ownership added to AM scope via FR `cpt-cf-account-management-fr-tenant-closure` under §4.1 *In Scope* and §5.2 *Tenant Hierarchy Management*. References use FR slugs (which are stable) rather than line numbers (which drift on reflow).
- Removed / rescoped requirements (retained for historical audit; these slugs carried the live `cpt-cf-tr-plugin-*` prefix in the pre-ADR draft PRD and are no longer definitions anywhere):
  - ~~fr-full-sync~~ — removed; no sync path exists.
  - ~~fr-drift-detection~~ — removed; transactional consistency replaces drift detection.
  - ~~nfr-sync-integrity~~ — removed.
  - ~~nfr-projection-availability~~ — rescoped or removed; availability now tracks AM storage availability.
- Platform references:
  - [TENANT_MODEL.md](../../../../../../docs/arch/authorization/TENANT_MODEL.md) — canonical `tenant_closure` schema with `barrier` and `descendant_status`.
  - [RESOURCE_GROUP_MODEL.md](../../../../../../docs/arch/authorization/RESOURCE_GROUP_MODEL.md) — precedent for co-located tree plus closure ownership (`resource_group` + `resource_group_closure`).
  - `cpt-cf-account-management-principle-barrier-as-data` — barrier as AM-native data.
  - `cpt-cf-account-management-adr-metadata-inheritance` — AM already performs barrier-aware ancestor walks on the hot path.
- Future reconsideration:
  - Revisit if AM hierarchy write rates or convert frequency change materially and make in-AM closure maintenance a measured bottleneck.
  - Revisit if VHP-460 event-driven sync introduces an independent reason to externalize projection ownership beyond current motivations.
