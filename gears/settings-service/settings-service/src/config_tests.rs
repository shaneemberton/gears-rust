// Created: 2026-08-12 by Constructor Tech
//! Tests for the fail-closed bootstrap contract.
//!
//! Acceptance criteria: FEATURE `gear-foundation.md` §6 — *a gear start with a
//! missing required bootstrap value fails at startup rather than falling back to
//! a default* — and CDSL step `inst-gf-init-2`.

use super::SettingsServiceConfig;

fn parse(json: serde_json::Value) -> Result<SettingsServiceConfig, serde_json::Error> {
    serde_json::from_value(json)
}

#[test]
fn a_complete_config_parses() {
    let cfg = parse(serde_json::json!({
        "jwks_endpoint": "https://idp.example/.well-known/jwks.json",
        "step_up_freshness_seconds": 300,
        "cache_ttl_seconds": 15,
    }))
    .expect("parses");
    assert_eq!(
        cfg.jwks_endpoint,
        "https://idp.example/.well-known/jwks.json"
    );
    assert_eq!(cfg.step_up_freshness_seconds, 300);
    assert_eq!(cfg.cache_ttl_seconds, 15);
}

#[test]
fn a_missing_jwks_endpoint_is_refused() {
    // Not defaulted to empty, not defaulted to a well-known URL. Without it the
    // Apply path cannot verify step-up, and a service that came up anyway would
    // accept applies it cannot vouch for.
    let err =
        parse(serde_json::json!({ "step_up_freshness_seconds": 300 })).expect_err("must not parse");
    assert!(
        err.to_string().contains("jwks_endpoint"),
        "the error must name the absent field, got `{err}`"
    );
}

#[test]
fn a_missing_step_up_window_is_refused() {
    let err = parse(serde_json::json!({ "jwks_endpoint": "https://idp.example/jwks" }))
        .expect_err("must not parse");
    assert!(
        err.to_string().contains("step_up_freshness_seconds"),
        "the error must name the absent field, got `{err}`"
    );
}

#[test]
fn an_empty_config_is_refused_rather_than_wholly_defaulted() {
    // The struct-level `#[serde(default)]` that every other gear carries would
    // make this succeed and start the service with invented security settings.
    assert!(parse(serde_json::json!({})).is_err());
}

#[test]
fn the_cache_backstop_defaults_because_the_design_fixes_it() {
    // The one value that is not a deployment decision. DESIGN.md §4.2 sets 30s.
    let cfg = parse(serde_json::json!({
        "jwks_endpoint": "https://idp.example/jwks",
        "step_up_freshness_seconds": 300,
    }))
    .expect("parses without the optional field");
    assert_eq!(cfg.cache_ttl_seconds, 30);
}

#[test]
fn a_mistyped_key_is_refused_rather_than_ignored() {
    // Without `deny_unknown_fields` this would start the service with the
    // default window while the operator believed they had set 60s.
    let err = parse(serde_json::json!({
        "jwks_endpoint": "https://idp.example/jwks",
        "step_up_freshness_seconds": 300,
        "step_up_freshness_second": 60,
    }))
    .expect_err("must not parse");
    assert!(
        err.to_string().contains("step_up_freshness_second"),
        "the error must name the unknown key, got `{err}`"
    );
}
