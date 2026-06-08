// === MODULE DEFINITION ===
// ToolKit needs access to the gear struct for instantiation
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
pub mod gear;
pub use gear::FileParserGear;

// === INTERNAL MODULES ===
// WARNING: These gears are internal implementation details!
// They are exposed only for comprehensive testing and should NOT be used by external consumers.
#[doc(hidden)]
pub mod api;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod domain;
#[doc(hidden)]
pub mod infra;
