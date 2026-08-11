// Created: 2026-08-11 by Constructor Tech
//! Settings Service SDK
//!
//! Public contract for the `settings-service` gear.
//!
//! Phase 1 of the gear-foundation feature delivers:
//! - [`SettingKey`] — the parsed setting key value object
//! - the opaque [`SecretHandle`] and the [`EffectiveSource`] vocabulary
//!
//! The reader and contribution traits, the error taxonomy, and the reader
//! degradation contract arrive in phase 2.
//!
//! # Setting key shape
//!
//! A setting's key is a GTS **instance** identifier of the form
//! `<value-type>~<setting-instance-id>`:
//!
//! ```text
//! gts.cf.toolkit.settings.types.bool_flag.v1~gts.acme.toolkit.settings.network.enable_proxy.v1
//! └──────────── value type, ends `~` ───────┘└────────── instance id, no trailing `~` ────────┘
//! ```
//!
//! The left half is a curated value type from the `gts.cf.toolkit.settings.types.*~`
//! catalog and is the **only** part registered in GTS. The setting itself is an
//! unregistered GTS instance living in the Settings DB.

#![forbid(unsafe_code)]
#![deny(rust_2018_idioms)]

pub mod key;
pub mod models;

pub use key::{SettingKey, SettingKeyError};
pub use models::{EffectiveSource, SecretHandle};
