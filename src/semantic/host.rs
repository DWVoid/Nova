//! Host environment traits.
//!
//! The semantic engine is deliberately decoupled from the concrete execution
//! environment (CLI tool, language server, build system, test harness, …).
//! Instead of calling the OS or a specific runtime directly, every external
//! capability is accessed through one of the traits defined here.
//!
//! # Traits
//!
//! | Trait | Responsibility |
//! |-------|---------------|
//! [`DependencyResolver`] | Resolve an external package name + version requirement → blob + concrete version |
//! [`Workspace`]          | Provide [`SyntaxResult`]s for source paths and raw blobs for binary paths |
//! [`TaskSpawner`]        | Spawn independent async tasks (abstracts the Tokio runtime) |
//! [`BlobStorage`]        | UUID-keyed persistent binary storage for incremental caching |
//!
//! # Design notes
//!
//! All traits are `async_trait` — the `#[async_trait]` attribute rewrites
//! async fn signatures into `Pin<Box<dyn Future>>` so that the traits are
//! object-safe and can be stored as `dyn Trait` inside the semantic engine.
//!
//! [`HostEnv`] bundles all four capabilities into a single struct of
//! `Arc<dyn …>` references so that the engine only needs to carry one value
//! around.

#![allow(unused)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::syntax::{SyntaxError, SyntaxResult};

// ── Shared primitive types ────────────────────────────────────────────────────

/// A raw binary blob (e.g. a compiled bundle, a cached artifact).
pub type Blob = Arc<Vec<u8>>;

/// A package identifier as it appears in a `use` declaration or dependency
/// manifest — an unresolved, opaque string at this stage.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PackageId(pub String);

/// A semver-style version requirement string (e.g. `"^1.2"`, `"=0.9.1"`).
/// Parsing and evaluation are deferred to the resolver implementation.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct VersionReq(pub String);

/// A concrete, resolved version string (e.g. `"1.3.2"`).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ConcreteVersion(pub String);

/// A file-system-like path for source files and binary artifacts.
/// Kept as an opaque `String` rather than `std::path::PathBuf` so that
/// virtual workspaces (e.g. in-memory test fixtures) can use arbitrary keys.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct SourcePath(pub String);

// ── Error type ────────────────────────────────────────────────────────────────

/// All errors that can be returned by host environment operations.
#[derive(Debug)]
pub enum HostError {
    /// The dependency could not be resolved (not found, version conflict, …).
    DependencyNotFound { package: PackageId, req: VersionReq, reason: String },
    /// A source file could not be read or parsed.
    SourceError { path: SourcePath, error: SyntaxError },
    /// A binary blob could not be read.
    BlobReadError { path: SourcePath, reason: String },
    /// The incremental store failed to read or write a blob.
    StorageError { key: Uuid, reason: String },
    /// Task spawning failed.
    SpawnError(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::DependencyNotFound { package, req, reason } =>
                write!(f, "dependency '{}' @ '{}' not found: {}", package.0, req.0, reason),
            HostError::SourceError { path, error } =>
                write!(f, "source error in '{}': {}", path.0, error.message),
            HostError::BlobReadError { path, reason } =>
                write!(f, "blob read error for '{}': {}", path.0, reason),
            HostError::StorageError { key, reason } =>
                write!(f, "storage error for key {}: {}", key, reason),
            HostError::SpawnError(msg) =>
                write!(f, "task spawn error: {}", msg),
        }
    }
}

impl std::error::Error for HostError {}

// ── DependencyResolver ────────────────────────────────────────────────────────

/// Resolves external package identifiers to concrete blobs.
///
/// Implementations might talk to a package registry, a local cache, or a
/// virtual in-memory store (useful for tests).
///
/// # Contract
/// - Given a [`PackageId`] and a [`VersionReq`], resolve to the best
///   matching [`ConcreteVersion`] and return the bundle blob at that version.
/// - The operation is async because most real resolvers involve I/O
///   (network, disk).
/// - Implementations must be `Send + Sync` so they can be shared across tasks.
#[async_trait]
pub trait DependencyResolver: Send + Sync {
    async fn resolve(
        &self,
        package: &PackageId,
        req: &VersionReq,
    ) -> Result<(Blob, ConcreteVersion), HostError>;
}

// ── Workspace ─────────────────────────────────────────────────────────────────

/// Provides the semantic engine with access to source files and binary
/// artifacts that make up the project being compiled.
///
/// # Source files
/// [`Workspace::syntax`] reads a `.nova` source file at the given
/// [`SourcePath`] and returns a fully-parsed [`SyntaxResult`].  Caching
/// (re-using a previously parsed result when the file has not changed) is the
/// responsibility of the implementation; the engine will call this method
/// freely without worrying about redundant parses.
///
/// # Binary blobs
/// [`Workspace::blob`] reads an opaque binary file (pre-compiled bundle,
/// resource file, …) and returns its raw bytes.  This is separate from
/// dependency resolution — blobs accessed here are already known to the
/// workspace (e.g. referenced by a local manifest path).
///
/// # Change detection
/// [`Workspace::has_changed`] lets the semantic engine ask whether a file
/// has been modified since the last time it was processed.  The workspace
/// implementation decides what "changed" means — a content hash comparison,
/// a modification timestamp, an LSP dirty-buffer flag, etc.  When a file
/// has not changed, the engine can skip re-parsing and re-analysis and
/// reuse the cached artifact from [`BlobStorage`] directly.
#[async_trait]
pub trait Workspace: Send + Sync {
    /// Parse and return the Nova source file at `path`.
    async fn syntax(&self, path: &SourcePath) -> Result<SyntaxResult, HostError>;

    /// Read and return the raw binary blob at `path`.
    async fn blob(&self, path: &SourcePath) -> Result<Blob, HostError>;

    /// List all Nova source file paths known to this workspace.
    /// Used to discover the full compilation unit set when no explicit list
    /// is provided.
    async fn source_paths(&self) -> Result<Vec<SourcePath>, HostError>;

    /// Return `true` if the file at `path` has changed since the last time
    /// the semantic engine processed it.
    ///
    /// The engine calls this before attempting to load a cached artifact:
    /// - `false` → the cached artifact (if present) is still valid and can
    ///   be returned directly.
    /// - `true`  → the cache entry must be evicted and the file re-processed.
    ///
    /// Implementations are free to use any change-detection strategy:
    /// content hashing, mtime, LSP buffer versioning, etc.  When in doubt,
    /// returning `true` is always safe — it forces a recompute but never
    /// produces stale results.
    ///
    /// # Errors
    /// Returns [`HostError::BlobReadError`] if the path cannot be stat-ed or
    /// otherwise inspected.
    async fn has_changed(&self, path: &SourcePath) -> Result<bool, HostError>;
}

// ── TaskSpawner ───────────────────────────────────────────────────────────────

/// Abstracts the Tokio (or any other) async runtime's task-spawning primitive.
///
/// The semantic engine uses this trait instead of calling `tokio::spawn`
/// directly so that:
/// - The engine can be tested with a synchronous stub spawner.
/// - Alternative runtimes (Rayon, a custom thread pool, …) can be plugged in
///   without touching engine code.
///
/// # Task handle
/// Spawning returns a [`JoinHandle`] whose output type is `()`.  Passes that
/// need to return values from spawned tasks should communicate via shared
/// state (e.g. `Arc<Mutex<_>>`, channels) rather than relying on the join
/// result.
pub trait TaskSpawner: Send + Sync {
    /// Spawn `task` as an independent concurrent unit of work.
    /// Returns a [`tokio::task::JoinHandle`] that can be awaited or ignored.
    fn spawn(
        &self,
        task: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    ) -> tokio::task::JoinHandle<()>;
}

// ── BlobStorage ───────────────────────────────────────────────────────────────

/// UUID-keyed binary storage for incremental compilation artefacts.
///
/// The semantic engine serialises intermediate results (resolved scopes,
/// inferred types, …) to blobs and stores them here under a deterministic
/// UUID derived from the content hash of the input.  On a subsequent
/// compilation, it checks whether a blob exists for the same UUID before
/// recomputing.
///
/// Implementations may be backed by:
/// - An on-disk directory (production CLI tool)
/// - An in-memory `HashMap<Uuid, Vec<u8>>` (tests, LSP server)
/// - A shared network cache (distributed CI)
#[async_trait]
pub trait BlobStorage: Send + Sync {
    /// Retrieve the blob stored under `key`, or `None` if absent.
    async fn get(&self, key: Uuid) -> Result<Option<Blob>, HostError>;

    /// Store `data` under `key`, overwriting any previous value.
    async fn put(&self, key: Uuid, data: Blob) -> Result<(), HostError>;

    /// Remove the blob stored under `key`. No-op if absent.
    async fn evict(&self, key: Uuid) -> Result<(), HostError>;
}

// ── HostEnv ───────────────────────────────────────────────────────────────────

/// The complete host environment, bundling all four capability traits into a
/// single value that the semantic engine carries.
///
/// All four fields are `Arc<dyn …>` so that `HostEnv` itself is cheap to
/// clone and can be shared freely across async tasks.
///
/// # Construction
/// ```ignore
/// let env = HostEnv {
///     resolver: Arc::new(MyResolver::new()),
///     workspace: Arc::new(MyWorkspace::open(".")),
///     spawner:   Arc::new(TokioSpawner),
///     storage:   Arc::new(DiskStorage::open(".nova-cache")),
/// };
/// ```
#[derive(Clone)]
pub struct HostEnv {
    /// Resolves external package identifiers to blobs + versions.
    pub resolver: Arc<dyn DependencyResolver>,
    /// Provides source files and binary blobs for the current project.
    pub workspace: Arc<dyn Workspace>,
    /// Spawns independent async tasks.
    pub spawner: Arc<dyn TaskSpawner>,
    /// Stores and retrieves incremental compilation artefacts.
    pub storage: Arc<dyn BlobStorage>,
}

// ── TokioSpawner (default implementation) ────────────────────────────────────

/// The default [`TaskSpawner`] implementation: delegates directly to
/// [`tokio::spawn`].
///
/// This is the implementation used in production.  Tests and alternative
/// runtimes supply their own.
pub struct TokioSpawner;

impl TaskSpawner for TokioSpawner {
    fn spawn(
        &self,
        task: Pin<Box<dyn Future<Output = ()> + Send + 'static>>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(task)
    }
}
