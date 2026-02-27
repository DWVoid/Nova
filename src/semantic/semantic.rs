use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::semantic::host::{Blob, HostEnv, HostError, SourcePath};
use crate::semantic::repository::{Repository, RepositoryDescriptor, REPOSITORY_ARTIFACT_KEY};
use crate::syntax::SyntaxResult;

// ── Per-bundle semantic artifact ──────────────────────────────────────────────

/// The serialisable artifact produced for a single bundle.
///
/// Stored under the bundle's UUID key in [`BlobStorage`].
/// Currently holds the parsed syntax trees for every source file;
/// richer semantic data (resolved names, inferred types, …) will be added
/// as later passes are implemented.
///
/// Encoded as **MessagePack** via `rmp_serde` for compact, fast
/// serialisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleArtifact {
    /// The bundle name (for debugging / cache-miss messages).
    pub bundle_name: String,
    /// Parsed chunks for each source file, keyed by their [`SourcePath`].
    pub sources: Vec<(SourcePath, crate::syntax::ast::Chunk)>,
}

// ── Top-level result ──────────────────────────────────────────────────────────

/// The value returned by [`transform`] on success.
#[derive(Debug)]
pub struct SemanticResult {
    /// The resolved repository (either loaded from cache or freshly built).
    pub repository: Repository,
    /// The per-bundle artifacts, in the same order as `repository.bundles`.
    pub bundles: Vec<BundleArtifact>,
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can arise during the semantic transform step.
#[derive(Debug)]
pub enum SemanticError {
    /// An underlying host operation failed.
    Host(HostError),
    /// MessagePack encoding failed.
    Encode(String),
    /// MessagePack decoding failed.
    Decode(String),
}

impl From<HostError> for SemanticError {
    fn from(e: HostError) -> Self { SemanticError::Host(e) }
}

// ── Msgpack helpers ───────────────────────────────────────────────────────────

/// Encode `value` as a MessagePack blob.
fn encode<T: Serialize>(value: &T) -> Result<Blob, SemanticError> {
    rmp_serde::to_vec(value)
        .map(|v| Arc::new(v))
        .map_err(|e| SemanticError::Encode(e.to_string()))
}

/// Decode a MessagePack blob back into `T`.
fn decode<T: for<'de> Deserialize<'de>>(blob: &[u8]) -> Result<T, SemanticError> {
    rmp_serde::from_slice(blob)
        .map_err(|e| SemanticError::Decode(e.to_string()))
}

// ── transform ─────────────────────────────────────────────────────────────────

/// Run the semantic transform for the repository accessible through `host`.
///
/// # Steps
///
/// 1. **Repository** — attempt to load a cached [`Repository`] from blob
///    storage under [`REPOSITORY_ARTIFACT_KEY`].  On a cache miss, read
///    `nova-workspace.toml` from the workspace, resolve all member bundles,
///    encode the result as MessagePack, and store it.
///
/// 2. **Per-bundle sources** — for each bundle in the repository, attempt
///    to load a cached [`BundleArtifact`] under the bundle's UUID key.
///    On a cache miss, parse every source file listed in the bundle
///    descriptor via the workspace, encode the artifact as MessagePack, and
///    store it.
///
/// # Incremental behaviour
/// Any bundle whose UUID key already exists in storage is skipped
/// entirely — its source files are not re-parsed.  Invalidation (evicting
/// stale entries) is the responsibility of the host: bump the bundle's
/// version in `bundle.toml` to get a fresh UUID and force a full rebuild.
pub async fn transform(host: Arc<HostEnv>) -> Result<SemanticResult, SemanticError> {
    // ── Step 1: Repository ────────────────────────────────────────────────
    let repository = load_or_build_repository(&host).await?;

    // ── Step 2: Per-bundle artifacts ──────────────────────────────────────
    let mut bundle_artifacts = Vec::with_capacity(repository.bundles.len());
    for bundle in &repository.bundles {
        let artifact = load_or_build_bundle(&host, bundle).await?;
        bundle_artifacts.push(artifact);
    }

    Ok(SemanticResult {
        repository,
        bundles: bundle_artifacts,
    })
}

// ── Private helpers ───────────────────────────────────────────────────────────

async fn load_or_build_repository(host: &HostEnv) -> Result<Repository, SemanticError> {
    let descriptor_path = SourcePath("nova-workspace.toml".into());

    // Evict the cached repository if the workspace descriptor has changed.
    if host.workspace.has_changed(&descriptor_path).await? {
        host.storage.evict(REPOSITORY_ARTIFACT_KEY).await?;
    } else if let Some(blob) = host.storage.get(REPOSITORY_ARTIFACT_KEY).await? {
        return decode::<Repository>(&blob);
    }

    // Cache miss (or stale eviction): build from the workspace descriptor.
    let toml_blob = host.workspace.blob(&descriptor_path).await?;
    let toml_text = std::str::from_utf8(&toml_blob)
        .map_err(|e| HostError::BlobReadError {
            path: descriptor_path.clone(),
            reason: format!("nova-workspace.toml is not valid UTF-8: {e}"),
        })?;

    let descriptor = RepositoryDescriptor::from_str(toml_text)
        .map_err(|reason| HostError::BlobReadError {
            path: descriptor_path,
            reason,
        })?;

    let repository = Repository::from_descriptor(&descriptor, host.workspace.as_ref()).await?;

    // Persist the repository artifact.
    host.storage.put(REPOSITORY_ARTIFACT_KEY, encode(&repository)?).await?;

    Ok(repository)
}

async fn load_or_build_bundle(
    host: &HostEnv,
    bundle: &crate::semantic::repository::Bundle,
) -> Result<BundleArtifact, SemanticError> {
    let source_paths = bundle.resolved_source_paths();

    // Check whether any source file has changed since the artifact was built.
    // We do this before the cache lookup so a stale artifact is never returned.
    let any_changed = {
        let mut changed = false;
        for path in &source_paths {
            if host.workspace.has_changed(path).await? {
                changed = true;
                break;
            }
        }
        changed
    };

    // If something changed, evict the stale artifact so the cache miss path
    // below rebuilds it cleanly.
    if any_changed {
        host.storage.evict(bundle.id).await?;
    } else if let Some(blob) = host.storage.get(bundle.id).await? {
        // Cache hit and nothing changed — return the cached artifact.
        return decode::<BundleArtifact>(&blob);
    }

    // Cache miss (or stale eviction): parse every source file.
    let mut sources = Vec::with_capacity(source_paths.len());
    for path in source_paths {
        let syntax: SyntaxResult = host.workspace.syntax(&path).await
            .map_err(SemanticError::Host)?;
        sources.push((path, syntax.chunk));
    }

    let artifact = BundleArtifact {
        bundle_name: bundle.name().to_owned(),
        sources,
    };

    // Persist the freshly built artifact.
    host.storage.put(bundle.id, encode(&artifact)?).await?;

    Ok(artifact)
}