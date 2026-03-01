//! Project manifest loading (`bundle.toml`) and source-file scanning.
//!
//! ## Design: `bundle.toml` as the Project Root
//!
//! A Nova project is described by a `bundle.toml` file at the project root.
//! The compiler driver locates this file (either at the path the user
//! specified, or by walking up from the current directory), reads the list of
//! source files declared under `source_files`, and stats them from the real
//! file system.
//!
//! The resulting [`ProjectManifest`] is consumed by the driver to:
//! 1. Determine the set of [`FileStat`] inputs for the semantic session.
//! 2. Determine the `target/` directory used for incremental storage.
//!
//! ## Design: Paths Are Project-Relative, Then Canonicalised
//!
//! Paths declared in `bundle.toml` are **relative to the directory that
//! contains `bundle.toml`**.  The driver canonicalises them to absolute
//! UTF-8 strings before handing them to the semantic session so that node
//! IDs derived from paths are stable regardless of the working directory.
//!
//! ## Design: Only Declared Files Are Compiled
//!
//! The compiler does not walk the file system looking for `*.nova` files.
//! Explicit declaration keeps the compilation boundary predictable and
//! mirrors how Rust's `mod` system works.

use std::path::{Path, PathBuf};
use serde::Deserialize;
use nova_analyze::semantic::FileStat;

// ---------------------------------------------------------------------------
// bundle.toml schema
// ---------------------------------------------------------------------------

/// The raw deserialization target for `bundle.toml`.
///
/// In TOML, every key written after a `[table]` header and before the next
/// header belongs to that table.  The real `bundle.toml` places
/// `source_files` **after** the `[package]` header, so it ends up inside
/// `package` from the TOML parser's perspective.
#[derive(Debug, Deserialize)]
struct RawManifest {
    package: RawPackage,
    /// Optional array-of-tables for future dependency support.
    #[serde(default)]
    #[allow(dead_code)]
    dependencies: Vec<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    /// Source files listed under `[package]` in `bundle.toml`.
    ///
    /// In the `bundle.toml` format all project keys (including
    /// `source_files`) follow the `[package]` header, so TOML attaches them
    /// to the `package` table.
    #[serde(default)]
    source_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Parsed and validated project manifest.
#[derive(Debug)]
pub struct ProjectManifest {
    /// Human-readable project name (from `[package] name`).
    pub name: String,
    /// Semver string (from `[package] version`).
    pub version: String,
    /// Optional description string.
    // Retained as public API for future use (e.g. `nvc --info`, docs generation).
    #[allow(dead_code)]
    pub description: String,
    /// Absolute path to the directory containing `bundle.toml`.
    pub root: PathBuf,
    /// Absolute path to `<root>/target/nova-incremental/`.
    ///
    /// The compiler driver creates this directory if it does not exist and
    /// passes it to the loose-file storage backend.
    pub incremental_dir: PathBuf,
    /// Source files in declaration order, with absolute paths.
    pub source_files: Vec<PathBuf>,
}

/// An error that can occur while loading or scanning a project.
#[derive(Debug)]
pub struct ProjectError(pub String);

impl std::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ProjectError {}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------

/// Load the project described by `manifest_path` (a path to `bundle.toml`
/// or to the directory containing it).
///
/// After parsing, all `source_files` paths are resolved to absolute paths
/// relative to the manifest's parent directory.
pub fn load_manifest(manifest_path: &Path) -> Result<ProjectManifest, ProjectError> {
    // Allow the caller to pass either the file itself or its parent directory.
    let manifest_file = if manifest_path.is_dir() {
        manifest_path.join("bundle.toml")
    } else {
        manifest_path.to_path_buf()
    };

    let root = manifest_file
        .parent()
        .ok_or_else(|| ProjectError(format!(
            "cannot determine project root from '{}'",
            manifest_file.display()
        )))?
        .to_path_buf();

    let text = std::fs::read_to_string(&manifest_file).map_err(|e| {
        ProjectError(format!(
            "failed to read '{}': {e}",
            manifest_file.display()
        ))
    })?;

    let raw: RawManifest = toml::from_str(&text).map_err(|e| {
        ProjectError(format!(
            "failed to parse '{}': {e}",
            manifest_file.display()
        ))
    })?;

    // Resolve all source file paths relative to the project root.
    let source_files: Vec<PathBuf> = raw
        .package
        .source_files
        .iter()
        .map(|rel| root.join(rel))
        .collect();

    let incremental_dir = root.join("target").join("nova-incremental");

    Ok(ProjectManifest {
        name: raw.package.name,
        version: raw.package.version,
        description: raw.package.description,
        root,
        incremental_dir,
        source_files,
    })
}

/// Walk up from `start` looking for a `bundle.toml` file.
///
/// Returns the path to `bundle.toml` if found, or `None` if the filesystem
/// root is reached without finding one.
pub fn find_manifest(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join("bundle.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// File stat scanning
// ---------------------------------------------------------------------------

/// Stat all source files declared in `manifest` and return a [`FileStat`]
/// for each one.
///
/// Returns an error if any file cannot be accessed.  The path stored inside
/// each [`FileStat`] is the **canonical absolute** UTF-8 string for the file,
/// which must match what [`crate::real_fs::RealFileAccess`] receives.
pub fn stat_source_files(manifest: &ProjectManifest) -> Result<Vec<FileStat>, ProjectError> {
    let mut stats = Vec::with_capacity(manifest.source_files.len());

    for path in &manifest.source_files {
        let canonical = path.canonicalize().map_err(|e| {
            ProjectError(format!(
                "cannot canonicalize '{}': {e}",
                path.display()
            ))
        })?;

        let canonical_str = canonical.to_str().ok_or_else(|| {
            ProjectError(format!(
                "path '{}' contains non-UTF-8 characters",
                canonical.display()
            ))
        })?;

        let meta = std::fs::metadata(&canonical).map_err(|e| {
            ProjectError(format!(
                "cannot stat '{}': {e}",
                canonical.display()
            ))
        })?;

        let size_bytes = meta.len();
        let modified_secs = meta
            .modified()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            })
            .unwrap_or(0);

        stats.push(FileStat::new(canonical_str, size_bytes, modified_secs));
    }

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_project(dir: &Path) {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("main.nova"), b"val x = 1;").unwrap();

        let manifest = r#"
[package]
name = "test-project"
version = "0.1.0"
description = "test"

source_files = ["src/main.nova"]
"#;
        std::fs::write(dir.join("bundle.toml"), manifest.as_bytes()).unwrap();
    }

    #[test]
    fn load_manifest_parses_fields() {
        let tmp = TempDir::new().unwrap();
        make_project(tmp.path());

        let m = load_manifest(&tmp.path().join("bundle.toml")).unwrap();
        assert_eq!(m.name, "test-project");
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.source_files.len(), 1);
        assert!(m.source_files[0].ends_with("main.nova"));
    }

    #[test]
    fn load_manifest_from_dir() {
        let tmp = TempDir::new().unwrap();
        make_project(tmp.path());
        // Passing the directory instead of the file should work too.
        let m = load_manifest(tmp.path()).unwrap();
        assert_eq!(m.name, "test-project");
    }

    #[test]
    fn stat_source_files_returns_stats() {
        let tmp = TempDir::new().unwrap();
        make_project(tmp.path());

        let m = load_manifest(tmp.path()).unwrap();
        let stats = stat_source_files(&m).unwrap();
        assert_eq!(stats.len(), 1);
        assert!(stats[0].size_bytes > 0);
    }

    #[test]
    fn find_manifest_walks_up() {
        let tmp = TempDir::new().unwrap();
        make_project(tmp.path());

        let sub = tmp.path().join("src");
        let found = find_manifest(&sub).unwrap();
        assert!(found.ends_with("bundle.toml"));
    }
}
