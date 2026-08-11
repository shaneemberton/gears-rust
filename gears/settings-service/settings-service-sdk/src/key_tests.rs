// Created: 2026-08-11 by Constructor Tech
//! Tests for the setting key value object.
//!
//! Contract source: `ADR-001-setting-key-gts-instance-id` — the setting key is a
//! GTS **instance** id `<value-type>~<setting-instance-id>`, admin instance id
//! `gts.<vendor>.toolkit.settings.<category>.<name>.v1`.
//!
//! Acceptance criteria: FEATURE `gear-foundation.md` §6.

use std::str::FromStr;

use super::{SegmentRejection, SettingKey, SettingKeyError};

/// A well-formed admin key, per ADR-001 decision point 3.
const VALID_KEY: &str =
    "gts.cf.toolkit.settings.types.bool_flag.v1~gts.acme.toolkit.settings.network.enable_proxy.v1";
const VALID_VALUE_TYPE: &str = "gts.cf.toolkit.settings.types.bool_flag.v1~";
const VALID_INSTANCE: &str = "gts.acme.toolkit.settings.network.enable_proxy.v1";

#[test]
fn parses_value_type_and_instance_halves() {
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert_eq!(key.value_type(), VALID_VALUE_TYPE);
    assert_eq!(key.instance_id(), VALID_INSTANCE);
}

#[test]
fn value_type_half_retains_its_terminator() {
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert!(
        key.value_type().ends_with('~'),
        "the value type is a GTS type and must keep its trailing `~`"
    );
}

#[test]
fn instance_half_carries_no_terminator() {
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert!(
        !key.instance_id().ends_with('~'),
        "the setting is a GTS instance and must not end with `~`"
    );
}

#[test]
fn round_trips_byte_identically() {
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert_eq!(
        key.as_str(),
        VALID_KEY,
        "parsing must not trim, lowercase, or otherwise normalize the key"
    );
}

#[test]
fn instance_half_keeps_the_gts_prefix() {
    // ADR-001 decision point 3: the admin instance id is itself `gts.`-prefixed.
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert!(
        key.instance_id().starts_with("gts."),
        "per ADR-001 the instance half is a full GTS id, not a bare segment"
    );
}

#[test]
fn rejects_key_without_separator() {
    let err = SettingKey::parse("gts.cf.toolkit.settings.types.bool_flag.v1")
        .expect_err("a key with no `~` must be rejected");
    assert_eq!(err, SettingKeyError::MissingSeparator);
}

#[test]
fn rejects_instance_half_with_trailing_separator() {
    let candidate = format!("{VALID_KEY}~");
    let err = SettingKey::parse(&candidate)
        .expect_err("a trailing `~` makes the right half a type, not an instance");
    assert_eq!(err, SettingKeyError::TrailingSeparator);
}

#[test]
fn rejects_empty_value_type_half() {
    let err = SettingKey::parse("~gts.acme.toolkit.settings.network.enable_proxy.v1")
        .expect_err("an empty value-type half must be rejected");
    assert_eq!(err, SettingKeyError::EmptyValueType);
}

#[test]
fn rejects_empty_instance_half() {
    let err =
        SettingKey::parse(VALID_VALUE_TYPE).expect_err("a bare value type is not a setting key");
    assert_eq!(err, SettingKeyError::EmptyInstance);
}

#[test]
fn rejects_uppercase_segment_naming_the_offender() {
    let candidate =
        "gts.cf.toolkit.settings.types.bool_flag.v1~gts.Acme.toolkit.settings.network.x.v1";
    let err = SettingKey::parse(candidate).expect_err("GTS segments are lowercase");
    match err {
        SettingKeyError::InvalidSegment { segment, reason } => {
            assert_eq!(reason, SegmentRejection::Uppercase);
            assert!(
                segment.contains("Acme"),
                "the error must name the offending segment, got `{segment}`"
            );
        }
        other => panic!("expected InvalidSegment, got {other:?}"),
    }
}

#[test]
fn rejects_reserved_path_separator_naming_the_offender() {
    let candidate =
        "gts.cf.toolkit.settings.types.bool_flag.v1~gts.acme.toolkit.settings.net/work.x.v1";
    let err = SettingKey::parse(candidate).expect_err("`/` is reserved and never valid");
    match err {
        SettingKeyError::InvalidSegment { segment, reason } => {
            assert_eq!(reason, SegmentRejection::ReservedSeparator);
            assert!(
                segment.contains('/'),
                "the error must name the offending segment, got `{segment}`"
            );
        }
        other => panic!("expected InvalidSegment, got {other:?}"),
    }
}

#[test]
fn rejects_instance_half_without_gts_prefix() {
    let candidate = "gts.cf.toolkit.settings.types.bool_flag.v1~acme.toolkit.settings.network.x.v1";
    let err =
        SettingKey::parse(candidate).expect_err("per ADR-001 the instance half is a full GTS id");
    match err {
        // Identifier-level, not segment-level: the whole id lacked the prefix.
        SettingKeyError::MissingGtsPrefix { id } => {
            assert_eq!(id, "acme.toolkit.settings.network.x.v1");
        }
        other => panic!("expected MissingGtsPrefix, got {other:?}"),
    }
}

#[test]
fn rejects_value_type_half_without_gts_prefix() {
    let candidate = "cf.toolkit.settings.types.bool_flag.v1~gts.acme.toolkit.settings.net.x.v1";
    let err = SettingKey::parse(candidate).expect_err("the value type must be a full GTS id");
    match err {
        SettingKeyError::MissingGtsPrefix { id } => {
            assert_eq!(id, "cf.toolkit.settings.types.bool_flag.v1");
        }
        other => panic!("expected MissingGtsPrefix, got {other:?}"),
    }
}

#[test]
fn rejects_chained_value_type() {
    // A second `~` means the value type was chained, so the first-`~` split
    // produced a fragment rather than a whole instance id.
    let err = SettingKey::parse("gts.a.b.v1~c.d.v1~gts.acme.toolkit.settings.net.x.v1")
        .expect_err("a chained value type must be rejected");
    match err {
        SettingKeyError::ChainedValueType { value_type } => {
            assert_eq!(value_type, "gts.a.b.v1~");
        }
        other => panic!("expected ChainedValueType, got {other:?}"),
    }
}

#[test]
fn rejects_empty_inner_segment() {
    let err = SettingKey::parse("gts..x.v1~gts.acme.toolkit.settings.net.x.v1")
        .expect_err("an empty segment must be rejected");
    match err {
        SettingKeyError::InvalidSegment { segment, reason } => {
            assert_eq!(reason, SegmentRejection::Empty);
            assert!(segment.is_empty());
        }
        other => panic!("expected InvalidSegment, got {other:?}"),
    }
}

#[test]
fn composes_admin_key_in_the_adr_shape() {
    let key = SettingKey::compose(VALID_VALUE_TYPE, "acme", "network", "enable_proxy")
        .expect("well-formed inputs must compose");
    assert_eq!(key.as_str(), VALID_KEY);
}

#[test]
fn compose_rejects_uppercase_vendor() {
    let err = SettingKey::compose(VALID_VALUE_TYPE, "Acme", "network", "enable_proxy")
        .expect_err("vendor must be lowercase");
    assert!(matches!(err, SettingKeyError::InvalidSegment { .. }));
}

#[test]
fn compose_rejects_category_containing_reserved_separator() {
    let err = SettingKey::compose(VALID_VALUE_TYPE, "acme", "net/work", "enable_proxy")
        .expect_err("`/` is reserved and never valid in a category slug");
    assert!(matches!(err, SettingKeyError::InvalidSegment { .. }));
}

#[test]
fn exposes_category_and_leaf_slugs_for_admin_keys() {
    // `UNIQUE(category_id, leaf_slug)` is enforced on the leaf, so it must be recoverable.
    let key = SettingKey::parse(VALID_KEY).expect("well-formed key must parse");
    assert_eq!(key.category_slug(), Some("network"));
    assert_eq!(key.leaf_slug(), Some("enable_proxy"));
}

#[test]
fn recategorizing_produces_a_different_key() {
    // ADR-001 decision point 6: the category is embedded, so a move re-keys the setting.
    let before =
        SettingKey::compose(VALID_VALUE_TYPE, "acme", "network", "enable_proxy").expect("composes");
    let after = SettingKey::compose(VALID_VALUE_TYPE, "acme", "security", "enable_proxy")
        .expect("composes");
    assert_ne!(before.as_str(), after.as_str());
}

#[test]
fn compose_rejects_chained_value_type_naming_the_callers_input() {
    // The caller supplied a bad `value_type`; the error must name that, not a
    // synthesized fragment of the joined key.
    let err = SettingKey::compose("gts.a.b.v1~c.d.v1~", "acme", "network", "x")
        .expect_err("a chained value type must be rejected");
    match err {
        SettingKeyError::ChainedValueType { value_type } => {
            assert_eq!(value_type, "gts.a.b.v1~c.d.v1~");
        }
        other => panic!("expected ChainedValueType, got {other:?}"),
    }
}

#[test]
fn compose_rejects_value_type_without_terminator() {
    let err = SettingKey::compose(
        "gts.cf.toolkit.settings.types.bool_flag.v1",
        "acme",
        "n",
        "x",
    )
    .expect_err("a value type must be terminated by `~`");
    assert_eq!(err, SettingKeyError::MissingSeparator);
}

#[test]
fn module_shaped_instance_has_no_admin_slugs() {
    // A module supplies its own instance id; the reconciler derives the category
    // from its namespace, so the admin accessors deliberately return `None`.
    let key =
        SettingKey::parse("gts.cf.toolkit.settings.types.bool_flag.v1~gts.acme.mymod.retries.v1")
            .expect("module-shaped key must still parse");
    assert_eq!(key.category_slug(), None);
    assert_eq!(key.leaf_slug(), None);
}

#[test]
fn from_str_matches_parse() {
    let via_parse = SettingKey::parse(VALID_KEY).expect("parses");
    let via_from_str = SettingKey::from_str(VALID_KEY).expect("parses");
    assert_eq!(via_parse, via_from_str);
}

#[test]
fn serializes_as_the_bare_key_string() {
    let key = SettingKey::parse(VALID_KEY).expect("parses");
    let json = serde_json::to_string(&key).expect("serializes");
    assert_eq!(json, format!("\"{VALID_KEY}\""));
}

#[test]
fn deserializes_through_parse() {
    let json = format!("\"{VALID_KEY}\"");
    let key: SettingKey = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(key.as_str(), VALID_KEY);
}

#[test]
fn deserializing_a_malformed_key_fails() {
    // A `SettingKey` that would not round-trip must never enter the type.
    let err = serde_json::from_str::<SettingKey>("\"not-a-key\"")
        .expect_err("a malformed key must be rejected at deserialization");
    assert!(
        err.to_string().contains('~'),
        "the parse error should surface, got `{err}`"
    );
}
