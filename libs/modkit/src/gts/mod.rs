//! GTS re-exports from `modkit-gts`.
//!
//! Platform base GTS types (plugin base, permission base, and future
//! role/grant/binding) live in the dedicated `modkit-gts` crate alongside
//! the link-time inventory machinery. This module re-exports them for
//! convenience so existing consumers can continue writing
//! `use modkit::gts::PluginV1;` without an extra dependency.
//!
//! A deprecated type alias is provided for the former `BaseModkitPluginV1`
//! name to preserve backward compatibility of the published `cf-modkit`
//! crate for external consumers. In-repo callsites have been migrated to
//! `PluginV1`.

pub use modkit_gts::{
    AuthzPermissionV1, InventoryInstance, InventoryTypeSchema, PluginV1, all_inventory_instances,
    all_inventory_type_schemas,
};

/// Deprecated alias — `BaseModkitPluginV1` was renamed to [`PluginV1`].
///
/// The "Base" prefix was redundant with the type's role (every
/// `struct_to_gts_schema(base = true)` struct is a base type); "Modkit" was
/// then dropped since the crate path (`modkit::gts::`) already carries that
/// context. Callsites in this workspace have all been migrated. This alias
/// is retained to keep the published crate's surface backward-compatible
/// for external consumers.
#[deprecated(since = "0.7.0", note = "Renamed to `PluginV1`")]
pub type BaseModkitPluginV1<P> = PluginV1<P>;
