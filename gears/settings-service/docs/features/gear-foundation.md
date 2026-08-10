<!-- Created: 2026-08-10 by Constructor Tech -->
<!-- Updated: 2026-08-10 by Constructor Tech -->

# Feature: Gear Foundation, SDK Contracts and Cross-Cutting Infrastructure

- [ ] `p1` - **ID**: `cpt-cf-settings-service-featstatus-gear-foundation`

- [ ] `p1` - `cpt-cf-settings-service-feature-gear-foundation`

<!-- toc -->

- [1. Feature Context](#1-feature-context)
  - [1.1 Overview](#11-overview)
  - [1.2 Purpose](#12-purpose)
  - [1.3 Actors](#13-actors)
  - [1.4 References](#14-references)
- [2. Actor Flows (CDSL)](#2-actor-flows-cdsl)
- [3. Processes / Business Logic (CDSL)](#3-processes--business-logic-cdsl)
  - [Gear Initialization and ClientHub Registration](#gear-initialization-and-clienthub-registration)
  - [Setting Key Parsing and Validation](#setting-key-parsing-and-validation)
  - [Optimistic Concurrency Precondition Evaluation](#optimistic-concurrency-precondition-evaluation)
  - [Domain Error to Problem Mapping](#domain-error-to-problem-mapping)
  - [Authorization Enforcement and Credential Step-Up](#authorization-enforcement-and-credential-step-up)
- [4. States (CDSL)](#4-states-cdsl)
- [5. Definitions of Done](#5-definitions-of-done)
  - [SDK Crate, Models and Value Objects](#sdk-crate-models-and-value-objects)
  - [SDK Trait Contracts](#sdk-trait-contracts)
  - [Error Taxonomy and Reader Degradation Contract](#error-taxonomy-and-reader-degradation-contract)
  - [RFC-9457 Problem Mapping](#rfc-9457-problem-mapping)
  - [Persistence Adapter and Migration Harness](#persistence-adapter-and-migration-harness)
  - [Gear Scaffold and ClientHub Registration](#gear-scaffold-and-clienthub-registration)
  - [REST and OData Infrastructure](#rest-and-odata-infrastructure)
  - [Policy Enforcement Point and Step-Up Gate](#policy-enforcement-point-and-step-up-gate)
  - [Audit Emitter](#audit-emitter)
- [6. Acceptance Criteria](#6-acceptance-criteria)

<!-- /toc -->

## 1. Feature Context

### 1.1 Overview

Establishes the `settings-service-sdk` crate and the gear scaffold that every later Settings Service feature is built on: SDK models and trait contracts, the error taxonomy and its RFC-9457 Problem mapping, PostgreSQL persistence with a migration harness, REST and OData infrastructure, the `PolicyEnforcer` PEP with credential step-up, and the Audit Emitter.

### 1.2 Purpose

`gears/settings-service/` currently contains documentation and no Rust source. This feature is what makes any code exist at all, and it is the only feature in the decomposition with no dependency of its own.

Its scope is deliberately drawn around what more than one later feature needs. Category Management, Setting Declarations, Typed Value Validation, and Value Resolution all need the same persistence adapter, the same `If-Match` precondition handling, the same authorization enforcement point, the same Problem mapping, and the same audit path. Building those once here is what keeps the later features about their domain rather than about plumbing.

Two contracts in this feature are consumed from outside the service and are therefore load-bearing beyond the gear itself: the `SettingsReaderClient` trait, which is the platform's in-process hot read path, and its degradation contract, which tells a consumer how to behave when the Settings Service is unreachable. Settings are a boot-time dependency, so getting that contract wrong makes every consuming gear's startup fragile.

**Requirements**: `cpt-cf-settings-service-fr-authn-role-gating`, `cpt-cf-settings-service-nfr-security-baseline`, `cpt-cf-settings-service-nfr-availability`

**Principles**: `cpt-cf-settings-service-principle-fail-closed`

### 1.3 Actors

| Actor | Role in Feature |
|-------|-----------------|
| `cpt-cf-settings-service-actor-internal-caller` | Consumes the `SettingsReaderClient` trait in process and must handle its degradation contract |
| `cpt-cf-settings-service-actor-contributing-module` | Consumes the `SettingsContributionClient` trait to register and retire its own declarations |
| `cpt-cf-settings-service-actor-authn-resolver` | Authenticates the caller and carries the credential step-up assertion |
| `cpt-cf-settings-service-actor-authz-resolver` | Supplies the authorization decision and the `AccessScope` constraints the PEP enforces |
| `cpt-cf-settings-service-actor-platform-admin` | The operator whose session is re-authenticated when a behavior-affecting action demands step-up |

### 1.4 References

- **PRD**: [PRD.md](../PRD.md) — `cpt-cf-settings-service-interface-settings-read-sdk`
- **Design**: [DESIGN.md](../DESIGN.md) — §4.2 (Audit Emitter), §4.3 (API Contracts, Error Response Format), §4.5 (Service-to-Service Pattern), §4.7 (Database schemas), §4.8 (Security and Authorization), §4.9 (Deployment Topology), §4.10 (Technology Stack)
- **DECOMPOSITION**: [DECOMPOSITION.md](../DECOMPOSITION.md) entry 2.1
- **Dependencies**: None. This is the root of the dependency graph.
- **Not applicable**: No domain entity or REST resource is delivered here, so there is no state machine and no actor flow. UX is out of scope (backend gear, no user interface). Performance targets are set at the system level in the PRD NFR section.

## 2. Actor Flows (CDSL)

Not applicable. This feature delivers SDK contracts and gear infrastructure with no user-facing interaction of its own. The actor flows that exercise these contracts belong to the features built on top of it: category administration in entry 2.2, declaration authoring in 2.3, and effective-value reads in 2.5.

## 3. Processes / Business Logic (CDSL)

### Gear Initialization and ClientHub Registration

- [ ] `p1` - **ID**: `cpt-cf-settings-service-algo-gear-foundation-gear-init`

**Input**: ToolKit-supplied gear configuration at startup

**Output**: A gear ready to serve, with its client traits resolvable through `ClientHub`

**Steps**:
1. [ ] - `p1` - Read deployment-owned bootstrap configuration from ToolKit config: database endpoint, broker endpoint, service identity, TLS material, ports - `inst-gf-init-1`
2. [ ] - `p1` - **IF** any required bootstrap value is absent → **RETURN** startup failure, because bootstrap configuration is deployment-owned and is never itself a managed setting - `inst-gf-init-2`
3. [ ] - `p1` - Establish the PostgreSQL connection pool and construct the `SecureConn` and `DBRunner` handles - `inst-gf-init-3`
4. [ ] - `p1` - Run outstanding schema migrations to completion - `inst-gf-init-4`
5. [ ] - `p1` - **IF** a migration fails → **RETURN** startup failure without serving traffic, so no request observes a partially migrated schema - `inst-gf-init-5`
6. [ ] - `p1` - Resolve the `TypesRegistryClient` and the Policy Decision client through `ClientHub` - `inst-gf-init-6`
7. [ ] - `p1` - Register the gear's own `SettingsReaderClient` and `SettingsContributionClient` implementations into `ClientHub` - `inst-gf-init-7`
8. [ ] - `p1` - Bind each registered trait according to the active deployment profile: the in-process implementation when co-located, the same trait over REST when out of process - `inst-gf-init-8`
9. [ ] - `p1` - Mark the gear ready and begin serving - `inst-gf-init-9`

### Setting Key Parsing and Validation

- [ ] `p1` - **ID**: `cpt-cf-settings-service-algo-gear-foundation-key-parse`

**Input**: Candidate setting key string

**Output**: A parsed key value object carrying its value-type half and instance half, or a validation problem

**Steps**:
1. [ ] - `p1` - Split the candidate on the last `~` that terminates the value-type half, giving a left value-type segment and a right instance segment - `inst-gf-key-1`
2. [ ] - `p1` - **IF** no such separator is present → **RETURN** validation problem stating the key must be a GTS instance identifier of the form value-type then instance - `inst-gf-key-2`
3. [ ] - `p1` - Assert the left half is a GTS type id terminated by `~` - `inst-gf-key-3`
4. [ ] - `p1` - Assert the right half is an instance id with no trailing `~` - `inst-gf-key-4`
5. [ ] - `p1` - Validate every dot-separated segment of both halves against the GTS grammar: lowercase, restricted to the permitted character set, and containing no `/` - `inst-gf-key-5`
6. [ ] - `p1` - **IF** any segment violates the grammar → **RETURN** validation problem naming the offending segment - `inst-gf-key-6`
7. [ ] - `p1` - **RETURN** the parsed key value object exposing both halves without re-normalizing the input, so a stored key and a supplied key compare identically - `inst-gf-key-7`

### Optimistic Concurrency Precondition Evaluation

- [ ] `p1` - **ID**: `cpt-cf-settings-service-algo-gear-foundation-precondition`

**Input**: Request `If-Match` header and the current representation of the target resource

**Output**: Proceed verdict, or the precondition problem to return

**Steps**:
1. [ ] - `p1` - **IF** the operation is a mutating `PATCH` or `DELETE` and no `If-Match` header is present → **RETURN** `428` precondition required - `inst-gf-precond-1`
2. [ ] - `p1` - Compute the current ETag from the target's persisted representation - `inst-gf-precond-2`
3. [ ] - `p1` - **IF** the supplied `If-Match` does not equal the current ETag → **RETURN** `412` precondition failed - `inst-gf-precond-3`
4. [ ] - `p1` - **RETURN** proceed, and carry the computed ETag forward so the handler can emit a refreshed value on success - `inst-gf-precond-4`

### Domain Error to Problem Mapping

- [ ] `p1` - **ID**: `cpt-cf-settings-service-algo-gear-foundation-problem-mapping`

**Input**: A `DomainError` raised anywhere in the service

**Output**: An RFC-9457 problem document and its HTTP status

**Steps**:
1. [ ] - `p1` - Map the `DomainError` variant to its HTTP status and its stable `gts://` type URI - `inst-gf-problem-1`
2. [ ] - `p1` - Set `Content-Type` to `application/problem+json` - `inst-gf-problem-2`
3. [ ] - `p1` - Populate the required members `type`, `title`, `status`, and `trace_id`, taking `trace_id` from the ambient request trace context - `inst-gf-problem-3`
4. [ ] - `p1` - **IF** the status is `422` → attach a field-level `errors` array, each entry carrying `field`, `code`, and `message` - `inst-gf-problem-4`
5. [ ] - `p1` - **IF** the error carries an authorization or entitlement denial → emit the denial without disclosing whether the target resource exists - `inst-gf-problem-5`
6. [ ] - `p1` - **IF** the variant is unrecognized → map to `500` with a generic title, never leaking an internal message into the response body - `inst-gf-problem-6`
7. [ ] - `p1` - **RETURN** the problem document and status - `inst-gf-problem-7`

### Authorization Enforcement and Credential Step-Up

- [ ] `p1` - **ID**: `cpt-cf-settings-service-algo-gear-foundation-authz-stepup`

**Input**: Authenticated request context, the target GTS resource type, the required action, and whether the action is behavior-affecting

**Output**: An `AccessScope` for the caller, or a denial

**Steps**:
1. [ ] - `p1` - Require an authenticated principal on the request context - `inst-gf-authz-1`
2. [ ] - `p1` - **IF** authentication is absent or invalid → **RETURN** denial - `inst-gf-authz-2`
3. [ ] - `p1` - Ask the Policy Decision client for a decision on the action against the target GTS resource type - `inst-gf-authz-3`
4. [ ] - `p1` - **IF** the decision cannot be obtained → **RETURN** denial, failing closed rather than proceeding on an unknown verdict - `inst-gf-authz-4`
5. [ ] - `p1` - **IF** the decision is deny → **RETURN** denial - `inst-gf-authz-5`
6. [ ] - `p1` - **IF** the action is behavior-affecting → require a valid credential step-up assertion established at the identity provider - `inst-gf-authz-6`
7. [ ] - `p1` - **IF** the step-up assertion is absent, expired, or not bound to this principal → **RETURN** denial - `inst-gf-authz-7`
8. [ ] - `p1` - Build the `AccessScope` from the decision's constraints - `inst-gf-authz-8`
9. [ ] - `p1` - **RETURN** the `AccessScope` for the handler to apply as a query and visibility predicate - `inst-gf-authz-9`

## 4. States (CDSL)

Not applicable. This feature introduces no domain entity and therefore no lifecycle. Entity state enters the service with `SettingDeclaration` in entry 2.3, which carries `active` and `retired`.

## 5. Definitions of Done

### SDK Crate, Models and Value Objects

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-sdk-models`

The system **MUST** provide a `settings-service-sdk` crate carrying the domain models exchanged with consumers, including the request and response shapes of the reader trait, the effective-value response with its `source` member, the opaque secret handle type, and a parsed setting-key value object. Models **MUST** serialize stably, because consuming gears depend on that wire shape.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-key-parse`

**Constraints**: `cpt-cf-settings-service-constraint-supplied-as-gear`

**Touches**:
- Entities: SDK models, setting-key value object, `SecretHandle`

### SDK Trait Contracts

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-sdk-traits`

The system **MUST** define `SettingsReaderClient` with `get_effective`, `get_effective_bulk`, `resolve_secret`, and `watch`, and `SettingsContributionClient` with `register_declarations` and `retire_declarations`. `get_effective_bulk` **MUST** return an independent per-key outcome so a mixed batch never fails wholesale. The traits **MUST** facade local against remote so `ClientHub` can bind either without the consumer changing.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-gear-init`

**Constraints**: `cpt-cf-settings-service-constraint-supplied-as-gear`

**Touches**:
- Entities: `SettingsReaderClient`, `SettingsContributionClient`

### Error Taxonomy and Reader Degradation Contract

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-error-taxonomy`

The system **MUST** define an error taxonomy whose reader-facing failures are mutually distinguishable: `Unavailable` when the value could not be resolved and a retry may succeed, `Retired` when the declaration was withdrawn and a retry will not help, and `NotFound` when no declaration row exists. The service **MUST NOT** substitute a Schema Default on failure, since the default lives in the same database and is equally unreachable.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-problem-mapping`

**Touches**:
- Entities: error taxonomy

### RFC-9457 Problem Mapping

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-problem-mapping`

The system **MUST** render every 4xx and 5xx response as `application/problem+json` carrying `type` as a `gts://` URI, `title`, `status`, and `trace_id`, and **MUST** include a field-level `errors` array on every `422`. An unrecognized internal error **MUST** map to `500` without leaking an internal message.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-problem-mapping`

**Touches**:
- Entities: `DomainError`, Problem document

### Persistence Adapter and Migration Harness

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-persistence`

The system **MUST** provide SeaORM entity scaffolding over PostgreSQL with `SecureConn` and `DBRunner` wiring and a migration harness that runs outstanding migrations at startup. A failed migration **MUST** prevent the gear from serving traffic rather than leaving a partially migrated schema reachable.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-gear-init`

**Constraints**: `cpt-cf-settings-service-constraint-postgres-primary-storage`

**Touches**:
- Entities: persistence adapter, migration harness

### Gear Scaffold and ClientHub Registration

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-gear-scaffold`

The system **MUST** provide a `#[toolkit::gear]` annotated gear that registers its client traits into `ClientHub` and binds them per the active deployment profile, and **MUST** take bootstrap configuration — database and broker endpoints, service identity, TLS, ports — from ToolKit config at gear init, never from a managed setting.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-gear-init`

**Constraints**: `cpt-cf-settings-service-constraint-supplied-as-gear`

**Touches**:
- Entities: gear scaffold, `ClientHub` registration

### REST and OData Infrastructure

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-rest-odata`

The system **MUST** provide shared `OperationBuilder` wiring, OData `$filter`, `$select`, and `$orderby` parsing against a per-resource field mapping, cursor-based pagination helpers, and `If-Match` and ETag handling returning `428` when the header is missing on a mutating request and `412` when it is stale. An expression referencing an unmapped field or unsupported operator **MUST** be rejected rather than silently ignored.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-precondition`

**Touches**:
- Entities: OData field mapping, pagination cursor, ETag

### Policy Enforcement Point and Step-Up Gate

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-authz-stepup`

The system **MUST** enforce authorization through the `PolicyEnforcer` PEP against the target GTS resource type, **MUST** derive an `AccessScope` from the decision's constraints for handlers to apply as a query and visibility predicate, and **MUST** verify a credential step-up assertion established at the identity provider before any behavior-affecting action. An authorization or entitlement decision that cannot be obtained **MUST** deny.

**Implements**:
- `cpt-cf-settings-service-algo-gear-foundation-authz-stepup`

**Constraints**: `cpt-cf-settings-service-constraint-rbac-policy-enforcer`, `cpt-cf-settings-service-constraint-step-up-at-idp`

**Touches**:
- Entities: `PolicyEnforcer`, `AccessScope`, step-up assertion

### Audit Emitter

- [ ] `p1` - **ID**: `cpt-cf-settings-service-dod-gear-foundation-audit-emitter`

The system **MUST** provide the shared Audit Emitter through which every mutating feature publishes its audit records and domain events, supporting pre-image and post-image capture so later features can audit a mutation's before and after state.

**Touches**:
- Entities: Audit Emitter

## 6. Acceptance Criteria

- [ ] The gear starts against an empty database, runs all migrations to completion, and reports ready
- [ ] A gear start with a failing migration does not serve traffic and surfaces the failure
- [ ] A gear start with a missing required bootstrap value fails at startup rather than falling back to a default
- [ ] `SettingsReaderClient` and `SettingsContributionClient` resolve through `ClientHub` after initialization
- [ ] The same consumer code compiles and runs against both the in-process and the REST binding of `SettingsReaderClient`
- [ ] `get_effective_bulk` returns an independent outcome per key, and one failing key does not fail the others in the batch
- [ ] `Unavailable`, `Retired`, and `NotFound` are distinguishable by a consumer without string matching
- [ ] A reader failure never returns a substituted Schema Default in place of an error
- [ ] A well-formed setting key parses into its value-type half and instance half, and the parsed key round-trips to a byte-identical string
- [ ] A key with no value-type separator, a trailing `~` on the instance half, an uppercase segment, or a `/` in any segment is rejected with a validation problem naming the offending segment
- [ ] Every 4xx and 5xx response carries `Content-Type: application/problem+json` with `type`, `title`, `status`, and `trace_id` populated
- [ ] A `422` response carries a field-level `errors` array with `field`, `code`, and `message` per entry
- [ ] An unrecognized internal error maps to `500` and its response body contains no internal message
- [ ] A mutating `PATCH` or `DELETE` without `If-Match` returns `428`
- [ ] A mutating `PATCH` or `DELETE` with a stale `If-Match` returns `412`
- [ ] An OData expression on an unmapped field or with an unsupported operator is rejected rather than ignored
- [ ] A pagination cursor round-trips and reproduces a stable order across pages
- [ ] A request whose authorization decision cannot be obtained is denied rather than allowed
- [ ] A behavior-affecting action without a valid step-up assertion is denied, and one with a step-up assertion bound to a different principal is also denied
- [ ] An `AccessScope` returned by the PEP is applied as a query predicate and not merely as a post-filter
- [ ] The Audit Emitter records a mutation with both pre-image and post-image available to the caller
