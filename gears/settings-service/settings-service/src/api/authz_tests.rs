//! Tests for the enforcement point's fail-closed contract.
//!
//! Acceptance criteria: FEATURE `gear-foundation.md` §6 — *a request whose
//! authorization decision cannot be obtained is denied rather than allowed* —
//! and CDSL steps `inst-gf-authz-4` / `inst-gf-authz-5`.
//!
//! Two things are pinned here: the projection every failure of
//! `PermissionEnforcer::access_scope` passes through, and, against a recording
//! decision point, what the enforcer asks and how it answers a refusal.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use authz_resolver_sdk::models::{
    EvaluationRequest, EvaluationResponse, EvaluationResponseContext,
};
use authz_resolver_sdk::pep::ConstraintCompileError;
use authz_resolver_sdk::{AuthZResolverApi, EnforcerError, PolicyEnforcer};
use serde_json::Value;
use toolkit_canonical_errors::{CanonicalError, Problem};
use uuid::Uuid;

use super::{Permission, PermissionEnforcer, deny, resource};
use crate::domain::error::DomainError;
use settings_service_sdk::gts;

fn problem_of(err: DomainError) -> Value {
    serde_json::to_value(Problem::from(CanonicalError::from(err))).expect("serializes")
}

/// Every shape `EnforcerError` can take. Kept as one list so a variant added
/// upstream is a compile error here as well as in the mapping.
fn every_failure() -> Vec<EnforcerError> {
    vec![
        EnforcerError::Denied { deny_reason: None },
        // `EvaluationFailed` carries the canonical error the resolver surfaced;
        // which category it was does not matter to the projection, only that it
        // never becomes an allow.
        EnforcerError::EvaluationFailed(CanonicalError::service_unavailable().create()),
        EnforcerError::EvaluationFailed(CanonicalError::service_unavailable().create()),
        EnforcerError::CompileFailed(ConstraintCompileError::ConstraintsRequiredButAbsent),
        EnforcerError::CompileFailed(ConstraintCompileError::AllConstraintsFailed {
            reason: "unsupported constraint operator".to_owned(),
        }),
    ]
}

#[test]
fn every_enforcement_failure_denies() {
    // The core of the contract. An explicit deny, an unreachable decision
    // point, and an uncompilable constraint set are all denials — there is no
    // failure shape that lets a request through.
    for err in every_failure() {
        let label = format!("{err:?}");
        assert!(
            matches!(
                deny(Permission::CategoryRead, &err),
                DomainError::Unauthorized { .. }
            ),
            "`{label}` must deny"
        );
    }
}

#[test]
fn an_unreachable_decision_point_denies_rather_than_defaults() {
    // Step 4 of the enforcement algorithm, stated as its own test because it is
    // the one an implementation is most tempted to get wrong: an outage in the
    // policy service must not become an outage-shaped allow.
    let err = EnforcerError::EvaluationFailed(CanonicalError::service_unavailable().create());
    assert!(matches!(
        deny(Permission::CategoryRead, &err),
        DomainError::Unauthorized { .. }
    ));
}

#[test]
fn uncompilable_constraints_deny_rather_than_widen() {
    // A decision arrived, but its constraints could not become a predicate.
    // Proceeding would mean applying *no* predicate — broader than anything the
    // policy point could have returned.
    let err = EnforcerError::CompileFailed(ConstraintCompileError::AllConstraintsFailed {
        reason: "unknown constraint operator".to_owned(),
    });
    assert!(matches!(
        deny(Permission::CategoryRead, &err),
        DomainError::Unauthorized { .. }
    ));
}

#[test]
fn all_denials_are_indistinguishable_on_the_wire() {
    // A caller must not learn *why* it was denied. Told apart, the three cases
    // reveal whether the policy service is down, whether a policy names them
    // specifically, and — combined with a probe — whether the resource exists.
    let rendered: Vec<Value> = every_failure()
        .into_iter()
        .map(|err| problem_of(deny(Permission::CategoryRead, &err)))
        .collect();
    let first = &rendered[0];
    for other in &rendered[1..] {
        assert_eq!(first, other, "denials must not be distinguishable");
    }
}

#[test]
fn a_denial_discloses_nothing_about_the_target() {
    let doc = problem_of(deny(
        Permission::CategoryRead,
        &EnforcerError::Denied { deny_reason: None },
    ));
    let rendered = serde_json::to_string(&doc).expect("serializes");
    assert_eq!(doc["status"], 403);
    for leak in [
        "unreachable",
        "connection",
        "constraint",
        "not found",
        "exists",
    ] {
        assert!(
            !rendered.contains(leak),
            "`{leak}` must not appear in a denial: {rendered}"
        );
    }
}

#[test]
fn the_resource_vocabulary_is_the_gts_type_ids() {
    // Policies are written against these ids, so they are a contract with the
    // policy decision point rather than an internal naming choice.
    assert_eq!(
        resource::DECLARATION.name(),
        "gts.cf.core.settings.declaration.v1~"
    );
    assert_eq!(resource::VALUE.name(), "gts.cf.core.settings.value.v1~");
    assert_eq!(
        resource::CATEGORY.name(),
        "gts.cf.core.settings.category.v1~"
    );
}

#[test]
fn a_denial_names_the_resource_that_was_enforced() {
    // Found by running the service: every denial used to carry the declaration
    // type, so a category request was refused with the wrong resource in its
    // context -- not a leak, but wrong metadata for anyone reading a log or an
    // audit trail.
    for permission in Permission::ALL {
        let doc = problem_of(deny(
            *permission,
            &EnforcerError::Denied { deny_reason: None },
        ));
        assert_eq!(
            doc["context"]["resource_type"],
            permission.resource().name(),
            "{permission:?}"
        );
    }
    for (permission, expected) in [
        (Permission::CategoryRead, gts::CATEGORY_SCHEMA),
        (Permission::ValueRead, gts::VALUE_SCHEMA),
        (Permission::DeclarationRead, gts::DECLARATION_SCHEMA),
    ] {
        let doc = problem_of(deny(
            permission,
            &EnforcerError::Denied { deny_reason: None },
        ));
        assert_eq!(doc["context"]["resource_type"], expected);
    }
}

#[test]
fn every_permission_names_a_resource_type_the_gear_publishes() {
    // The role-definition validator resolves a rule's target type through the
    // types registry. The gear registers its resource types as the
    // SDK's stubs and no other; a permission whose resource were spelled any other way would
    // be published pointing at a type nobody registers, and no role could be
    // composed from it.
    let published = [
        gts::CATEGORY_SCHEMA,
        gts::DECLARATION_SCHEMA,
        gts::VALUE_SCHEMA,
    ];
    let collected: Vec<String> = settings_service_sdk::gts_declarations()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    for id in published {
        assert!(
            collected.iter().any(|c| c == id),
            "`{id}` is not in the SDK collector the gear publishes"
        );
    }
    for permission in Permission::ALL {
        let resource = permission.resource();
        assert!(
            published.contains(&resource.name()),
            "{permission:?} names `{}`, which the gear does not publish",
            resource.name()
        );
        assert_eq!(resource.name(), permission.resource_id(), "{permission:?}");
    }
}

#[test]
fn a_permission_keeps_its_wire_spelling() {
    // A policy is written against these strings; renaming an action would
    // silently revoke every grant that names it. Written out in full so a
    // change to the table fails here, whatever the catalogue says.
    use super::Permission as P;
    let expected = [
        (P::CategoryRead, &resource::CATEGORY, "read"),
        (P::CategoryCreate, &resource::CATEGORY, "create"),
        (P::CategoryUpdate, &resource::CATEGORY, "update"),
        (P::CategoryDelete, &resource::CATEGORY, "delete"),
        (P::DeclarationRead, &resource::DECLARATION, "read"),
        (P::DeclarationCreate, &resource::DECLARATION, "create"),
        (P::DeclarationUpdate, &resource::DECLARATION, "update"),
        (P::DeclarationDelete, &resource::DECLARATION, "delete"),
        (P::ValueRead, &resource::VALUE, "read"),
        (P::ValueReadUnmasked, &resource::VALUE, "read_unmasked"),
        (P::ValueWrite, &resource::VALUE, "write"),
        (P::ValueDelegate, &resource::VALUE, "delegate"),
    ];
    assert_eq!(expected.len(), P::ALL.len(), "a permission is not pinned");
    for (permission, resource, action) in expected {
        assert_eq!(
            permission.resource().name(),
            resource.name(),
            "{permission:?}"
        );
        assert_eq!(permission.action(), action, "{permission:?}");
    }
}

/// How the recording decision point answers.
#[derive(Clone, Copy)]
enum Verdict {
    Allow,
    Deny,
    Unreachable,
}

/// A policy decision point that keeps every request it was asked.
struct Recording {
    verdict: Verdict,
    asked: Mutex<Vec<EvaluationRequest>>,
}

impl Recording {
    fn answering(verdict: Verdict) -> Arc<Self> {
        Arc::new(Self {
            verdict,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn take(&self) -> Vec<EvaluationRequest> {
        std::mem::take(&mut *self.asked.lock().expect("lock"))
    }
}

#[async_trait]
impl AuthZResolverApi for Recording {
    async fn evaluate(
        &self,
        _ctx: toolkit_security::PlatformSecurityContext,
        request: EvaluationRequest,
    ) -> Result<EvaluationResponse, CanonicalError> {
        self.asked.lock().expect("lock").push(request);
        match self.verdict {
            Verdict::Allow | Verdict::Deny => Ok(EvaluationResponse {
                decision: matches!(self.verdict, Verdict::Allow),
                context: EvaluationResponseContext::default(),
            }),
            Verdict::Unreachable => Err(CanonicalError::service_unavailable().create()),
        }
    }
}

fn enforcer_over(pdp: &Arc<Recording>) -> PermissionEnforcer {
    let pdp: Arc<dyn AuthZResolverApi> = pdp.clone();
    PermissionEnforcer::over(PolicyEnforcer::new(pdp))
}

fn caller() -> toolkit_security::SecurityContext {
    crate::test_support::context_for(Uuid::from_u128(0x7e))
}

#[tokio::test]
async fn the_enforcer_asks_exactly_the_permission_it_was_given() {
    // The wrapper is the only road from a handler to the decision point, so
    // what it sends is what a policy is matched against: the permission's
    // resource type and action, the id the handler named, and nothing added.
    let pdp = Recording::answering(Verdict::Allow);
    let enforcer = enforcer_over(&pdp);
    let ctx = caller();
    let named = Uuid::from_u128(0x5e77);

    for &permission in Permission::ALL {
        for resource_id in [None, Some(named)] {
            enforcer
                .access_scope(&ctx, permission, resource_id)
                .await
                .unwrap_or_else(|err| panic!("{permission:?} is allowed: {err:?}"));

            let asked = pdp.take();
            assert_eq!(asked.len(), 1, "{permission:?}: one question per call");
            let request = &asked[0];
            assert_eq!(
                request.resource.resource_type,
                permission.resource().name(),
                "{permission:?}"
            );
            assert_eq!(request.action.name, permission.action(), "{permission:?}");
            assert_eq!(request.resource.id, resource_id, "{permission:?}");
            assert!(
                request.resource.properties.is_empty(),
                "{permission:?}: no resource property is sent"
            );
            assert!(
                !request.context.require_constraints,
                "{permission:?}: constraints are not required"
            );
        }
    }
}

#[tokio::test]
async fn a_refused_or_unobtainable_decision_is_a_denial_for_every_permission() {
    // The fail-closed contract, through the enforcer rather than the
    // projection alone: a `false` decision and a decision point that cannot be
    // reached both end as the same `Unauthorized`, naming the resource asked.
    for verdict in [Verdict::Deny, Verdict::Unreachable] {
        let pdp = Recording::answering(verdict);
        let enforcer = enforcer_over(&pdp);
        let ctx = caller();
        for &permission in Permission::ALL {
            let err = enforcer
                .access_scope(&ctx, permission, None)
                .await
                .expect_err("no scope without a positive decision");
            let DomainError::Unauthorized { resource } = err else {
                panic!("{permission:?}: expected a denial, got {err:?}");
            };
            assert_eq!(resource, permission.resource().name(), "{permission:?}");
        }
    }
}
