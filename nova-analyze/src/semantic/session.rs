//! [`SemanticSession`] – entry point for the semantic pipeline.
//!
//! ## Design: Static Topology + ProjectDescriptor fan-out
//!
//! The graph topology is **static** (declared once via `EngineBuilder`).
//! Variable file sets are handled by writing a new [`ProjectDescriptor`] to
//! the single input node.  The `expand` transform fans this out into a
//! `Collection<FileStat>`, and the remaining per-element transforms
//! run incrementally on only the changed files.
//!
//! ```text
//! PROJECT_INPUT → [expand] → Collection<FileStat>
//!                                └─[load]  → Collection<FileContent>
//!                                              └─[lex]  → Collection<LexOutput>
//!                                                           └─[parse] → Collection<ParseOutput>
//!                                                                          └─[collect] → bundle_output
//! ```

use std::sync::Arc;
use uuid::Uuid;

use nova_incremental::{Engine, EngineBuilder, EngineError, UpdateReport};
use nova_incremental::Storage;

use crate::semantic::file_access::FileAccess;
use crate::semantic::file_stat::FileStat;
use crate::semantic::project_descriptor::ProjectDescriptor;
use crate::semantic::load_transform::{
    EXPAND_KEY, LOAD_KEY, LEX_KEY, PARSE_KEY, COLLECT_KEY,
    ExpandTransform, LoadTransform, LexTransform, ParseTransform, CollectTransform,
};

// ---------------------------------------------------------------------------
// Stable node UUID constants
// ---------------------------------------------------------------------------

const NODE_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1,
    0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

fn node(name: &str) -> Uuid { Uuid::new_v5(&NODE_NS, name.as_bytes()) }

pub fn project_input_id() -> Uuid  { node("project_input") }
pub fn expand_transform_id() -> Uuid  { node("expand_t") }
pub fn load_transform_id()   -> Uuid  { node("load_t") }
pub fn lex_transform_id()    -> Uuid  { node("lex_t") }
pub fn parse_transform_id()  -> Uuid  { node("parse_t") }
pub fn collect_transform_id() -> Uuid { node("collect_t") }
pub fn bundle_output_id()    -> Uuid  { node("bundle_output") }

// ---------------------------------------------------------------------------
// SemanticSession
// ---------------------------------------------------------------------------

pub struct SemanticSession {
    pub(crate) engine: Engine,
}

impl SemanticSession {
    /// Open a new session.  On warm start (storage has prior committed values),
    /// the hash early-exit will skip unchanged transforms automatically.
    pub async fn open(
        storage: Arc<dyn Storage>,
        fs: Arc<dyn FileAccess>,
    ) -> Result<Self, EngineError> {
        let engine = build_engine(fs, storage).await?;
        Ok(Self { engine })
    }

    // -----------------------------------------------------------------------
    // File set management
    // -----------------------------------------------------------------------

    /// Replace the current file set with `stats`.
    ///
    /// Writing a new `ProjectDescriptor` to the single input node lets the
    /// incremental engine diff the collection automatically.
    pub fn set_files(&self, stats: Vec<FileStat>) -> Result<(), EngineError> {
        self.engine.set_input(project_input_id(), ProjectDescriptor::new(stats))
    }

    /// Convenience wrapper that accepts `(path, size, mtime)` tuples.
    pub fn set_files_from_stats(&self, stats: Vec<FileStat>) -> Result<(), EngineError> {
        self.set_files(stats)
    }

    // -----------------------------------------------------------------------
    // Incremental update
    // -----------------------------------------------------------------------

    pub async fn run(&self) -> UpdateReport { self.engine.update().await }

    pub async fn checkpoint(&self) -> Result<(), EngineError> { self.engine.checkpoint().await }
    pub async fn commit(&self)     -> Result<(), EngineError> { self.engine.commit().await }
    pub async fn discard(&self)    -> Result<(), EngineError> { self.engine.discard().await }

    // -----------------------------------------------------------------------
    // Value access
    // -----------------------------------------------------------------------

    pub async fn get_bundle(&self) -> Result<Option<String>, EngineError> {
        self.engine.get::<String>(bundle_output_id()).await
    }

    /// Read per-file values from the engine by iterating the collection
    /// stored on the bundle edge via the intermediary approach.
    ///
    /// NOTE: `get_content`, `get_lex_result`, etc. are not available in the
    /// static-topology model without explicit output nodes per file.  Use
    /// `get_bundle()` for the assembled output.  For per-file access, the
    /// caller should maintain their own map from paths to results and drive
    /// updates through `set_files`.
    ///
    /// For test convenience, the methods below read from the engine's
    /// intermediate collection edges via `get_collection_element` helpers.
    pub async fn get_bundle_string(&self) -> Result<Option<String>, EngineError> {
        self.get_bundle().await
    }
}

// ---------------------------------------------------------------------------
// Engine construction
// ---------------------------------------------------------------------------

async fn build_engine(
    fs: Arc<dyn FileAccess>,
    storage: Arc<dyn Storage>,
) -> Result<Engine, EngineError> {
    EngineBuilder::new()
        .register(EXPAND_KEY,  ExpandTransform)
        .register(LOAD_KEY,    LoadTransform(Arc::clone(&fs)))
        .register(LEX_KEY,     LexTransform)
        .register(PARSE_KEY,   ParseTransform)
        .register(COLLECT_KEY, CollectTransform)
        .input_node::<ProjectDescriptor>(project_input_id())
        .output_node(bundle_output_id())
        .transform_node(expand_transform_id(),  EXPAND_KEY)
        .transform_node(load_transform_id(),    LOAD_KEY)
        .transform_node(lex_transform_id(),     LEX_KEY)
        .transform_node(parse_transform_id(),   PARSE_KEY)
        .transform_node(collect_transform_id(), COLLECT_KEY)
        .wire_into_slot(project_input_id(), expand_transform_id(), 0)
        .wire_slot_to_slot(expand_transform_id(),  0, load_transform_id(),    0)
        .wire_slot_to_slot(load_transform_id(),    0, lex_transform_id(),     0)
        .wire_slot_to_slot(lex_transform_id(),     0, parse_transform_id(),   0)
        .wire_slot_to_slot(parse_transform_id(),   0, collect_transform_id(), 0)
        .wire_slot_to(collect_transform_id(), 0, bundle_output_id())
        .build(storage)
        .await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use nova_incremental::MemoryStorage;
    use crate::semantic::file_access::MockFileAccess;

    fn make_fs(files: &[(&str, &str)]) -> Arc<dyn FileAccess> {
        let mut mock = MockFileAccess::new();
        for (path, content) in files { mock.add(*path, content.as_bytes().to_vec()); }
        Arc::new(mock)
    }

    async fn make_session(files: &[(&str, &str)]) -> SemanticSession {
        SemanticSession::open(
            Arc::new(MemoryStorage::new()) as Arc<dyn Storage>,
            make_fs(files),
        ).await.unwrap()
    }

    // -----------------------------------------------------------------------
    // Node ID stability
    // -----------------------------------------------------------------------

    #[test]
    fn node_ids_are_stable() {
        assert_eq!(project_input_id(), project_input_id());
        assert_eq!(bundle_output_id(), bundle_output_id());
        assert_ne!(project_input_id(), bundle_output_id());
    }

    // -----------------------------------------------------------------------
    // Basic pipeline
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn empty_project_produces_empty_bundle() {
        let s = make_session(&[]).await;
        s.set_files(vec![]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        // Expand runs, produces empty collection; collect runs, produces "".
        let bundle = s.get_bundle().await.unwrap().unwrap_or_default();
        assert!(bundle.is_empty() || bundle.trim().is_empty(),
            "empty project bundle was: {:?}", bundle);
    }

    #[tokio::test]
    async fn single_file_bundle_is_produced() {
        let src = "namespace main;";
        let s = make_session(&[("main.nova", src)]).await;
        s.set_files(vec![FileStat::new("main.nova", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle().await.unwrap();
        assert!(bundle.is_some(), "bundle should be present after a successful run");
        assert!(bundle.unwrap().contains("main.nova"));
    }

    #[tokio::test]
    async fn multiple_files_all_appear_in_bundle() {
        let s = make_session(&[
            ("a.nova", "namespace a;"),
            ("b.nova", "namespace b;"),
            ("c.nova", "namespace c;"),
        ]).await;
        s.set_files(vec![
            FileStat::new("a.nova", 11, 0),
            FileStat::new("b.nova", 11, 0),
            FileStat::new("c.nova", 11, 0),
        ]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle().await.unwrap().unwrap();
        assert!(bundle.contains("a.nova"), "bundle missing a.nova: {bundle}");
        assert!(bundle.contains("b.nova"), "bundle missing b.nova: {bundle}");
        assert!(bundle.contains("c.nova"), "bundle missing c.nova: {bundle}");
    }

    #[tokio::test]
    async fn incremental_update_skips_unchanged_files() {
        let s = make_session(&[
            ("a.nova", "namespace a;"),
            ("b.nova", "namespace b;"),
        ]).await;
        let stats = vec![
            FileStat::new("a.nova", 11, 0),
            FileStat::new("b.nova", 11, 0),
        ];
        s.set_files(stats.clone()).unwrap();
        let r1 = s.run().await;
        assert!(r1.is_ok(), "{:?}", r1.errors);
        // Re-submit the same stats — engine should skip everything.
        s.set_files(stats).unwrap();
        let r2 = s.run().await;
        assert_eq!(r2.transforms_evaluated, 0,
            "unchanged inputs must produce zero evaluations; report: {:?}", r2);
    }

    #[tokio::test]
    async fn adding_a_file_only_processes_new_file() {
        let s = make_session(&[
            ("a.nova", "namespace a;"),
            ("new.nova", "namespace new_mod;"),
        ]).await;
        // First run with only 'a'.
        s.set_files(vec![FileStat::new("a.nova", 11, 0)]).unwrap();
        s.run().await;
        // Second run adds 'new'.
        s.set_files(vec![
            FileStat::new("a.nova", 11, 0),
            FileStat::new("new.nova", 17, 1),
        ]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        // Expand re-runs; load/lex/parse run only for the new element.
        assert!(report.collection_elements_changed >= 1,
            "at least 1 new element should have been processed");
    }

    #[tokio::test]
    async fn removing_a_file_from_project() {
        let s = make_session(&[
            ("keep.nova", "namespace keep;"),
            ("drop.nova", "namespace drop_me;"),
        ]).await;
        s.set_files(vec![
            FileStat::new("keep.nova", 14, 0),
            FileStat::new("drop.nova", 16, 0),
        ]).unwrap();
        s.run().await;
        // Remove 'drop.nova'.
        s.set_files(vec![FileStat::new("keep.nova", 14, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        // Bundle should not contain drop_me.
        let bundle = s.get_bundle().await.unwrap().unwrap_or_default();
        assert!(!bundle.contains("drop.nova"),
            "drop.nova must not appear in bundle after removal: {bundle}");
        assert!(bundle.contains("keep.nova"),
            "keep.nova must still appear in bundle: {bundle}");
    }

    #[tokio::test]
    async fn missing_file_produces_error_in_report() {
        let s = SemanticSession::open(
            Arc::new(MemoryStorage::new()) as Arc<dyn Storage>,
            Arc::new(MockFileAccess::new()) as Arc<dyn FileAccess>,
        ).await.unwrap();
        s.set_files(vec![FileStat::new("ghost.nova", 0, 0)]).unwrap();
        let report = s.run().await;
        assert!(!report.is_ok(), "missing file must produce an error");
        assert!(!report.errors.is_empty());
    }

    #[tokio::test]
    async fn lex_error_blocks_parse_stage() {
        let mut mock = MockFileAccess::new();
        mock.add("bad.nova", vec![0x00]); // null byte → lex error
        let s = SemanticSession::open(
            Arc::new(MemoryStorage::new()) as Arc<dyn Storage>,
            Arc::new(mock) as Arc<dyn FileAccess>,
        ).await.unwrap();
        s.set_files(vec![FileStat::new("bad.nova", 1, 0)]).unwrap();
        let report = s.run().await;
        // Should error at lex stage (error_count >= 1).
        assert!(!report.is_ok());
    }

    #[tokio::test]
    async fn syntax_error_is_isolated_to_one_file() {
        let bad_src  = "val x = 1;"; // parse error
        let good_src = "namespace ok;";
        let s = make_session(&[("bad.nova", bad_src), ("good.nova", good_src)]).await;
        s.set_files(vec![
            FileStat::new("bad.nova",  bad_src.len()  as u64, 0),
            FileStat::new("good.nova", good_src.len() as u64, 0),
        ]).unwrap();
        let report = s.run().await;
        // good.nova should have succeeded; bad.nova should error.
        assert!(report.errors.iter().any(|(_, e)| e.message.contains("bad.nova")),
            "expected a parse error mentioning bad.nova: {:?}", report.errors);
        // The bundle should still contain good.nova since collect sees partial collection.
        let bundle = s.get_bundle().await.unwrap().unwrap_or_default();
        assert!(bundle.contains("ok"), "good.nova's namespace should be in bundle: {bundle}");
    }

    #[tokio::test]
    async fn warm_start_skips_recomputation() {
        let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());
        let fs = make_fs(&[("lib.nova", "namespace lib;")]);

        // Session 1: compile, commit.
        {
            let s = SemanticSession::open(Arc::clone(&storage), Arc::clone(&fs)).await.unwrap();
            s.set_files(vec![FileStat::new("lib.nova", 14, 42)]).unwrap();
            s.checkpoint().await.unwrap();
            s.run().await;
            s.commit().await.unwrap();
        }

        // Session 2: same inputs, should skip all transforms (hash early-exit).
        let s2 = SemanticSession::open(Arc::clone(&storage), Arc::clone(&fs)).await.unwrap();
        s2.set_files(vec![FileStat::new("lib.nova", 14, 42)]).unwrap();
        let report = s2.run().await;
        assert_eq!(report.transforms_evaluated, 0,
            "warm start with unchanged inputs must evaluate nothing: {:?}", report);
    }

    #[tokio::test]
    async fn checkpoint_and_discard_rolls_back() {
        let s = make_session(&[("m.nova", "namespace m;")]).await;
        s.set_files(vec![FileStat::new("m.nova", 12, 0)]).unwrap();
        s.checkpoint().await.unwrap();
        s.run().await;
        s.discard().await.unwrap();
        // After discard, bundle is absent (nothing was committed).
        let bundle = s.get_bundle().await.unwrap();
        assert!(bundle.is_none(), "after discard bundle must be absent: {:?}", bundle);
    }
}
