// Created: 2026-08-13 by Constructor Tech
//! Category management.

pub mod key;
pub mod repo;
pub mod service;
pub mod visibility;

pub use key::CategoryKey;
pub use repo::{Category, CategoryDraft, CategoryRepository};
pub use service::CategoryService;
pub use visibility::{DomainVisibility, domain_visibility, is_visible};
