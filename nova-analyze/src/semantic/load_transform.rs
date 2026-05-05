//! Stateless transforms for the Nova semantic pipeline.
//!
//! ## Pipeline (§15 of the design doc)
//!
//! ```text
//! ProjectDescriptor
//!   └─[expand]──► Collection<FileStat>
//!                    └─[load]──► Collection<FileContent>   (per FileStat)
//!                                   └─[lex]──► Collection<LexOutput>
//!                                                └─[parse]──► Collection<ParseOutput>
//!                                                               └─[collect]──► String
//! ```
//!
//! All transforms implement the new [`Transform`] trait so they are fully
//! typed and require no separate value-type registration.

use std::sync::Arc;
use std::hash::{Hash, Hasher, DefaultHasher};
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use nova_incremental::{
    Transform, TransformContext, TransformRegisterContext, TransformError,
    KeyExtractor,
};

use crate::semantic::project_descriptor::ProjectDescriptor;
use crate::semantic::file_access::FileAccess;
use crate::semantic::file_stat::FileStat;
use crate::semantic::file_content::FileContent;

// ---------------------------------------------------------------------------
// LexOutput / ParseOutput (public value types)
// ---------------------------------------------------------------------------

/// The output of the lex stage: a token stream bundled with its source path.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexOutput {
    pub path: String,
    pub lex:  crate::lexical::LexicalResult,
}

impl LexOutput {
    pub fn new(path: impl Into<String>, lex: crate::lexical::LexicalResult) -> Self {
        Self { path: path.into(), lex }
    }
}

/// The output of the parse stage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParseOutput {
    pub path:   String,
    pub result: crate::syntax::SyntaxResult,
}

impl ParseOutput {
    pub fn new(path: impl Into<String>, result: crate::syntax::SyntaxResult) -> Self {
        Self { path: path.into(), result }
    }
}

// ---------------------------------------------------------------------------
// KeyExtractors
// ---------------------------------------------------------------------------

/// Extract a `u64` key for a `FileStat` by hashing its path.
pub struct FileStatByPath;
impl KeyExtractor<FileStat> for FileStatByPath {
    fn extract_key(s: &FileStat) -> u64 {
        let mut h = DefaultHasher::new();
        s.path.hash(&mut h);
        h.finish()
    }
}

/// Extract a `u64` key for a `FileContent` by hashing its path.
pub struct FileContentByPath;
impl KeyExtractor<FileContent> for FileContentByPath {
    fn extract_key(c: &FileContent) -> u64 {
        let mut h = DefaultHasher::new();
        c.path.hash(&mut h);
        h.finish()
    }
}

/// Extract a `u64` key for a `LexOutput` by hashing its path.
pub struct LexOutputByPath;
impl KeyExtractor<LexOutput> for LexOutputByPath {
    fn extract_key(lo: &LexOutput) -> u64 {
        let mut h = DefaultHasher::new();
        lo.path.hash(&mut h);
        h.finish()
    }
}

/// Extract a `u64` key for a `ParseOutput` by hashing its path.
pub struct ParseOutputByPath;
impl KeyExtractor<ParseOutput> for ParseOutputByPath {
    fn extract_key(po: &ParseOutput) -> u64 {
        let mut h = DefaultHasher::new();
        po.path.hash(&mut h);
        h.finish()
    }
}

// ---------------------------------------------------------------------------
// ExpandTransform: ProjectDescriptor → Collection<FileStat>
// ---------------------------------------------------------------------------

/// Expands a [`ProjectDescriptor`] into a `Collection<FileStat>`.
pub struct ExpandTransform;

#[async_trait]
impl Transform for ExpandTransform {
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized {
        ctx.input::<ProjectDescriptor>();
        ctx.output_collection::<FileStat, FileStatByPath>();
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let desc = ctx.input::<ProjectDescriptor>(0)?;
        ctx.output_collection(0, desc.files.clone())
    }
}

// ---------------------------------------------------------------------------
// LoadTransform: FileStat → FileContent  (invoked per element)
// ---------------------------------------------------------------------------

/// Reads file bytes from the virtual filesystem.
pub struct LoadTransform(pub Arc<dyn FileAccess>);

#[async_trait]
impl Transform for LoadTransform {
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized {
        ctx.input::<FileStat>();
        ctx.output::<FileContent>();
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let stat = ctx.input::<FileStat>(0)?;
        let path = stat.path.clone();
        let bytes = self.0.read_file(&path).await.map_err(|e| {
            TransformError::with_source(format!("load({}): read failed", path), e.to_string())
        })?;
        ctx.output(0, FileContent::new(path, bytes))
    }
}

// ---------------------------------------------------------------------------
// LexTransform: FileContent → LexOutput  (invoked per element)
// ---------------------------------------------------------------------------

/// Runs the lexer on file content.
pub struct LexTransform;

#[async_trait]
impl Transform for LexTransform {
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized {
        ctx.input::<FileContent>();
        ctx.output::<LexOutput>();
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let content = ctx.input::<FileContent>(0)?;
        let path = content.path.clone();
        let src  = std::str::from_utf8(&content.bytes).map_err(|e| {
            TransformError::with_source(
                format!("lex({}): file is not valid UTF-8", path), e.to_string(),
            )
        })?;
        let lex = crate::lexical::transform(src).map_err(|e| {
            TransformError::with_source(
                format!("lex({}): lexical error at {}:{}", path,
                    e.position.line(), e.position.column()),
                e.message.clone(),
            )
        })?;
        ctx.output(0, LexOutput::new(path, lex))
    }
}

// ---------------------------------------------------------------------------
// ParseTransform: LexOutput → ParseOutput  (invoked per element)
// ---------------------------------------------------------------------------

/// Runs the parser on lex output.
pub struct ParseTransform;

#[async_trait]
impl Transform for ParseTransform {
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized {
        ctx.input::<LexOutput>();
        ctx.output::<ParseOutput>();
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let lo   = ctx.input::<LexOutput>(0)?;
        let path = lo.path.clone();
        let lex  = lo.lex.clone();
        let result = crate::syntax::transform(lex).map_err(|e| {
            TransformError::with_source(
                format!("parse({}): syntax error at {}:{}", path,
                    e.position.line(), e.position.column()),
                e.message.clone(),
            )
        })?;
        ctx.output(0, ParseOutput::new(path, result))
    }
}

// ---------------------------------------------------------------------------
// CollectTransform: Collection<ParseOutput> → String
// ---------------------------------------------------------------------------

/// Gathers all parsed outputs and serialises them as a textual bundle.
pub struct CollectTransform;

#[async_trait]
impl Transform for CollectTransform {
    fn register(ctx: &mut impl TransformRegisterContext) where Self: Sized {
        ctx.input_collection::<ParseOutput, ParseOutputByPath>();
        ctx.output::<String>();
    }

    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let col = ctx.input_collection::<ParseOutput>(0)?;
        let mut parts: Vec<String> = Vec::with_capacity(col.len());
        for po in col.values() {
            let encoded = crate::formats::textual::encode(&po.result)
                .map_err(|e| TransformError::new(
                    format!("collect encode({}): {}", po.path, e)
                ))?;
            parts.push(format!("## {}\n{}", po.path, encoded));
        }
        // Sort by path for deterministic output.
        parts.sort();
        ctx.output(0, parts.join("\n\n"))
    }
}

// ---------------------------------------------------------------------------
// Re-exported type aliases / constants (backwards compat for session.rs)
// ---------------------------------------------------------------------------

/// Transform registration key constants.
pub const EXPAND_KEY:  &str = "expand";
pub const LOAD_KEY:    &str = "load";
pub const LEX_KEY:     &str = "lex";
pub const PARSE_KEY:   &str = "parse";
pub const COLLECT_KEY: &str = "collect";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::file_access::MockFileAccess;
    use crate::semantic::file_content::FileContent;
    use nova_incremental::{EngineBuilder, MemoryStorage, Uuid};
    use std::sync::Arc;

    fn node(name: &str) -> Uuid {
        const NS: Uuid = Uuid::from_bytes([
            0x6b, 0xa7, 0xb8, 0x14, 0x9d, 0xad, 0x11, 0xd1,
            0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
        ]);
        Uuid::new_v5(&NS, name.as_bytes())
    }

    #[tokio::test]
    async fn expand_produces_collection() {
        let s = Arc::new(MemoryStorage::new());
        let proj_in = node("proj");
        let expand_t = node("expand_t");
        let out = node("out");
        let engine = EngineBuilder::new()
            .register(EXPAND_KEY, ExpandTransform)
            .input_node::<ProjectDescriptor>(proj_in)
            .output_node(out)
            .transform_node(expand_t, EXPAND_KEY)
            .wire_into_slot(proj_in, expand_t, 0)
            .wire_slot_to(expand_t, 0, out)
            .with_storage(s).build().await.unwrap();

        engine.set_input(proj_in, ProjectDescriptor::new(vec![
            FileStat::new("a.nova", 1, 0),
            FileStat::new("b.nova", 2, 0),
        ])).unwrap();

        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        assert_eq!(report.transforms_evaluated, 1);
    }

    #[tokio::test]
    async fn load_transform_reads_file() {
        let mut mock = MockFileAccess::new();
        mock.add("src/main.nova", b"namespace main;".to_vec());
        let fs: Arc<dyn FileAccess> = Arc::new(mock);

        let s = Arc::new(MemoryStorage::new());
        let proj_in  = node("proj2");
        let expand_t = node("expand_t2");
        let load_t   = node("load_t2");
        let out      = node("out2");

        let engine = EngineBuilder::new()
            .register(EXPAND_KEY, ExpandTransform)
            .register(LOAD_KEY, LoadTransform(Arc::clone(&fs)))
            .input_node::<ProjectDescriptor>(proj_in)
            .output_node(out)
            .transform_node(expand_t, EXPAND_KEY)
            .transform_node(load_t, LOAD_KEY)
            .wire_into_slot(proj_in, expand_t, 0)
            .wire_slot_to_slot(expand_t, 0, load_t, 0)
            .wire_slot_to(load_t, 0, out)
            .with_storage(s).build().await.unwrap();

        engine.set_input(proj_in, ProjectDescriptor::new(vec![
            FileStat::new("src/main.nova", 15, 0),
        ])).unwrap();

        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        assert!(report.collection_elements_changed >= 1);
    }

    #[tokio::test]
    async fn lex_transform_runs_on_valid_utf8() {
        // Build a minimal engine to test the lex transform in isolation.
        let s = Arc::new(MemoryStorage::new());
        let mut mock = MockFileAccess::new();
        mock.add("test.nova", b"namespace test;".to_vec());
        let fs: Arc<dyn FileAccess> = Arc::new(mock);
        let content_in = node("lex_content_in");
        let lex_t      = node("lex_t_solo");
        let lex_out    = node("lex_out_solo");
        let engine = EngineBuilder::new()
            .register(LEX_KEY, LexTransform)
            .input_node::<FileContent>(content_in)
            .output_node(lex_out)
            .transform_node(lex_t, LEX_KEY)
            .wire_into_slot(content_in, lex_t, 0)
            .wire_slot_to(lex_t, 0, lex_out)
            .with_storage(s).build().await.unwrap();
        engine.set_input(content_in, FileContent::new("test.nova", b"namespace test;".to_vec())).unwrap();
        let report = engine.update().await;
        assert!(report.is_ok(), "{:?}", report.errors);
        assert_eq!(report.transforms_evaluated, 1);
    }
}
