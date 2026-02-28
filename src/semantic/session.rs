//! [`SemanticSession`] – the entry point for the semantic pipeline.
//!
//! ## Design: Session as Incremental Graph Coordinator
//!
//! A `SemanticSession` owns one [`IncrementalEngine`] and maintains a
//! registry of file nodes indexed by path.  It exposes a simple API:
//!
//! - [`update_files`](SemanticSession::update_files) – supply the current
//!   set of [`FileStat`] values (from a directory scan or IDE notification).
//!   New files are added, removed files are dropped, changed stats update
//!   the input values.
//! - [`run`](SemanticSession::run) – propagate all dirty nodes through the
//!   incremental graph and return an [`UpdateReport`].
//! - [`get_content`](SemanticSession::get_content) – retrieve the loaded
//!   [`FileContent`] for a file by path.
//!
//! ## Design: UUID v5 Node Identity
//!
//! Each file's stat node ID is derived via `NodeId::named(FILE_NS, path)`.
//! This means:
//! - The same file always maps to the same `NodeId` across restarts (enabling
//!   hash-based early exit on the first run after reload).
//! - No separate path→ID map needs to be persisted; it is always
//!   recomputable from the path.
//!
//! ## Design: Registry Keys for Load Transforms
//!
//! The load transform for each file is registered under `"load:<path>"`.
//! When persisting the graph topology, these keys are stored in edge entries.
//! On reload, `SemanticSession::load` re-registers the same keys by
//! iterating over the stored paths.
//!
//! ## Design: `Arc<dyn Storage>` Passed In
//!
//! The user supplies the storage backend.  This keeps the session testable
//! with `MemoryStorage` and production-ready with any async key-value store.

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::incremental::{
    IncrementalEngine, EngineError, TransformRegistry, UpdateReport,
    NodeId,
    storage::Storage,
    transform::Transform,
    value::Value,
};
use crate::semantic::file_access::FileAccess;
use crate::semantic::file_content::FileContent;
use crate::semantic::file_stat::FileStat;
use crate::semantic::load_transform::LoadFile;

// ---------------------------------------------------------------------------
// Namespace UUID for file stat node IDs (UUID v5)
//
// This UUID is a fixed constant unique to the Nova semantic pipeline.
// It must never change; doing so would invalidate all persisted node IDs.
// ---------------------------------------------------------------------------
const FILE_NODE_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1,
    0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

/// Derive a stable [`NodeId`] for the stat input node of `path`.
pub fn stat_node_id(path: &str) -> NodeId {
    NodeId::named(FILE_NODE_NS, path)
}

/// Derive a stable [`NodeId`] for the content output node of `path`.
///
/// Uses a different sub-namespace so stat and content nodes for the same
/// file never collide.
pub fn content_node_id(path: &str) -> NodeId {
    // Append a distinguishing suffix before hashing.
    NodeId::named(FILE_NODE_NS, &format!("{path}\x00content"))
}

// ---------------------------------------------------------------------------
// Per-file bookkeeping
// ---------------------------------------------------------------------------

/// All node IDs associated with a single source file.
#[derive(Debug, Clone)]
pub struct FileNodes {
    /// Input node carrying the [`FileStat`].
    pub stat_node: NodeId,
    /// Computed output node carrying the [`FileContent`].
    pub content_node: NodeId,
}

// ---------------------------------------------------------------------------
// SemanticSession
// ---------------------------------------------------------------------------

/// Coordinator for the file-loading stage of the semantic pipeline.
///
/// Create one with [`SemanticSession::new`], call
/// [`update_files`](Self::update_files) to register the current file set,
/// then call [`run`](Self::run) to execute the incremental update.
pub struct SemanticSession {
    engine: IncrementalEngine,
    /// Maps canonical path → per-file node IDs.
    files: HashMap<String, FileNodes>,
    /// Shared VFS used by all load transforms.
    fs: Arc<dyn FileAccess>,
}

impl SemanticSession {
    /// Create a new session backed by `storage` and reading files through `fs`.
    ///
    /// The `storage` is passed directly to the [`IncrementalEngine`]; persist
    /// calls write topology and hashes through it.
    pub fn new(storage: Arc<dyn Storage>, fs: Arc<dyn FileAccess>) -> Self {
        let engine = IncrementalEngine::new(storage);
        Self {
            engine,
            files: HashMap::new(),
            fs,
        }
    }

    // -----------------------------------------------------------------------
    // File set management
    // -----------------------------------------------------------------------

    /// Apply a new snapshot of the file set.
    ///
    /// This method computes the diff between the supplied `stats` and the
    /// currently tracked set, then:
    ///
    /// 1. **New files** – adds a stat input node and a content output node,
    ///    registers the `LoadFile` transform, and connects them.
    /// 2. **Changed files** – updates the stat input node's value; the
    ///    content node is automatically marked dirty by the graph.
    /// 3. **Removed files** – removes the stat and content nodes from the
    ///    graph (their edges are cleaned up by `Graph::remove_node`).
    ///
    /// After this call, [`run`](Self::run) will recompute only the content
    /// nodes whose stat actually changed.
    pub fn update_files(&mut self, stats: Vec<FileStat>) -> Result<(), EngineError> {
        let new_paths: HashMap<String, FileStat> = stats
            .into_iter()
            .map(|s| (s.path.clone(), s))
            .collect();

        // --- Add or update files ---
        for (path, stat) in &new_paths {
            if let Some(nodes) = self.files.get(path) {
                // Existing file: only update the stat input if the value
                // has actually changed.  Calling set_input always marks the
                // node dirty even when the value is identical, which would
                // defeat the hash-based early-exit optimisation.
                let changed = self.engine
                    .graph()
                    .peek_value(nodes.stat_node)
                    .and_then(|(v, _)| v.downcast::<FileStat>().cloned())
                    .map(|current| current != *stat)
                    .unwrap_or(true); // treat missing value as changed

                if changed {
                    self.engine.set_input(nodes.stat_node, Value::new(stat.clone()))?;
                }
            } else {
                // New file: wire up stat → load → content.
                self.add_file(path, stat.clone())?;
            }
        }

        // --- Remove files no longer present ---
        let removed: Vec<String> = self.files.keys()
            .filter(|p| !new_paths.contains_key(*p))
            .cloned()
            .collect();

        for path in removed {
            self.remove_file(&path);
        }

        Ok(())
    }

    /// Add a single new file to the graph.
    fn add_file(&mut self, path: &str, stat: FileStat) -> Result<(), EngineError> {
        // Derive stable node IDs from the path.
        let stat_node    = stat_node_id(path);
        let content_node = content_node_id(path);

        // Register the load transform under its per-file key.
        let key = LoadFile::registry_key(path);
        let transform = Transform::OneToOne(Arc::new(
            LoadFile::new(path, Arc::clone(&self.fs))
        ));

        // We need mutable access to the engine only for transform registration;
        // all other operations take &self.
        // SAFETY: register_transform_dynamic is &mut self, so we call it here
        // before any graph mutations.
        self.engine.register_transform(key.clone(), transform);

        // Add the stat input node with a pre-assigned ID.
        self.engine.graph().add_input_node_with_id(stat_node);
        self.engine.graph().set_input(stat_node, Value::new(stat))
            .map_err(EngineError::Graph)?;

        // Add the content computed node.
        self.engine.graph().add_computed_node_with_id(content_node);

        // Connect: stat_node → LoadFile → content_node.
        self.engine.connect_by_key(
            &[stat_node],
            &[content_node],
            &key,
        )?;

        self.files.insert(path.to_string(), FileNodes { stat_node, content_node });
        Ok(())
    }

    /// Remove a file from the graph and clean up all associated nodes.
    fn remove_file(&mut self, path: &str) {
        if let Some(nodes) = self.files.remove(path) {
            // Removing the stat node also removes the edge to content_node,
            // which marks content_node dirty.  We then remove content_node too.
            self.engine.remove_node(nodes.stat_node);
            self.engine.remove_node(nodes.content_node);
        }
    }

    // -----------------------------------------------------------------------
    // Incremental update
    // -----------------------------------------------------------------------

    /// Run the incremental update cycle.
    ///
    /// Only content nodes whose stat changed (or are newly added) are
    /// recomputed.  Returns an [`UpdateReport`] with counts and any errors.
    pub async fn run(&self) -> UpdateReport {
        self.engine.update().await
    }

    // -----------------------------------------------------------------------
    // Value access
    // -----------------------------------------------------------------------

    /// Retrieve the loaded content for `path`.
    ///
    /// Returns `Ok(None)` if the file has not been loaded yet or its content
    /// was evicted from the cache.  Returns `Err` on storage failure.
    pub async fn get_content(&self, path: &str) -> Result<Option<FileContent>, EngineError> {
        let nodes = match self.files.get(path) {
            Some(n) => n,
            None    => return Ok(None),
        };
        match self.engine.get_value(nodes.content_node).await? {
            Some(v) => Ok(v.downcast::<FileContent>().cloned()),
            None    => Ok(None),
        }
    }

    /// Return the [`FileNodes`] for `path`, if tracked.
    pub fn file_nodes(&self, path: &str) -> Option<&FileNodes> {
        self.files.get(path)
    }

    /// Return the set of all currently tracked file paths.
    pub fn tracked_paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Persist the current graph topology and all computed hashes to storage.
    ///
    /// After calling this, a new session created with
    /// [`SemanticSession::load`] will start with clean hashes and skip
    /// unchanged files on the first `run()`.
    pub async fn save(&self) -> Result<(), EngineError> {
        self.engine.save().await
    }

    /// Restore a session from storage.
    ///
    /// The caller must supply the same `fs` and the same file set (via
    /// `update_files`) after loading so the graph is ready for the next
    /// `run()`.
    pub async fn load(
        storage: Arc<dyn Storage>,
        fs: Arc<dyn FileAccess>,
        known_paths: &[&str],
    ) -> Result<Self, EngineError> {
        // Re-build the registry for all known paths.
        let mut registry = TransformRegistry::new();
        for path in known_paths {
            let key = LoadFile::registry_key(path);
            let transform = Transform::OneToOne(Arc::new(
                LoadFile::new(*path, Arc::clone(&fs))
            ));
            registry.register(key, transform);
        }

        let engine = IncrementalEngine::load(Arc::clone(&storage), registry).await?;

        // Reconstruct the files map from the known paths.
        let mut files = HashMap::new();
        for path in known_paths {
            let stat_node    = stat_node_id(path);
            let content_node = content_node_id(path);
            if engine.graph().contains_node(stat_node) {
                files.insert(path.to_string(), FileNodes { stat_node, content_node });
            }
        }

        Ok(Self { engine, files, fs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::storage::MemoryStorage;
    use crate::semantic::file_access::MockFileAccess;

    fn make_fs(files: &[(&str, &str)]) -> Arc<dyn FileAccess> {
        let mut mock = MockFileAccess::new();
        for (path, content) in files {
            mock.add(*path, content.as_bytes().to_vec());
        }
        Arc::new(mock)
    }

    fn make_session(files: &[(&str, &str)]) -> SemanticSession {
        let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let fs      = make_fs(files);
        SemanticSession::new(storage, fs)
    }

    // -----------------------------------------------------------------------
    // stat_node_id / content_node_id
    // -----------------------------------------------------------------------

    #[test]
    fn node_ids_are_stable_for_same_path() {
        let a = stat_node_id("src/main.nova");
        let b = stat_node_id("src/main.nova");
        assert_eq!(a, b);
    }

    #[test]
    fn stat_and_content_ids_differ() {
        let s = stat_node_id("src/main.nova");
        let c = content_node_id("src/main.nova");
        assert_ne!(s, c);
    }

    #[test]
    fn node_ids_differ_for_different_paths() {
        assert_ne!(stat_node_id("a.nova"), stat_node_id("b.nova"));
        assert_ne!(content_node_id("a.nova"), content_node_id("b.nova"));
    }

    // -----------------------------------------------------------------------
    // Basic load
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn single_file_is_loaded() {
        let mut session = make_session(&[("main.nova", "val x = 1;")]);
        let stat = FileStat::new("main.nova", 10, 1000);
        session.update_files(vec![stat]).unwrap();
        let report = session.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);

        let content = session.get_content("main.nova").await.unwrap().unwrap();
        assert_eq!(content.as_str().unwrap(), "val x = 1;");
    }

    #[tokio::test]
    async fn multiple_files_loaded_in_parallel() {
        let mut session = make_session(&[
            ("a.nova", "-- a"),
            ("b.nova", "-- b"),
            ("c.nova", "-- c"),
        ]);
        let stats = vec![
            FileStat::new("a.nova", 4, 100),
            FileStat::new("b.nova", 4, 200),
            FileStat::new("c.nova", 4, 300),
        ];
        session.update_files(stats).unwrap();
        session.run().await;

        assert_eq!(session.get_content("a.nova").await.unwrap().unwrap().as_str().unwrap(), "-- a");
        assert_eq!(session.get_content("b.nova").await.unwrap().unwrap().as_str().unwrap(), "-- b");
        assert_eq!(session.get_content("c.nova").await.unwrap().unwrap().as_str().unwrap(), "-- c");
    }

    // -----------------------------------------------------------------------
    // Incremental: unchanged stat → content not reloaded
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn unchanged_stat_skips_reload() {
        let mut session = make_session(&[("x.nova", "-- original")]);
        let stat = FileStat::new("x.nova", 11, 500);
        session.update_files(vec![stat.clone()]).unwrap();
        session.run().await;

        // Second run with the same stat → nothing dirty.
        session.update_files(vec![stat]).unwrap();
        let report = session.run().await;
        assert_eq!(report.nodes_evaluated, 0,
            "unchanged stat must not trigger reload");
    }

    #[tokio::test]
    async fn changed_mtime_triggers_reload() {
        // The FS has updated content under the same path.
        let mut mock = MockFileAccess::new();
        mock.add("f.nova", b"-- v1".to_vec());
        let fs: Arc<dyn FileAccess> = Arc::new(mock);

        let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let mut session = SemanticSession::new(Arc::clone(&storage), Arc::clone(&fs));

        let stat_v1 = FileStat::new("f.nova", 5, 1000);
        session.update_files(vec![stat_v1]).unwrap();
        session.run().await;
        assert_eq!(
            session.get_content("f.nova").await.unwrap().unwrap().as_str().unwrap(),
            "-- v1"
        );

        // Simulate file content change on disk: update mock and change stat.
        // We must rebuild the session with updated mock content.
        let mut mock2 = MockFileAccess::new();
        mock2.add("f.nova", b"-- v2".to_vec());
        let fs2: Arc<dyn FileAccess> = Arc::new(mock2);
        let mut session2 = SemanticSession::new(Arc::new(MemoryStorage::new()), fs2);

        let stat_v2 = FileStat::new("f.nova", 5, 2000); // mtime changed
        session2.update_files(vec![stat_v2]).unwrap();
        let report = session2.run().await;
        assert_eq!(report.nodes_evaluated, 1);
        assert_eq!(
            session2.get_content("f.nova").await.unwrap().unwrap().as_str().unwrap(),
            "-- v2"
        );
    }

    // -----------------------------------------------------------------------
    // Adding a new file mid-session
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn adding_file_mid_session() {
        let mut session = make_session(&[
            ("existing.nova", "-- exists"),
            ("new.nova",      "-- new"),
        ]);

        // First run: only one file.
        session.update_files(vec![FileStat::new("existing.nova", 9, 0)]).unwrap();
        session.run().await;

        // Second run: add a new file.
        session.update_files(vec![
            FileStat::new("existing.nova", 9, 0), // unchanged
            FileStat::new("new.nova", 5, 1),      // new
        ]).unwrap();
        let report = session.run().await;

        assert!(report.is_ok(), "{:?}", report.errors);
        assert_eq!(
            session.get_content("new.nova").await.unwrap().unwrap().as_str().unwrap(),
            "-- new"
        );
    }

    // -----------------------------------------------------------------------
    // Removing a file mid-session
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn removing_file_mid_session() {
        let mut session = make_session(&[
            ("keep.nova",   "-- keep"),
            ("remove.nova", "-- remove"),
        ]);

        session.update_files(vec![
            FileStat::new("keep.nova",   4, 0),
            FileStat::new("remove.nova", 8, 0),
        ]).unwrap();
        session.run().await;

        // Second snapshot: remove.nova is gone.
        session.update_files(vec![FileStat::new("keep.nova", 4, 0)]).unwrap();
        session.run().await;

        assert!(session.file_nodes("remove.nova").is_none(),
            "removed file must not be tracked");
        assert!(session.get_content("remove.nova").await.unwrap().is_none(),
            "content for removed file must not be accessible");
        // keep.nova must still be accessible.
        let kept = session.get_content("keep.nova").await.unwrap().unwrap();
        assert_eq!(kept.as_str().unwrap(), "-- keep");
    }

    // -----------------------------------------------------------------------
    // tracked_paths
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn tracked_paths_reflect_current_set() {
        let mut session = make_session(&[("a.nova", ""), ("b.nova", "")]);
        session.update_files(vec![
            FileStat::new("a.nova", 0, 0),
            FileStat::new("b.nova", 0, 0),
        ]).unwrap();

        let mut paths: Vec<_> = session.tracked_paths().collect();
        paths.sort();
        assert_eq!(paths, vec!["a.nova", "b.nova"]);
    }

    // -----------------------------------------------------------------------
    // Missing file → error surfaced in report
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn missing_file_produces_error_in_report() {
        // VFS has no files at all.
        let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
        let fs      = Arc::new(MockFileAccess::new()) as Arc<dyn FileAccess>;
        let mut session = SemanticSession::new(storage, fs);

        session.update_files(vec![FileStat::new("ghost.nova", 0, 0)]).unwrap();
        let report = session.run().await;

        assert!(!report.is_ok(), "missing file should produce an error");
        assert_eq!(report.errors.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Save and reload
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn save_and_reload_preserves_node_ids() {
        let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
        let fs = make_fs(&[("lib.nova", "-- lib")]);

        let mut session = SemanticSession::new(Arc::clone(&storage), Arc::clone(&fs));
        session.update_files(vec![FileStat::new("lib.nova", 6, 42)]).unwrap();
        session.run().await;
        session.save().await.unwrap();

        let session2 = SemanticSession::load(
            Arc::clone(&storage), Arc::clone(&fs), &["lib.nova"]
        ).await.unwrap();

        // The stable node IDs must be present in the reloaded graph.
        let expected_stat    = stat_node_id("lib.nova");
        let expected_content = content_node_id("lib.nova");
        assert!(session2.engine.graph().contains_node(expected_stat));
        assert!(session2.engine.graph().contains_node(expected_content));
    }
}
