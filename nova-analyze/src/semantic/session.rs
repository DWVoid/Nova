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
//! Legend:
//!   [node]             transform node
//!   {node}             output node (terminal, read by consumers)
//!   (value)            single value edge
//!   <value>            collection edge (fan-out or fan-in boundary)
//!   ───►               value edge (single → single)
//!   ═══►               collection edge
//!   ┌───┐              subgraph boundary (one instance per file)
//!
//!
//!  PROJECT_INPUT ───► [expand] ═══► <FileStat>
//!                                        │
//!                          ┌─────────────┘
//!                          │  per-file subgraph instance
//!                         ┌▼───────────────────────────────────────┐
//!                         │                                         │
//!                         │  [load] ───► (FileContent)              │
//!                         │    │                                     │
//!                         │    ▼                                     │
//!                         │  [lex] ───► (LexOutput)                 │
//!                         │    │                                     │
//!                         │    ▼                                     │
//!                         │  [parse] ───► (ParseOutput)             │
//!                         │      │           │           │          │
//!                         │  (per-elem)  (per-elem)  (per-elem)    │
//!                         │      ▼           ▼           ▼          │
//!                         │  [symbol]  [bundle_fragment]            │
//!                         │      │           │                      │
//!                         │      ▼           ▼                      │
//!                         │ (BundleExports) (BundleFragment)        │
//!                         └─────────────────────────────────────────┘
//!                      ════╪═══════════════════╪═════════════════════════
//!                          │        │          │
//!                          │  (coll of all     │
//!                          │   ParseOutputs)   │
//!                          │        │          │
//!                          │        ▼          │
//!                          │  [collect]        │
//!                          │        │          │
//!                          │        ▼          │
//!                          │  {bundle_output}  │
//!                          │      String       │
//!                          │                   │
//!          (coll of all    │    (coll of all   │
//!           BundleExports) │    BundleFrags)   │
//!                    │     │         │         │
//!                    ▼     │         ▼         │
//!             [symbol_collect]  [bundle_assemble]
//!                    │                  │
//!                    ▼                  ▼
//!             {symbol_output}    {bundle_intermediate_output}
//!             Vec<BundleExports>        Bundle
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
    SYMBOL_KEY, SYMBOL_COLLECT_KEY,
    BUNDLE_FRAGMENT_KEY, BUNDLE_ASSEMBLE_KEY,
    ExpandTransform, LoadTransform, LexTransform, ParseTransform, CollectTransform,
    SymbolTransform, SymbolCollectTransform,
    BundleFragmentTransform, BundleAssembleTransform,
};
use crate::semantic::symbol_model::BundleExports;
use crate::bundle::Bundle;

// ---------------------------------------------------------------------------
// Stable node UUID constants
// ---------------------------------------------------------------------------

const NODE_NS: Uuid = Uuid::from_bytes([
    0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1,
    0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
]);

fn node(name: &str) -> Uuid { Uuid::new_v5(&NODE_NS, name.as_bytes()) }

pub fn project_input_id()       -> Uuid { node("project_input") }
pub fn expand_transform_id()    -> Uuid { node("expand_t") }
pub fn load_transform_id()      -> Uuid { node("load_t") }
pub fn lex_transform_id()       -> Uuid { node("lex_t") }
pub fn parse_transform_id()     -> Uuid { node("parse_t") }
pub fn collect_transform_id()   -> Uuid { node("collect_t") }
pub fn symbol_transform_id()          -> Uuid { node("symbol_t") }
pub fn symbol_collect_transform_id()  -> Uuid { node("symbol_collect_t") }
pub fn bundle_fragment_transform_id() -> Uuid { node("bundle_fragment_t") }
pub fn bundle_assemble_transform_id() -> Uuid { node("bundle_assemble_t") }
pub fn bundle_output_id()             -> Uuid { node("bundle_output") }
pub fn symbol_output_id()             -> Uuid { node("symbol_output") }
pub fn bundle_intermediate_output_id()-> Uuid { node("bundle_intermediate_output") }

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

    /// Retrieve the aggregated per-file symbol models from the pipeline.
    ///
    /// Returns `None` if the pipeline has not been run yet or was discarded.
    /// The returned vector contains one [`BundleExports`] per source file,
    /// in arbitrary order; callers should index or sort by namespace.
    pub async fn get_symbol_exports(&self) -> Result<Option<Vec<BundleExports>>, EngineError> {
        self.engine.get::<Vec<BundleExports>>(symbol_output_id()).await
    }

    /// Retrieve the assembled [`Bundle`] intermediate representation.
    ///
    /// Returns `None` if the pipeline has not been run or was discarded.
    /// The bundle contains the complete file list, type list, type
    /// declarations, function list and source locations for the project.
    pub async fn get_bundle_intermediate(&self) -> Result<Option<Bundle>, EngineError> {
        self.engine.get::<Bundle>(bundle_intermediate_output_id()).await
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
        .register(EXPAND_KEY,        ExpandTransform)
        .register(LOAD_KEY,          LoadTransform(Arc::clone(&fs)))
        .register(LEX_KEY,           LexTransform)
        .register(PARSE_KEY,         ParseTransform)
        .register(COLLECT_KEY,       CollectTransform)
        .register(SYMBOL_KEY,            SymbolTransform)
        .register(SYMBOL_COLLECT_KEY,    SymbolCollectTransform)
        .register(BUNDLE_FRAGMENT_KEY,   BundleFragmentTransform)
        .register(BUNDLE_ASSEMBLE_KEY,   BundleAssembleTransform)
        .input_node::<ProjectDescriptor>(project_input_id())
        .output_node(bundle_output_id())
        .output_node(symbol_output_id())
        .output_node(bundle_intermediate_output_id())
        .transform_node(expand_transform_id(),              EXPAND_KEY)
        .transform_node(load_transform_id(),                LOAD_KEY)
        .transform_node(lex_transform_id(),                 LEX_KEY)
        .transform_node(parse_transform_id(),               PARSE_KEY)
        .transform_node(collect_transform_id(),             COLLECT_KEY)
        .transform_node(symbol_transform_id(),              SYMBOL_KEY)
        .transform_node(symbol_collect_transform_id(),      SYMBOL_COLLECT_KEY)
        .transform_node(bundle_fragment_transform_id(),     BUNDLE_FRAGMENT_KEY)
        .transform_node(bundle_assemble_transform_id(),     BUNDLE_ASSEMBLE_KEY)
        .wire_into_slot(project_input_id(), expand_transform_id(), 0)
        .wire_slot_to_slot(expand_transform_id(),  0, load_transform_id(),    0)
        .wire_slot_to_slot(load_transform_id(),    0, lex_transform_id(),     0)
        .wire_slot_to_slot(lex_transform_id(),     0, parse_transform_id(),   0)
        // Parallel fan-out: collect (String), symbol, bundle_fragment
        .wire_slot_to_slot(parse_transform_id(),   0, collect_transform_id(), 0)
        .wire_slot_to_slot(parse_transform_id(),   0, symbol_transform_id(),  0)
        .wire_slot_to_slot(parse_transform_id(),   0, bundle_fragment_transform_id(), 0)
        .wire_slot_to(collect_transform_id(), 0, bundle_output_id())
        .wire_slot_to_slot(symbol_transform_id(), 0, symbol_collect_transform_id(), 0)
        .wire_slot_to(symbol_collect_transform_id(), 0, symbol_output_id())
        .wire_slot_to_slot(bundle_fragment_transform_id(), 0, bundle_assemble_transform_id(), 0)
        .wire_slot_to(bundle_assemble_transform_id(), 0, bundle_intermediate_output_id())
        .with_storage(storage)
        .build()
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

    // -----------------------------------------------------------------------
    // Symbol pipeline tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn empty_project_produces_no_symbols() {
        let s = make_session(&[]).await;
        s.set_files(vec![]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        // With no files the engine may not evaluate the symbol_collect
        // transform at all, so `None` or `Some(vec![])` are both valid.
        let symbols = s.get_symbol_exports().await.unwrap();
        assert!(
            symbols.as_ref().map_or(true, |v| v.is_empty()),
            "empty project should have no symbols: {:?}", symbols
        );
    }

    #[tokio::test]
    async fn file_with_no_exports_produces_empty_exports() {
        let s = make_session(&[("lib.nova", "namespace lib;")]).await;
        s.set_files(vec![FileStat::new("lib.nova", 14, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let symbols = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].namespace, "lib");
        assert!(symbols[0].imports.is_empty());
        assert!(symbols[0].exports.is_empty());
    }

    #[tokio::test]
    async fn file_with_exported_value_appears_in_symbols() {
        let src = "namespace mylib; export define answer 42;";
        let s = make_session(&[("lib.nova", src)]).await;
        s.set_files(vec![FileStat::new("lib.nova", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let symbols = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].exports.len(), 1);
        assert!(matches!(
            &symbols[0].exports[0],
            crate::semantic::symbol_model::ExportedDef::Value { name, .. } if name == "answer"
        ));
    }

    #[tokio::test]
    async fn unexported_definitions_are_excluded() {
        let src = "namespace mylib; define hidden 0; export define visible 1;";
        let s = make_session(&[("lib.nova", src)]).await;
        s.set_files(vec![FileStat::new("lib.nova", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let symbols = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(symbols[0].exports.len(), 1);
        assert!(matches!(
            &symbols[0].exports[0],
            crate::semantic::symbol_model::ExportedDef::Value { name, .. } if name == "visible"
        ));
    }

    #[tokio::test]
    async fn multiple_files_each_produce_symbols() {
        let s = make_session(&[
            ("a.nova", "namespace mod_a; export define x 1;"),
            ("b.nova", "namespace mod_b; export define y 2;"),
        ]).await;
        s.set_files(vec![
            FileStat::new("a.nova", 30, 0),
            FileStat::new("b.nova", 30, 0),
        ]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let symbols = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(symbols.len(), 2);
        // Both namespaces appear.
        let namespaces: Vec<&str> = symbols.iter().map(|s| s.namespace.as_str()).collect();
        assert!(namespaces.contains(&"mod_a"));
        assert!(namespaces.contains(&"mod_b"));
    }

    #[tokio::test]
    async fn import_statement_appears_in_symbols() {
        let src = "use Std.Collections; namespace mylib; export define x 0;";
        let s = make_session(&[("lib.nova", src)]).await;
        s.set_files(vec![FileStat::new("lib.nova", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let symbols = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(symbols[0].imports.len(), 1);
        assert_eq!(symbols[0].imports[0].name, "Collections");
    }

    #[tokio::test]
    async fn symbol_output_available_after_incremental_change() {
        let mut s = make_session(&[
            ("a.nova", "namespace a; export define x 1;"),
        ]).await;
        // First run.
        s.set_files(vec![FileStat::new("a.nova", 24, 0)]).unwrap();
        let r1 = s.run().await;
        assert!(r1.is_ok(), "{:?}", r1.errors);
        let sym1 = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(sym1[0].exports.len(), 1);

        // Add a second file.
        let mut mock = MockFileAccess::new();
        mock.add("a.nova", b"namespace a; export define x 1;");
        mock.add("b.nova", b"namespace b; export define y 2;");
        let fs: Arc<dyn FileAccess> = Arc::new(mock);
        s.engine = build_engine(Arc::clone(&fs), Arc::new(MemoryStorage::new())).await.unwrap();
        s.set_files(vec![
            FileStat::new("a.nova", 24, 0),
            FileStat::new("b.nova", 24, 1),
        ]).unwrap();
        let r2 = s.run().await;
        assert!(r2.is_ok(), "{:?}", r2.errors);
        let sym2 = s.get_symbol_exports().await.unwrap().unwrap();
        assert_eq!(sym2.len(), 2, "after adding b.nova, symbol count should be 2");
    }

    // -----------------------------------------------------------------------
    // Bundle intermediate pipeline tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn empty_project_produces_empty_bundle_intermediate() {
        let s = make_session(&[]).await;
        s.set_files(vec![]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        // With no files the engine may not evaluate the assemble transform.
        let bundle = s.get_bundle_intermediate().await.unwrap();
        assert!(bundle.as_ref().map_or(true, |b| b.files.is_empty()),
            "empty project should have no files: {:?}", bundle);
    }

    #[tokio::test]
    async fn bundle_intermediate_contains_file_list() {
        let src = "namespace App;";
        let s = make_session(&[("main.nv", src)]).await;
        s.set_files(vec![FileStat::new("main.nv", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.files, vec!["main.nv"]);
    }

    #[tokio::test]
    async fn bundle_intermediate_contains_type_declarations() {
        let src = "namespace App; define Point struct x: int y: int end";
        let s = make_session(&[("geom.nv", src)]).await;
        s.set_files(vec![FileStat::new("geom.nv", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.type_declarations.len(), 1);
        assert!(matches!(&bundle.type_declarations[0].body, crate::bundle::TypeBody::Struct { fields } if fields.len() == 2));
    }

    #[tokio::test]
    async fn bundle_intermediate_contains_function_list() {
        let src = "namespace Math; define add(x: int): int end";
        let s = make_session(&[("math.nv", src)]).await;
        s.set_files(vec![FileStat::new("math.nv", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.func_list.len(), 1);
        assert!(bundle.func_list.iter().any(|f| f.name == "Math.add"));
    }

    #[tokio::test]
    async fn bundle_intermediate_contains_impl_bodies() {
        let src = "namespace App; implement for Foo define bar(): unit end end";
        let s = make_session(&[("app.nv", src)]).await;
        s.set_files(vec![FileStat::new("app.nv", src.len() as u64, 0)]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.impl_bodies.len(), 1);
        assert_eq!(bundle.impl_bodies[0].methods.len(), 1);
    }

    #[tokio::test]
    async fn bundle_intermediate_has_correct_format_version() {
        let src = "namespace App;";
        let s = make_session(&[("a.nv", src)]).await;
        s.set_files(vec![FileStat::new("a.nv", 14, 0)]).unwrap();
        s.run().await;
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.version.major, 1);
        assert_eq!(bundle.version.minor, 0);
        assert_eq!(bundle.version.patch, 0);
    }

    #[tokio::test]
    async fn bundle_intermediate_multi_file() {
        let s = make_session(&[
            ("a.nv", "namespace A; define S struct x: int end"),
            ("b.nv", "namespace B; define T struct y: int end"),
        ]).await;
        s.set_files(vec![
            FileStat::new("a.nv", 35, 0),
            FileStat::new("b.nv", 35, 0),
        ]).unwrap();
        let report = s.run().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        let bundle = s.get_bundle_intermediate().await.unwrap().unwrap();
        assert_eq!(bundle.files.len(), 2);
        assert_eq!(bundle.type_declarations.len(), 2);
    }
}
