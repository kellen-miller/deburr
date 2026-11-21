//! deburr: deterministic cleanup evidence for Go and TypeScript code.

pub mod analysis;
pub mod audit;
pub mod cli;
pub mod compare;
pub mod discover;
pub mod duplication;
pub mod identity;
pub mod provenance;
pub mod render;
pub mod report;
pub mod review;
pub mod syntax;

/// The version this binary reports: `DEBURR_VERSION` when set at build time
/// (release builds), else the Cargo package version.
pub const VERSION: &str = match option_env!("DEBURR_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};
