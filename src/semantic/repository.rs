//! Repository descriptor and the `Repository` / `Bundle` runtime types.
//!
//! # Concepts
//!
//! ## Repository
//! A **repository** is a collection of bundles' sources and metadata
//! required to build them.  It is the Nova analogue of a Cargo workspace.
//! The repository is described by a `nova-workspace.toml` file that lists
//! the paths to each member bundle's root directory.
//!
//! Example `nova-workspace.toml`:
//! ```toml
//! [workspace]
//! members = [
//!     "core",
//!     "utils",
//!     "app",
//! ]
//! ```
//!
//! ## Bundle
//! A **bundle** is the minimal distribution unit of compiled Nova sources,
//! fully described by its `bundle.toml`.  Inside the repository each bundle
//! is assigned a stable [`Uuid`] (derived from its name + version) used as
//! the artifact storage key for its compiled outputs.
//!
//! ## Repository artifact UUID
//! The repository itself is stored under a **fixed, well-known UUID**:
//! [`REPOSITORY_ARTIFACT_KEY`].  This allows the host's [`BlobStorage`] to
//! cache and retrieve the repository-level artifact without any additional
//! lookup.
//!
//! # Parsing
//! [`RepositoryDescriptor::from_str`] parses a `nova-workspace.toml`.
//! [`Repository::from_descriptor`] resolves each member path to a
//! [`Bundle`] by loading its `bundle.toml` via the [`Workspace`] host trait.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::semantic::bundle::{bundle_uuid, BundleDescriptor, DependencyEntry};
use crate::semantic::host::{SourcePath, Workspace, HostError};

// ── Well-known repository artifact key ───────────────────────────────────────

/// Fixed UUID under which the repository-level artifact is stored in
/// [`BlobStorage`](crate::semantic::host::BlobStorage).
///
/// Being fixed (not derived from content) means the host can always locate
/// the latest repository artifact without knowing its content hash in
/// advance.  Stale entries are overwritten on each successful repository
/// build.
pub const REPOSITORY_ARTIFACT_KEY: Uuid =
    Uuid::from_bytes([0x4e, 0x6f, 0x76, 0x61, 0x52, 0x65, 0x70, 0x6f,
                      0x52, 0x6f, 0x6f, 0x74, 0x00, 0x00, 0x00, 0x01]);

// ── Repository descriptor (wire type) ────────────────────────────────────────

/// Raw `[workspace]` table from `nova-workspace.toml`.
#[derive(Debug, Clone, Deserialize)]
struct WorkspaceMeta {
    /// Paths to member bundle root directories, relative to the workspace
    /// root.
    #[serde(default)]
    members: Vec<String>,
}

/// Raw on-disk shape of `nova-workspace.toml`.
#[derive(Debug, Clone, Deserialize)]
struct RawRepositoryDescriptor {
    workspace: WorkspaceMeta,
}

/// The parsed form of a `nova-workspace.toml` file.
///
/// This is the lightweight, allocation-cheap descriptor.  Call
/// [`Repository::from_descriptor`] (async, requires a [`Workspace`] host)
/// to resolve each member path to a full [`Bundle`].
#[derive(Debug, Clone)]
pub struct RepositoryDescriptor {
    /// Paths to member bundle root directories (relative to workspace root).
    pub members: Vec<SourcePath>,
}

impl RepositoryDescriptor {
    /// Parse a `RepositoryDescriptor` from raw TOML text.
    ///
    /// # Errors
    /// Returns a descriptive string if the TOML is malformed or missing the
    /// `[workspace]` table.
    pub fn from_str(toml_text: &str) -> Result<Self, String> {
        let raw: RawRepositoryDescriptor = toml::from_str(toml_text)
            .map_err(|e| format!("nova-workspace.toml parse error: {e}"))?;
        Ok(RepositoryDescriptor {
            members: raw.workspace.members.into_iter().map(SourcePath).collect(),
        })
    }
}

// ── Bundle runtime type ───────────────────────────────────────────────────────

/// A fully-resolved bundle within a repository.
///
/// Each `Bundle` holds its parsed [`BundleDescriptor`] plus a stable
/// [`Uuid`] artifact key.  The UUID is derived from the bundle's
/// name + version via [`bundle_uuid`], so it is stable across runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Stable artifact-storage key for this bundle's compiled outputs.
    pub id: Uuid,
    /// The parsed `bundle.toml` for this bundle.
    pub descriptor: BundleDescriptor,
    /// Absolute-style path to this bundle's root directory (as understood
    /// by the host [`Workspace`]).
    pub root: SourcePath,
}

impl Bundle {
    /// Construct a `Bundle` from its root path and descriptor.
    pub fn new(root: SourcePath, descriptor: BundleDescriptor) -> Self {
        let id = descriptor.id;
        Bundle { id, descriptor, root }
    }

    /// The bundle's name (convenience accessor).
    pub fn name(&self) -> &str {
        &self.descriptor.package.name
    }

    /// The bundle's version string (convenience accessor).
    pub fn version(&self) -> &str {
        &self.descriptor.package.version
    }

    /// The source file paths for this bundle, resolved relative to
    /// [`Bundle::root`].
    ///
    /// Returns paths of the form `"<root>/<relative>"` by joining the root
    /// with each entry in [`BundleDescriptor::source_files`].
    pub fn resolved_source_paths(&self) -> Vec<SourcePath> {
        self.descriptor.source_files.iter().map(|rel| {
            let root = self.root.0.trim_end_matches('/');
            SourcePath(format!("{}/{}", root, rel.0))
        }).collect()
    }

    /// The declared dependencies of this bundle.
    pub fn dependencies(&self) -> &[DependencyEntry] {
        &self.descriptor.dependencies
    }
}

// ── Repository runtime type ───────────────────────────────────────────────────

/// A fully-resolved repository: the collection of all member [`Bundle`]s.
///
/// The repository is stored under [`REPOSITORY_ARTIFACT_KEY`] in the host
/// blob storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    /// All member bundles, in the order they appear in `nova-workspace.toml`.
    pub bundles: Vec<Bundle>,
}

impl Repository {
    /// Resolve a [`RepositoryDescriptor`] into a full `Repository` by loading
    /// each member's `bundle.toml` via the host [`Workspace`].
    ///
    /// For each member path `p`, the workspace is asked for the file at
    /// `"<p>/bundle.toml"`.  The blob is UTF-8 decoded and parsed as a
    /// [`BundleDescriptor`].
    ///
    /// # Errors
    /// Returns [`HostError`] if any member's `bundle.toml` cannot be read
    /// or parsed.
    pub async fn from_descriptor(
        descriptor: &RepositoryDescriptor,
        workspace: &dyn Workspace,
    ) -> Result<Self, HostError> {
        let mut bundles = Vec::with_capacity(descriptor.members.len());

        for member in &descriptor.members {
            let manifest_path = SourcePath(
                format!("{}/bundle.toml", member.0.trim_end_matches('/'))
            );

            let blob = workspace.blob(&manifest_path).await?;
            let text = std::str::from_utf8(&blob)
                .map_err(|e| HostError::BlobReadError {
                    path: manifest_path.clone(),
                    reason: format!("bundle.toml is not valid UTF-8: {e}"),
                })?;

            let bundle_desc = BundleDescriptor::from_str(text)
                .map_err(|reason| HostError::BlobReadError {
                    path: manifest_path,
                    reason,
                })?;

            bundles.push(Bundle::new(member.clone(), bundle_desc));
        }

        Ok(Repository { bundles })
    }

    /// Look up a bundle by name.  Returns the first bundle whose
    /// [`Bundle::name`] matches.
    pub fn find_bundle(&self, name: &str) -> Option<&Bundle> {
        self.bundles.iter().find(|b| b.name() == name)
    }

    /// The well-known UUID under which the repository artifact is stored.
    pub const fn artifact_key() -> Uuid {
        REPOSITORY_ARTIFACT_KEY
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RepositoryDescriptor ──────────────────────────────────────────────

    #[test]
    fn parses_workspace_members() {
        let toml = r#"
[workspace]
members = ["core", "utils", "app"]
"#;
        let desc = RepositoryDescriptor::from_str(toml).unwrap();
        assert_eq!(desc.members.len(), 3);
        assert_eq!(desc.members[0].0, "core");
        assert_eq!(desc.members[2].0, "app");
    }

    #[test]
    fn empty_members_is_valid() {
        let toml = "[workspace]\n";
        let desc = RepositoryDescriptor::from_str(toml).unwrap();
        assert!(desc.members.is_empty());
    }

    #[test]
    fn rejects_missing_workspace_table() {
        assert!(RepositoryDescriptor::from_str("members = []").is_err());
    }

    #[test]
    fn rejects_malformed_toml() {
        assert!(RepositoryDescriptor::from_str("[[[[").is_err());
    }

    // ── Bundle ────────────────────────────────────────────────────────────

    fn sample_bundle() -> Bundle {
        let desc = BundleDescriptor::from_str(r#"
[package]
name    = "my-lib"
version = "1.2.3"

source_files = ["src/lib.nova", "src/util.nova"]
"#).unwrap();
        Bundle::new(SourcePath("libs/my-lib".into()), desc)
    }

    #[test]
    fn bundle_id_matches_descriptor_id() {
        let b = sample_bundle();
        assert_eq!(b.id, b.descriptor.id);
    }

    #[test]
    fn bundle_id_is_stable() {
        let a = sample_bundle();
        let b = sample_bundle();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn bundle_name_and_version_accessors() {
        let b = sample_bundle();
        assert_eq!(b.name(), "my-lib");
        assert_eq!(b.version(), "1.2.3");
    }

    #[test]
    fn resolved_source_paths_join_root() {
        let b = sample_bundle();
        let paths = b.resolved_source_paths();
        assert_eq!(paths[0].0, "libs/my-lib/src/lib.nova");
        assert_eq!(paths[1].0, "libs/my-lib/src/util.nova");
    }

    // ── Repository ────────────────────────────────────────────────────────

    #[test]
    fn repository_artifact_key_is_fixed() {
        // The key must never change — cached artifacts depend on it.
        assert_eq!(
            Repository::artifact_key().to_string(),
            "4e6f7661-5265-706f-5272-6f6f74000001"
        );
    }

    #[test]
    fn repository_find_bundle() {
        let repo = Repository {
            bundles: vec![sample_bundle()],
        };
        assert!(repo.find_bundle("my-lib").is_some());
        assert!(repo.find_bundle("missing").is_none());
    }
}
