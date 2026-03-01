//! [`SemanticSession`] – entry point for the semantic pipeline.
//!
//! ## Design: Three Shared Transforms, N File Nodes
//!
//! Transforms are registered once (`"load"`, `"lex"`, `"parse"`).  Adding a
//! file only allocates four nodes and three edges; no new transforms are
//! registered.  Graph metadata stays O(files) in nodes, O(1) in transform
//! keys.

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use nova_incremental::{IncrementalEngine, EngineError, UpdateReport, NodeId};
use nova_incremental::storage::Storage;

use crate::semantic::file_access::FileAccess;
use crate::semantic::file_content::FileContent;
use crate::semantic::file_stat::FileStat;
use crate::semantic::load_transform::{
    LexOutput, LOAD_KEY, LEX_KEY, PARSE_KEY,
    make_load_fn, make_lex_fn, make_parse_fn,
};
use crate::syntax::SyntaxResult;

// ---------------------------------------------------------------------------
// UUID namespace for stable file node IDs – must never change.
// ---------------------------------------------------------------------------
const FILE_NODE_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1,
    0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

pub fn stat_node_id(path: &str)    -> NodeId { NodeId::named(FILE_NODE_NS, path) }
pub fn content_node_id(path: &str) -> NodeId { NodeId::named(FILE_NODE_NS, &format!("{path}\x00content")) }
pub fn lex_node_id(path: &str)     -> NodeId { NodeId::named(FILE_NODE_NS, &format!("{path}\x00lex")) }
pub fn parse_node_id(path: &str)   -> NodeId { NodeId::named(FILE_NODE_NS, &format!("{path}\x00parse")) }

// ---------------------------------------------------------------------------
// Per-file bookkeeping
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct FileNodes {
    pub stat_node:    NodeId,
    pub content_node: NodeId,
    pub lex_node:     NodeId,
    pub parse_node:   NodeId,
    /// Last-seen stat used for change detection (avoids calling into the
    /// engine when the stat is unchanged, preserving the hash-early-exit
    /// optimisation).
    pub last_stat: FileStat,
}

// ---------------------------------------------------------------------------
// SemanticSession
// ---------------------------------------------------------------------------

pub struct SemanticSession {
    pub(crate) engine: IncrementalEngine,
    files: HashMap<String, FileNodes>,
    fs: Arc<dyn FileAccess>,
}

impl SemanticSession {
    /// Register value types and the three shared transforms into `engine`.
    fn setup(engine: &mut IncrementalEngine, fs: &Arc<dyn FileAccess>) -> Result<(), EngineError> {
        // Value types
        engine.register_value_type::<FileStat>("FileStat")?;
        engine.register_value_type::<FileContent>("FileContent")?;
        engine.register_value_type::<LexOutput>("LexOutput")?;
        engine.register_value_type::<SyntaxResult>("SyntaxResult")?;

        // Shared stateless transforms – registered once, shared by all files.
        engine.register_one_to_one::<FileStat, FileContent, _, _>(
            LOAD_KEY, make_load_fn(Arc::clone(fs)))?;
        engine.register_one_to_one::<FileContent, LexOutput, _, _>(
            LEX_KEY, make_lex_fn())?;
        engine.register_one_to_one::<LexOutput, SyntaxResult, _, _>(
            PARSE_KEY, make_parse_fn())?;

        Ok(())
    }

    /// Create a new session backed by `storage` and reading files via `fs`.
    pub fn new(storage: Arc<dyn Storage>, fs: Arc<dyn FileAccess>) -> Self {
        let mut engine = IncrementalEngine::new(storage);
        Self::setup(&mut engine, &fs)
            .expect("session setup must not fail");
        Self { engine, files: HashMap::new(), fs }
    }

    // -----------------------------------------------------------------------
    // File set management
    // -----------------------------------------------------------------------

    /// Apply a new snapshot of the file set.
    pub fn update_files(&mut self, stats: Vec<FileStat>) -> Result<(), EngineError> {
        let new_paths: HashMap<String, FileStat> = stats
            .into_iter()
            .map(|s| (s.path.clone(), s))
            .collect();

        for (path, stat) in &new_paths {
            if let Some(nodes) = self.files.get_mut(path) {
                if nodes.last_stat != *stat {
                    nodes.last_stat = stat.clone();
                    let stat_node = nodes.stat_node;
                    self.engine.set_input(stat_node, stat.clone())?;
                }
            } else {
                self.add_file(path, stat.clone())?;
            }
        }

        let removed: Vec<String> = self.files.keys()
            .filter(|p| !new_paths.contains_key(*p))
            .cloned()
            .collect();
        for path in removed {
            self.remove_file(&path);
        }
        Ok(())
    }

    /// Wire up four nodes for a newly discovered file.
    fn add_file(&mut self, path: &str, stat: FileStat) -> Result<(), EngineError> {
        let stat_node    = stat_node_id(path);
        let content_node = content_node_id(path);
        let lex_node     = lex_node_id(path);
        let parse_node   = parse_node_id(path);

        // Nodes with stable pre-derived IDs.
        self.engine.graph().add_input_node_with_id(stat_node);
        let last_stat = stat.clone();
        self.engine.set_input(stat_node, stat)?;
        self.engine.graph().add_computed_node_with_id(content_node);
        self.engine.graph().add_computed_node_with_id(lex_node);
        self.engine.graph().add_computed_node_with_id(parse_node);

        // Edges use the shared transform keys.
        self.engine.connect(&[stat_node],    &[content_node], LOAD_KEY)?;
        self.engine.connect(&[content_node], &[lex_node],     LEX_KEY)?;
        self.engine.connect(&[lex_node],     &[parse_node],   PARSE_KEY)?;

        self.files.insert(path.to_string(), FileNodes {
            stat_node, content_node, lex_node, parse_node, last_stat,
        });
        Ok(())
    }

    fn remove_file(&mut self, path: &str) {
        if let Some(nodes) = self.files.remove(path) {
            self.engine.remove_node(nodes.stat_node);
            self.engine.remove_node(nodes.content_node);
            self.engine.remove_node(nodes.lex_node);
            self.engine.remove_node(nodes.parse_node);
        }
    }

    // -----------------------------------------------------------------------
    // Incremental update
    // -----------------------------------------------------------------------

    pub async fn run(&self) -> UpdateReport {
        self.engine.update().await
    }

    // -----------------------------------------------------------------------
    // Value access (typed)
    // -----------------------------------------------------------------------

    pub async fn get_content(&self, path: &str) -> Result<Option<FileContent>, EngineError> {
        let nodes = match self.files.get(path) { Some(n) => n, None => return Ok(None) };
        self.engine.get_value::<FileContent>(nodes.content_node).await
    }

    /// Return the `LexicalResult` for `path` (unwrapped from `LexOutput`).
    pub async fn get_lex_result(
        &self, path: &str,
    ) -> Result<Option<crate::lexical::LexicalResult>, EngineError> {
        let nodes = match self.files.get(path) { Some(n) => n, None => return Ok(None) };
        Ok(self.engine.get_value::<LexOutput>(nodes.lex_node).await?.map(|lo| lo.lex))
    }

    pub async fn get_syntax_result(&self, path: &str) -> Result<Option<SyntaxResult>, EngineError> {
        let nodes = match self.files.get(path) { Some(n) => n, None => return Ok(None) };
        self.engine.get_value::<SyntaxResult>(nodes.parse_node).await
    }

    pub fn file_nodes(&self, path: &str) -> Option<&FileNodes> { self.files.get(path) }

    pub fn tracked_paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    pub async fn save(&self) -> Result<(), EngineError> {
        self.engine.save().await
    }

    /// Restore a session from storage.
    ///
    /// `known_paths` must list every file that was tracked when `save()` was
    /// called so nodes and the `files` map can be reconstructed.  No per-file
    /// transforms need to be re-registered; the three shared transforms are
    /// enough.
    pub async fn load(
        storage: Arc<dyn Storage>,
        fs: Arc<dyn FileAccess>,
        known_paths: &[&str],
    ) -> Result<Self, EngineError> {
        let mut engine = IncrementalEngine::new(Arc::clone(&storage));
        Self::setup(&mut engine, &fs)?;

        let engine = IncrementalEngine::load(Arc::clone(&storage), engine).await?;

        let mut files = HashMap::new();
        for path in known_paths {
            let stat_node = stat_node_id(path);
            if engine.graph().contains_node(stat_node) {
                files.insert(path.to_string(), FileNodes {
                    stat_node,
                    content_node: content_node_id(path),
                    lex_node:     lex_node_id(path),
                    parse_node:   parse_node_id(path),
                    last_stat: FileStat::new(*path, u64::MAX, u64::MAX),
                });
            }
        }

        Ok(Self { engine, files, fs })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use nova_incremental::storage::MemoryStorage;
    use crate::semantic::file_access::MockFileAccess;

    fn make_fs(files: &[(&str, &str)]) -> Arc<dyn FileAccess> {
        let mut mock = MockFileAccess::new();
        for (path, content) in files { mock.add(*path, content.as_bytes().to_vec()); }
        Arc::new(mock)
    }

    fn make_session(files: &[(&str, &str)]) -> SemanticSession {
        SemanticSession::new(Arc::new(MemoryStorage::new()), make_fs(files))
    }

    // -----------------------------------------------------------------------
    // Node ID helpers
    // -----------------------------------------------------------------------
    #[test] fn node_ids_stable() {
        assert_eq!(stat_node_id("src/main.nova"), stat_node_id("src/main.nova"));
    }
    #[test] fn stat_and_content_ids_differ() {
        assert_ne!(stat_node_id("a.nova"), content_node_id("a.nova"));
    }
    #[test] fn all_four_ids_distinct() {
        let p = "src/main.nova";
        let ids = [stat_node_id(p), content_node_id(p), lex_node_id(p), parse_node_id(p)];
        for i in 0..ids.len() {
            for j in (i+1)..ids.len() { assert_ne!(ids[i], ids[j]); }
        }
    }

    // -----------------------------------------------------------------------
    // Basic load
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn single_file_is_loaded() {
        let src = "namespace main;";
        let mut s = make_session(&[("main.nova", src)]);
        s.update_files(vec![FileStat::new("main.nova", src.len() as u64, 0)]).unwrap();
        assert!(s.run().await.is_ok());
        assert_eq!(s.get_content("main.nova").await.unwrap().unwrap().as_str().unwrap(), src);
    }

    #[tokio::test]
    async fn multiple_files_loaded_in_parallel() {
        let mut s = make_session(&[
            ("a.nova", "namespace a;"), ("b.nova", "namespace b;"), ("c.nova", "namespace c;"),
        ]);
        s.update_files(vec![
            FileStat::new("a.nova", 12, 0), FileStat::new("b.nova", 12, 0), FileStat::new("c.nova", 12, 0),
        ]).unwrap();
        assert!(s.run().await.is_ok());
        assert_eq!(s.get_content("a.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace a;");
        assert_eq!(s.get_content("b.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace b;");
        assert_eq!(s.get_content("c.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace c;");
    }

    // -----------------------------------------------------------------------
    // Incremental
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn unchanged_stat_skips_reload() {
        let src = "namespace original;";
        let mut s = make_session(&[("x.nova", src)]);
        let stat = FileStat::new("x.nova", src.len() as u64, 500);
        s.update_files(vec![stat.clone()]).unwrap();
        s.run().await;
        s.update_files(vec![stat]).unwrap();
        let report = s.run().await;
        assert_eq!(report.nodes_evaluated, 0, "unchanged stat must not trigger reload");
    }

    #[tokio::test]
    async fn changed_mtime_triggers_reload() {
        let mut mock = MockFileAccess::new();
        mock.add("f.nova", b"namespace v1;".to_vec());
        let mut s = SemanticSession::new(Arc::new(MemoryStorage::new()), Arc::new(mock));
        s.update_files(vec![FileStat::new("f.nova", 13, 1000)]).unwrap();
        s.run().await;
        assert_eq!(s.get_content("f.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace v1;");

        let mut mock2 = MockFileAccess::new();
        mock2.add("f.nova", b"namespace v2;".to_vec());
        let mut s2 = SemanticSession::new(Arc::new(MemoryStorage::new()), Arc::new(mock2));
        s2.update_files(vec![FileStat::new("f.nova", 13, 2000)]).unwrap();
        let report = s2.run().await;
        assert_eq!(report.nodes_evaluated, 3, "content + lex + parse must all run");
        assert_eq!(s2.get_content("f.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace v2;");
    }

    // -----------------------------------------------------------------------
    // Adding / removing files
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn adding_file_mid_session() {
        let mut s = make_session(&[
            ("existing.nova", "namespace existing;"), ("new.nova", "namespace new_mod;"),
        ]);
        s.update_files(vec![FileStat::new("existing.nova", 19, 0)]).unwrap();
        s.run().await;
        s.update_files(vec![
            FileStat::new("existing.nova", 19, 0), FileStat::new("new.nova", 17, 1),
        ]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        assert_eq!(s.get_content("new.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace new_mod;");
    }

    #[tokio::test]
    async fn removing_file_mid_session() {
        let mut s = make_session(&[
            ("keep.nova", "namespace keep;"), ("remove.nova", "namespace remove_me;"),
        ]);
        s.update_files(vec![
            FileStat::new("keep.nova", 15, 0), FileStat::new("remove.nova", 20, 0),
        ]).unwrap();
        s.run().await;
        s.update_files(vec![FileStat::new("keep.nova", 15, 0)]).unwrap();
        s.run().await;
        assert!(s.file_nodes("remove.nova").is_none());
        assert!(s.get_content("remove.nova").await.unwrap().is_none());
        assert_eq!(s.get_content("keep.nova").await.unwrap().unwrap().as_str().unwrap(), "namespace keep;");
    }

    // -----------------------------------------------------------------------
    // tracked_paths
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn tracked_paths_reflect_current_set() {
        let mut s = make_session(&[("a.nova", ""), ("b.nova", "")]);
        s.update_files(vec![FileStat::new("a.nova", 0, 0), FileStat::new("b.nova", 0, 0)]).unwrap();
        let mut paths: Vec<_> = s.tracked_paths().collect();
        paths.sort();
        assert_eq!(paths, vec!["a.nova", "b.nova"]);
    }

    // -----------------------------------------------------------------------
    // Missing file
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn missing_file_produces_error_in_report() {
        let mut s = SemanticSession::new(
            Arc::new(MemoryStorage::new()), Arc::new(MockFileAccess::new()));
        s.update_files(vec![FileStat::new("ghost.nova", 0, 0)]).unwrap();
        let report = s.run().await;
        assert!(!report.is_ok());
        assert_eq!(report.errors.len(), 1);
    }

    // -----------------------------------------------------------------------
    // Save and reload
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn save_and_reload_preserves_node_ids() {
        let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
        let fs = make_fs(&[("lib.nova", "namespace lib;")]);

        let mut s = SemanticSession::new(Arc::clone(&storage), Arc::clone(&fs));
        s.update_files(vec![FileStat::new("lib.nova", 14, 42)]).unwrap();
        s.run().await;
        s.save().await.unwrap();

        let s2 = SemanticSession::load(Arc::clone(&storage), Arc::clone(&fs), &["lib.nova"])
            .await.unwrap();
        assert!(s2.engine.graph().contains_node(stat_node_id("lib.nova")));
        assert!(s2.engine.graph().contains_node(content_node_id("lib.nova")));
    }

    // -----------------------------------------------------------------------
    // Lex result
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn lex_result_is_available_after_run() {
        let src = "namespace test;";
        let mut s = make_session(&[("t.nova", src)]);
        s.update_files(vec![FileStat::new("t.nova", src.len() as u64, 0)]).unwrap();
        s.run().await;
        let lex = s.get_lex_result("t.nova").await.unwrap().unwrap();
        assert!(lex.tokens.len() >= 3);
    }

    #[tokio::test]
    async fn lex_result_for_unknown_path_is_none() {
        assert!(make_session(&[]).get_lex_result("no-such.nova").await.unwrap().is_none());
    }

    // -----------------------------------------------------------------------
    // Syntax result
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn syntax_result_is_available_after_run() {
        let src = "namespace my_module;";
        let mut s = make_session(&[("m.nova", src)]);
        s.update_files(vec![FileStat::new("m.nova", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let sr = s.get_syntax_result("m.nova").await.unwrap().unwrap();
        assert_eq!(sr.chunk.namespace.path[0].value, "my_module");
    }

    #[tokio::test]
    async fn syntax_result_for_unknown_path_is_none() {
        assert!(make_session(&[]).get_syntax_result("no-such.nova").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn lex_error_blocks_parse_stage() {
        let mut mock = MockFileAccess::new();
        mock.add("bad.nova", vec![0x00]);
        let mut s = SemanticSession::new(Arc::new(MemoryStorage::new()), Arc::new(mock));
        s.update_files(vec![FileStat::new("bad.nova", 1, 0)]).unwrap();
        let report = s.run().await;
        assert_eq!(report.errors.len(), 1,
            "only lex should error, parse should be blocked: {:?}", report.errors);
        assert!(s.get_syntax_result("bad.nova").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn syntax_error_is_isolated_to_one_file() {
        let bad_src  = "val x = 1;";
        let good_src = "namespace ok;";
        let mut s = make_session(&[("bad.nova", bad_src), ("good.nova", good_src)]);
        s.update_files(vec![
            FileStat::new("bad.nova",  bad_src.len()  as u64, 0),
            FileStat::new("good.nova", good_src.len() as u64, 0),
        ]).unwrap();
        let report = s.run().await;
        let sr = s.get_syntax_result("good.nova").await.unwrap().unwrap();
        assert_eq!(sr.chunk.namespace.path[0].value, "ok");
        assert!(report.errors.iter().any(|(_, e)| e.message.contains("bad.nova")),
            "expected a parse error mentioning bad.nova: {:?}", report.errors);
    }

    #[tokio::test]
    async fn changing_file_reruns_full_pipeline() {
        let mut mock = MockFileAccess::new();
        mock.add("f.nova", b"namespace v1;".to_vec());
        let mut s = SemanticSession::new(Arc::new(MemoryStorage::new()), Arc::new(mock));
        s.update_files(vec![FileStat::new("f.nova", 13, 100)]).unwrap();
        s.run().await;
        assert_eq!(s.get_syntax_result("f.nova").await.unwrap().unwrap().chunk.namespace.path[0].value, "v1");

        let mut mock2 = MockFileAccess::new();
        mock2.add("f.nova", b"namespace v2;".to_vec());
        let mut s2 = SemanticSession::new(Arc::new(MemoryStorage::new()), Arc::new(mock2));
        s2.update_files(vec![FileStat::new("f.nova", 13, 200)]).unwrap();
        let report = s2.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        assert_eq!(report.nodes_evaluated, 3, "all three stages must run");
        assert_eq!(s2.get_syntax_result("f.nova").await.unwrap().unwrap().chunk.namespace.path[0].value, "v2");
    }
}
