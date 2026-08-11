// Created: 2026-08-11 by Constructor Tech
// @cpt-algo:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1
//! The setting key value object.
//!
//! A setting key is a GTS instance identifier `<value-type>~<setting-instance-id>`.
//! The left half is a curated value type terminated by `~`; the right half is the
//! setting's own instance id and carries no trailing `~`.
//!
//! Catalog membership of the value type — that it comes from
//! `gts.cf.toolkit.settings.types.*~` — is deliberately **not** checked here. A
//! syntactically valid key may name a value type that does not exist, and
//! resolving that requires the types registry, so it belongs to declaration
//! creation rather than to key parsing.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Prefix every GTS identifier starts with.
pub const GTS_PREFIX: &str = "gts.";

/// Terminator that marks the end of a GTS **type** segment.
pub const TYPE_TERMINATOR: char = '~';

/// Reserved path separator. Never valid inside any GTS segment.
pub const RESERVED_SEPARATOR: char = '/';

/// Why a candidate setting key was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingKeyError {
    /// No `~` present, so the value-type half cannot be separated from the instance half.
    #[error("setting key has no `~` separating the value type from the instance id")]
    MissingSeparator,

    /// The value-type half is empty.
    #[error("the value-type half of the setting key is empty")]
    EmptyValueType,

    /// The instance half is empty.
    #[error("the instance half of the setting key is empty")]
    EmptyInstance,

    /// The instance half ends with `~`, which would make it a type rather than an instance.
    #[error("the instance half must not end with `~`; a trailing `~` marks a GTS type")]
    TrailingSeparator,

    /// The value type is chained, so the key carries more than two halves.
    #[error(
        "the value type `{value_type}` is chained; a setting's value type must be a single catalog type"
    )]
    ChainedValueType {
        /// The chained value type, as supplied.
        value_type: String,
    },

    /// An identifier did not begin with the `gts.` prefix.
    ///
    /// This is an identifier-level fault, not a segment-level one, which is why
    /// it does not travel as a [`SegmentRejection`].
    #[error("`{id}` is not a GTS identifier: it must begin with `{GTS_PREFIX}`")]
    MissingGtsPrefix {
        /// The identifier that lacked the prefix.
        id: String,
    },

    /// A segment violated the GTS grammar.
    #[error("segment `{segment}` is not a valid GTS segment: {reason}")]
    InvalidSegment {
        /// The offending segment, reported so the caller can point at it.
        segment: String,
        /// Why it was rejected.
        reason: SegmentRejection,
    },
}

/// The specific grammar rule a segment broke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentRejection {
    /// Contained an uppercase character.
    Uppercase,
    /// Contained the reserved `/` separator.
    ReservedSeparator,
    /// Contained a character outside the permitted set.
    IllegalCharacter,
    /// Was empty.
    Empty,
}

impl fmt::Display for SegmentRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Uppercase => "must be lowercase",
            Self::ReservedSeparator => "must not contain the reserved `/` separator",
            // `.` separates segments, so it can never appear inside one.
            Self::IllegalCharacter => "contains a character outside [a-z0-9_]",
            Self::Empty => "must not be empty",
        };
        f.write_str(s)
    }
}

/// A parsed setting key.
///
/// Holds the key verbatim: parsing never trims, lowercases, or otherwise
/// normalizes, so a stored key and a supplied key compare byte-identically.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey {
    raw: String,
    /// Byte index of the `~` that terminates the value-type half.
    separator: usize,
}

impl SettingKey {
    /// Parse a candidate setting key.
    ///
    /// The key is split at the **first** `~`: everything up to and including it
    /// is the value type, everything after is the instance id. Settings value
    /// types are flat catalog entries, so a chained value type is rejected
    /// rather than silently mis-split.
    ///
    /// # Errors
    ///
    /// Returns [`SettingKeyError`] when the candidate is not a well-formed
    /// `<value-type>~<instance-id>` pair, naming the offending segment.
    pub fn parse(raw: &str) -> Result<Self, SettingKeyError> {
        // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-1
        let separator = raw
            .find(TYPE_TERMINATOR)
            // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-2
            .ok_or(SettingKeyError::MissingSeparator)?;
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-2
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-1

        // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-3
        if separator == 0 {
            return Err(SettingKeyError::EmptyValueType);
        }
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-3

        // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-4
        let instance = &raw[separator + TYPE_TERMINATOR.len_utf8()..];
        if instance.is_empty() {
            return Err(SettingKeyError::EmptyInstance);
        }
        if instance.ends_with(TYPE_TERMINATOR) {
            return Err(SettingKeyError::TrailingSeparator);
        }
        // A further `~` inside the instance half means the value type was chained,
        // so the split produced a fragment rather than a whole instance id.
        if instance.contains(TYPE_TERMINATOR) {
            return Err(SettingKeyError::ChainedValueType {
                value_type: raw[..=separator].to_owned(),
            });
        }
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-4

        // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-5
        // The value type is a GTS *type*; validate its body without the terminator.
        validate_gts_id(&raw[..separator])?;
        // The setting is a GTS *instance*; per ADR-001 it is a full `gts.`-prefixed id.
        validate_gts_id(instance)?;
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-5

        // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-7
        Ok(Self {
            raw: raw.to_owned(),
            separator,
        })
        // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-7
    }

    /// Compose an admin-authored key.
    ///
    /// Builds the instance id as `gts.<vendor>.toolkit.settings.<category>.<name>.v1`
    /// and joins it to `value_type`, which must already end with `~`.
    ///
    /// # Errors
    ///
    /// Returns [`SettingKeyError`] when `value_type` is not a single well-formed
    /// GTS type, or when any supplied segment breaks the GTS grammar.
    pub fn compose(
        value_type: &str,
        vendor: &str,
        category: &str,
        name: &str,
    ) -> Result<Self, SettingKeyError> {
        // Validate the caller's own inputs before splicing them together, so an
        // error names what the caller passed rather than a synthesized string.
        let body = value_type
            .strip_suffix(TYPE_TERMINATOR)
            .ok_or(SettingKeyError::MissingSeparator)?;
        if body.contains(TYPE_TERMINATOR) {
            return Err(SettingKeyError::ChainedValueType {
                value_type: value_type.to_owned(),
            });
        }
        validate_gts_id(body)?;

        for segment in [vendor, category, name] {
            validate_segment(segment)?;
        }

        let instance = format!("{GTS_PREFIX}{vendor}.toolkit.settings.{category}.{name}.v1");
        Self::parse(&format!("{value_type}{instance}"))
    }

    /// The full key, byte-identical to what was parsed.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The value-type half, including its trailing `~`.
    #[must_use]
    pub fn value_type(&self) -> &str {
        &self.raw[..=self.separator]
    }

    /// The instance-id half, without a trailing `~`.
    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.raw[self.separator + TYPE_TERMINATOR.len_utf8()..]
    }

    /// The category slug embedded in an admin instance id, when present.
    ///
    /// Returns `None` for a module-supplied instance id that does not follow the
    /// admin shape; the reconciler derives the category from its own namespace.
    #[must_use]
    pub fn category_slug(&self) -> Option<&str> {
        self.admin_parts().map(|(category, _)| category)
    }

    /// The leaf name embedded in an admin instance id, when present.
    ///
    /// This is the value uniqueness is enforced on within a category.
    #[must_use]
    pub fn leaf_slug(&self) -> Option<&str> {
        self.admin_parts().map(|(_, name)| name)
    }

    /// The category and leaf segments, but only for the admin instance shape
    /// `gts.<vendor>.toolkit.settings.<category>.<name>.v1`.
    ///
    /// Walks the segment iterator rather than collecting, because both public
    /// accessors sit on the SDK read path.
    fn admin_parts(&self) -> Option<(&str, &str)> {
        let mut segments = self.instance_id().split('.');
        segments.next()?; // gts
        segments.next()?; // vendor
        if segments.next()? != "toolkit" {
            return None;
        }
        if segments.next()? != "settings" {
            return None;
        }
        let category = segments.next()?;
        let name = segments.next()?;
        segments.next()?; // version
        // Anything further means this is not the admin shape.
        if segments.next().is_some() {
            return None;
        }
        Some((category, name))
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SettingKey {
    type Err = SettingKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Serializes as the bare key string, so the wire shape is the key itself.
impl Serialize for SettingKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Deserializes through [`SettingKey::parse`], so a malformed key never enters
/// the type; consumers cannot receive a `SettingKey` that would not round-trip.
impl<'de> Deserialize<'de> for SettingKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Validate a whole GTS identifier body: `gts.`-prefixed, all segments legal.
fn validate_gts_id(candidate: &str) -> Result<(), SettingKeyError> {
    if !candidate.starts_with(GTS_PREFIX) {
        return Err(SettingKeyError::MissingGtsPrefix {
            id: candidate.to_owned(),
        });
    }
    for segment in candidate.split('.') {
        validate_segment(segment)?;
    }
    Ok(())
}

/// Validate one dot-separated GTS segment.
fn validate_segment(segment: &str) -> Result<(), SettingKeyError> {
    let reject = |reason| {
        Err(SettingKeyError::InvalidSegment {
            segment: segment.to_owned(),
            reason,
        })
    };

    // @cpt-begin:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-6
    if segment.is_empty() {
        return reject(SegmentRejection::Empty);
    }
    for ch in segment.chars() {
        // `/` is checked first: it is reserved everywhere, and reporting it as a
        // generic illegal character would hide why it can never be used.
        if ch == RESERVED_SEPARATOR {
            return reject(SegmentRejection::ReservedSeparator);
        }
        if ch.is_ascii_uppercase() {
            return reject(SegmentRejection::Uppercase);
        }
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_') {
            return reject(SegmentRejection::IllegalCharacter);
        }
    }
    // @cpt-end:cpt-cf-settings-service-algo-gear-foundation-key-parse:p1:inst-gf-key-6
    Ok(())
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod key_tests;
