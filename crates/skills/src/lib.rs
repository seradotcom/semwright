//! Open Agent Skills package tooling.
//!
//! Skills are untrusted procedural guidance. This crate validates, inspects, packages and
//! resolves their declared Semwright dependencies, but it never executes Skill scripts and
//! never grants broker authority.

mod bundle;
mod compat;
mod model;
mod package;
mod requirements;

pub use bundle::{BundleReport, bundle};
pub use compat::{
    AuthoritySummary, CapabilityMatch, CompatibilityReport, Drift, ExampleReport,
    RequirementReport, SkillTestReport, capability_ids_from_search, catalog_capability_from_broker,
    conformance_test, doctor, lock, test_examples,
};
pub use model::*;
pub use package::{
    ValidationReport, discover, export_capability, load, parse_skill_text, scaffold,
    valid_skill_name, validate, validate_archive_path,
};
pub use requirements::{
    load_lock, load_requirements, parse_requirements_bytes, requirements_digest, write_lock,
};

pub const REQUIREMENTS_SCHEMA: &str = include_str!("../../../schemas/skill-requirements-v1.json");
pub const LOCK_SCHEMA: &str = include_str!("../../../schemas/skill-lock-v1.json");
pub const REPORT_SCHEMA: &str = include_str!("../../../schemas/skill-report-v1.json");

#[cfg(test)]
mod tests;
