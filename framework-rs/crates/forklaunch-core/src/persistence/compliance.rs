//! Explicit entity metadata for application-owned compliance adapters.
//!
//! Classification does not enable encryption or authorize data access. Storage
//! adapters must verify these declarations against their schema and enforce
//! tenant boundaries. Unannotated fields have classification `None`; nullability
//! is explicit because Rust types and database nullability need not agree.
//!
//! ```
//! use forklaunch_core::persistence::compliance::{ComplianceLevel, ComplianceMetadata};
//! #[derive(ComplianceMetadata)]
//! #[compliance(entity = "Contact")]
//! struct Contact {
//!     id: String,
//!     #[compliance(level = "pii", nullable, name = "email_address")]
//!     email: Option<String>,
//! }
//! let field = &Contact::FIELDS[1];
//! assert_eq!(Contact::ENTITY, "Contact");
//! assert_eq!(field.storage_name, "email_address");
//! assert_eq!(field.level, ComplianceLevel::Pii);
//! assert!(field.can_anonymize());
//! ```
//!
//! Misspelled classifications fail at compile time:
//! ```compile_fail
//! use forklaunch_core::persistence::compliance::ComplianceMetadata;
//! #[derive(ComplianceMetadata)]
//! struct Contact { #[compliance(level = "personal")] email: String }
//! ```
//!
//! Duplicate storage names also fail:
//! ```compile_fail
//! use forklaunch_core::persistence::compliance::ComplianceMetadata;
//! #[derive(ComplianceMetadata)]
//! struct Contact { id: String, #[compliance(name = "id")] email: String }
//! ```
pub use forklaunch_core_macros::ComplianceMetadata;

/// Classifications shared with TypeScript's explicit compliance registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplianceLevel {
    None,
    Pii,
    Phi,
    Pci,
}

/// A field's Rust name and explicitly declared storage metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComplianceField {
    pub rust_name: &'static str,
    pub storage_name: &'static str,
    pub level: ComplianceLevel,
    pub nullable: bool,
}

impl ComplianceField {
    /// Whether the TypeScript retention policy permits clearing this field.
    /// Adapters must still verify their storage schema before writing null.
    #[must_use]
    pub const fn can_anonymize(&self) -> bool {
        self.nullable && !matches!(self.level, ComplianceLevel::None)
    }
}

/// Metadata can be derived for named structs or implemented by an adapter.
/// Use `#[compliance(crate_path = "renamed_core")]` with a renamed dependency.
pub trait ComplianceMetadata {
    const ENTITY: &'static str;
    const FIELDS: &'static [ComplianceField];
}
