//! Bundle descriptor — the parsed form of a `bundle.toml` file.
//!
//! A **bundle** is the minimal distribution unit of compiled Nova sources.
//! Every bundle has a `bundle.toml` that declares:
//! - its identity (`name`, `version`, …)
//! - which source files it contains
//! - which other bundles it depends on
//!
//! # Example `bundle.toml`
//! ```toml
//! [package]
//! name    = "my-lib"
//! version = "1.0.0"
//!
//! source_files = ["src/lib.nova", "src/util.nova"]
//!
//! [[dependencies]]
//! name    = "std"
//! version = "^1.0"
//! ```
//!
//! # Parsing
//! Call [`BundleDescriptor::from_str`] with the raw TOML text, or
//! [`BundleDescriptor::from_file`] with a [`SourcePath`] via the host
//! [`Workspace`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::semantic::host::SourcePath;

// ── Wire types (match the TOML schema exactly) ────────────────────────────────

/// Raw `[package]` table from `bundle.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    /// Bundle name, e.g. `"com.example.my-lib"`.
    pub name: String,
    /// SemVer string, e.g. `"1.0.0"`.
    pub version: String,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: String,
    /// Optional list of author strings.
    #[serde(default)]
    pub authors: Vec<String>,
    /// Optional SPDX licence identifier.
    #[serde(default)]
    pub license: String,
    /// Optional URL to source repository.
    #[serde(default)]
    pub repository: Option<String>,
}

/// One `[[dependencies]]` entry in `bundle.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEntry {
    /// Bundle name of the dependency.
    pub name: String,
    /// SemVer version requirement string, e.g. `"^1.0"`, `"=2.1.0"`.
    pub version: String,
    /// Local path to the dependency's directory (optional — takes precedence
    /// over registry resolution when present).
    #[serde(default)]
    pub path: Option<String>,
    /// Whether this dependency is optional (defaults to `false`).
    #[serde(default)]
    pub optional: bool,
}

/// The raw on-disk shape of `bundle.toml`, before any semantic processing.
///
/// Field names mirror the TOML keys verbatim so that `toml::from_str` works
/// with zero custom deserialization logic.
#[derive(Debug, Clone, Deserialize)]
struct RawBundleDescriptor {
    package: PackageMeta,
    /// Relative paths to `.nova` source files, relative to the bundle root.
    #[serde(default)]
    source_files: Vec<String>,
    /// Dependency entries.
    #[serde(default)]
    dependencies: Vec<DependencyEntry>,
}

// ── Public descriptor type ────────────────────────────────────────────────────

/// The parsed, validated form of a `bundle.toml` file.
///
/// Each `BundleDescriptor` is assigned a stable [`Uuid`] at construction
/// time, derived from the bundle's `name` and `version`.  This UUID is used
/// as the artifact storage key for compiled outputs belonging to this bundle.
///
/// # UUID derivation
/// The UUID is a **version-5 UUID** (SHA-1 namespace hash) over the string
/// `"<name>@<version>"` in the DNS namespace.  The same name+version always
/// produces the same UUID, which is essential for incremental cache hits
/// across separate compiler invocations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleDescriptor {
    /// Stable artifact-storage key for this bundle.
    pub id: Uuid,
    /// Identity and metadata from the `[package]` table.
    pub package: PackageMeta,
    /// Source file paths relative to the bundle root directory.
    pub source_files: Vec<SourcePath>,
    /// Declared dependencies.
    pub dependencies: Vec<DependencyEntry>,
}

impl BundleDescriptor {
    /// Parse a `BundleDescriptor` from raw TOML text.
    ///
    /// # Errors
    /// Returns a descriptive string if the TOML is malformed or missing
    /// required fields.
    pub fn from_str(toml_text: &str) -> Result<Self, String> {
        let raw: RawBundleDescriptor = toml::from_str(toml_text)
            .map_err(|e| format!("bundle.toml parse error: {e}"))?;

        let id = bundle_uuid(&raw.package.name, &raw.package.version);
        let source_files = raw.source_files.into_iter().map(SourcePath).collect();

        Ok(BundleDescriptor {
            id,
            package: raw.package,
            source_files,
            dependencies: raw.dependencies,
        })
    }
}

// ── UUID derivation helper ────────────────────────────────────────────────────

/// Derive a stable v5 UUID for a bundle from its name and version.
///
/// Uses the DNS namespace UUID as the namespace argument (an arbitrary but
/// stable choice — the actual namespace semantics are not relevant here).
///
/// This is `pub(crate)` so that [`Repository`](super::repository::Repository)
/// can use the same function when generating the repository's own UUID.
pub(crate) fn bundle_uuid(name: &str, version: &str) -> Uuid {
    Uuid::new_v5(&Uuid::NAMESPACE_DNS, format!("{name}@{version}").as_bytes())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
[package]
name        = "sample-project"
version     = "0.1.0"
description = "A sample Nova project"
authors     = ["Nova Team"]

source_files = [
    "src/main.nova",
    "src/math_utils.nova",
    "src/data_structures.nova"
]

[[dependencies]]
name    = "std"
version = "1.0.0"
path    = "../stdlib"

[[dependencies]]
name     = "graphics"
version  = "2.1.0"
optional = true
"#;

    #[test]
    fn parses_package_meta() {
        let desc = BundleDescriptor::from_str(SAMPLE).unwrap();
        assert_eq!(desc.package.name, "sample-project");
        assert_eq!(desc.package.version, "0.1.0");
        assert_eq!(desc.package.authors, vec!["Nova Team"]);
    }

    #[test]
    fn parses_source_files() {
        let desc = BundleDescriptor::from_str(SAMPLE).unwrap();
        assert_eq!(desc.source_files.len(), 3);
        assert_eq!(desc.source_files[0].0, "src/main.nova");
    }

    #[test]
    fn parses_dependencies() {
        let desc = BundleDescriptor::from_str(SAMPLE).unwrap();
        assert_eq!(desc.dependencies.len(), 2);
        assert_eq!(desc.dependencies[0].name, "std");
        assert_eq!(desc.dependencies[0].path, Some("../stdlib".into()));
        assert!(!desc.dependencies[0].optional);
        assert!(desc.dependencies[1].optional);
    }

    #[test]
    fn id_is_stable_across_calls() {
        let a = BundleDescriptor::from_str(SAMPLE).unwrap();
        let b = BundleDescriptor::from_str(SAMPLE).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn different_versions_produce_different_ids() {
        let v1 = bundle_uuid("pkg", "1.0.0");
        let v2 = bundle_uuid("pkg", "2.0.0");
        assert_ne!(v1, v2);
    }

    #[test]
    fn rejects_missing_package_table() {
        let bad = r#"source_files = ["src/main.nova"]"#;
        assert!(BundleDescriptor::from_str(bad).is_err());
    }

    #[test]
    fn rejects_malformed_toml() {
        assert!(BundleDescriptor::from_str("[[[[").is_err());
    }
}
