// Created: 2026-08-13 by Constructor Tech
//! Tests for the category wire shapes.
//!
//! Acceptance: FEATURE `category-management.md` §6 — the request and response
//! shapes are a contract consuming clients depend on.

use super::{CategoryDto, CreateCategoryRequest, UpdateCategoryRequest};
use crate::api::precondition::ETag;
use crate::domain::category::{Category, CategoryKey};
use crate::domain::error::DomainError;
use crate::field;
use uuid::Uuid;

fn category() -> Category {
    Category {
        id: Uuid::nil(),
        key: CategoryKey::parse("network").expect("valid"),
        name: "Network".to_owned(),
        description: None,
        domain_affinity: Some("infra".to_owned()),
        sort_order: 3,
        icon: None,
        etag: ETag::new("v1"),
    }
}

#[test]
fn the_response_never_carries_the_etag() {
    // The tag is a response header. Carrying it in the body too would give a
    // client two sources for one precondition, one of which it might send back
    // stale.
    let json = serde_json::to_string(&CategoryDto::from(category())).expect("serializes");
    assert!(!json.contains("etag"), "got `{json}`");
    assert!(!json.contains("v1"), "got `{json}`");
}

#[test]
fn absent_optionals_are_omitted_rather_than_null() {
    let json = serde_json::to_value(CategoryDto::from(category())).expect("serializes");
    assert!(json.get("description").is_none());
    assert!(json.get("icon").is_none());
    assert_eq!(json["domainAffinity"], "infra");
}

#[test]
fn the_wire_shape_is_camel_case() {
    // Consuming clients match on these names; a rename is a breaking change.
    let json = serde_json::to_value(CategoryDto::from(category())).expect("serializes");
    let mut fields: Vec<_> = json.as_object().expect("object").keys().cloned().collect();
    fields.sort();
    assert_eq!(fields, ["domainAffinity", "id", "key", "name", "sortOrder"]);
}

#[test]
fn a_request_refuses_an_unknown_field() {
    // `deny_unknown_fields`: a mistyped `sortOrder` would otherwise be silently
    // dropped and the category created with a weight the caller never chose.
    let err = serde_json::from_value::<CreateCategoryRequest>(serde_json::json!({
        "key": "network", "name": "Network", "sortOrde": 5
    }))
    .expect_err("must not parse");
    assert!(err.to_string().contains("sortOrde"), "got `{err}`");
}

#[test]
fn sort_order_defaults_to_zero() {
    let req: CreateCategoryRequest =
        serde_json::from_value(serde_json::json!({ "key": "network", "name": "Network" }))
            .expect("parses");
    assert_eq!(req.sort_order, 0);
}

#[test]
fn a_request_validates_its_key_on_the_way_in() {
    let req = CreateCategoryRequest {
        key: "net/work".to_owned(),
        name: "Network".to_owned(),
        description: None,
        domain_affinity: None,
        sort_order: 0,
        icon: None,
    };
    match req.into_draft() {
        Err(DomainError::Validation { code, .. }) => {
            assert_eq!(code, field::CATEGORY_KEY_RESERVED_SEPARATOR);
        }
        other => panic!("expected a key violation, got {other:?}"),
    }
}

#[test]
fn update_validates_its_key_the_same_way() {
    let req = UpdateCategoryRequest {
        key: String::new(),
        name: "Network".to_owned(),
        description: None,
        domain_affinity: None,
        sort_order: 0,
        icon: None,
    };
    assert!(req.into_draft().is_err());
}
