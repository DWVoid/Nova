//! Shared stateless transforms for the semantic pipeline.
//!
//! ## Design: Three Stateless Shared Transforms
//!
//! Instead of registering one transform closure per file, all files share
//! three transforms registered once under fixed keys `"load"`, `"lex"`,
//! and `"parse"`.  The **path travels with the value** through the graph:
//!
//! ```text
//! FileStat (path, size, mtime)
//!   └─[load]→ FileContent (path, bytes)
//!               └─[lex]→ LexOutput (path, tokens)
//!                          └─[parse]→ SyntaxResult
//! ```
//!
//! - `FileStat.path` tells the load transform which file to read.
//! - `FileContent.path` is forwarded into `LexOutput` for error context.
//! - `LexOutput.path` is used by the parse transform for error messages.
//!
//! This keeps the graph metadata simple: the edge set is fixed and never
//! grows as files are added.

use std::sync::Arc;
use serde::{Serialize, Deserialize};
use nova_incremental::transform::TransformError;
use crate::semantic::file_access::FileAccess;
use crate::semantic::file_stat::FileStat;
use crate::semantic::file_content::FileContent;
use crate::lexical::LexicalResult;

// ---------------------------------------------------------------------------
// Fixed transform keys
// ---------------------------------------------------------------------------

/// Registry key for the file-load transform.
pub const LOAD_KEY: &str = "load";
/// Registry key for the lex transform.
pub const LEX_KEY: &str = "lex";
/// Registry key for the parse transform.
pub const PARSE_KEY: &str = "parse";

// ---------------------------------------------------------------------------
// LexOutput – carries path through the graph from lex stage to parse stage
// ---------------------------------------------------------------------------

/// The output of the lex stage: a token stream bundled with its source path.
///
/// The source path is not part of `LexicalResult` itself (which is a pure
/// lexer output type), so we carry it here so the parse transform can include
/// it in error messages without any extra bookkeeping.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LexOutput {
    /// The canonical path this lex result was produced from.
    pub path: String,
    /// The lexer output.
    pub lex: LexicalResult,
}

impl LexOutput {
    pub fn new(path: impl Into<String>, lex: LexicalResult) -> Self {
        Self { path: path.into(), lex }
    }
}

// ---------------------------------------------------------------------------
// Load transform (FileStat → FileContent)
// ---------------------------------------------------------------------------

/// Builds the shared `"load"` transform closure.
///
/// The closure reads the path from `FileStat.path` at runtime, so a single
/// closure instance serves every file.
pub fn make_load_fn(
    fs: Arc<dyn FileAccess>,
) -> impl Fn(&FileStat)
        -> std::pin::Pin<Box<dyn std::future::Future<
            Output = Result<FileContent, TransformError>
        > + Send>>
       + Send + Sync + 'static
{
    move |stat: &FileStat| {
        let path  = stat.path.clone();
        let my_fs = Arc::clone(&fs);
        Box::pin(async move {
            let bytes = my_fs.read_file(&path).await.map_err(|e| {
                TransformError::with_source(
                    format!("load({}): read failed", path),
                    e.to_string(),
                )
            })?;
            Ok(FileContent::new(path, bytes))
        })
    }
}

// ---------------------------------------------------------------------------
// Lex transform (FileContent → LexOutput)
// ---------------------------------------------------------------------------

/// The shared `"lex"` transform closure.  Path is read from `FileContent`.
pub fn make_lex_fn()
-> impl Fn(&FileContent)
        -> std::pin::Pin<Box<dyn std::future::Future<
            Output = Result<LexOutput, TransformError>
        > + Send>>
       + Send + Sync + 'static
{
    move |content: &FileContent| {
        let path  = content.path.clone();
        let src_result = std::str::from_utf8(&content.bytes)
            .map(|s| s.to_string())
            .map_err(|e| TransformError::with_source(
                format!("lex({}): file is not valid UTF-8", path),
                e.to_string(),
            ));
        Box::pin(async move {
            let src = src_result?;
            let lex = crate::lexical::transform(&src).map_err(|e| {
                TransformError::with_source(
                    format!("lex({}): lexical error at {}:{}", path,
                        e.position.line(), e.position.column()),
                    e.message.clone(),
                )
            })?;
            Ok(LexOutput::new(path, lex))
        })
    }
}

// ---------------------------------------------------------------------------
// Parse transform (LexOutput → SyntaxResult)
// ---------------------------------------------------------------------------

/// The shared `"parse"` transform closure.  Path is read from `LexOutput`.
pub fn make_parse_fn()
-> impl Fn(&LexOutput)
        -> std::pin::Pin<Box<dyn Future<
            Output = Result<crate::syntax::SyntaxResult, TransformError>
        > + Send>>
       + Send + Sync + 'static
{
    move |lo: &LexOutput| {
        let path      = lo.path.clone();
        let lex_clone = lo.lex.clone();
        Box::pin(async move {
            crate::syntax::transform(lex_clone).map_err(|e| {
                TransformError::with_source(
                    format!("parse({}): syntax error at {}:{}", path,
                        e.position.line(), e.position.column()),
                    e.message.clone(),
                )
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::file_access::MockFileAccess;

    fn make_fs(path: &str, content: &[u8]) -> Arc<dyn FileAccess> {
        let mut mock = MockFileAccess::new();
        mock.add(path, content.to_vec());
        Arc::new(mock)
    }

    #[tokio::test]
    async fn load_fn_loads_file_content() {
        let fs = make_fs("src/main.nova", b"val x = 1;");
        let f  = make_load_fn(fs);
        let stat = FileStat::new("src/main.nova", 10, 0);
        let content = f(&stat).await.unwrap();
        assert_eq!(content.bytes, b"val x = 1;");
        assert_eq!(content.path, "src/main.nova");
    }

    #[tokio::test]
    async fn load_fn_missing_file_is_error() {
        let fs = Arc::new(MockFileAccess::new()) as Arc<dyn FileAccess>;
        let f  = make_load_fn(fs);
        let stat = FileStat::new("missing.nova", 0, 0);
        let result = f(&stat).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("read failed"));
    }

    #[tokio::test]
    async fn load_fn_error_includes_path() {
        let fs = Arc::new(MockFileAccess::new()) as Arc<dyn FileAccess>;
        let f  = make_load_fn(fs);
        let stat = FileStat::new("some/deep/path.nova", 0, 0);
        let err = f(&stat).await.unwrap_err();
        assert!(err.message.contains("some/deep/path.nova"));
    }

    #[tokio::test]
    async fn lex_fn_lexes_valid_source() {
        let f = make_lex_fn();
        let content = FileContent::new("test.nova", b"namespace test;".to_vec());
        let out = f(&content).await.unwrap();
        assert_eq!(out.path, "test.nova");
        assert!(out.lex.tokens.len() >= 3);
    }

    #[tokio::test]
    async fn lex_fn_invalid_utf8_is_error() {
        let f = make_lex_fn();
        let content = FileContent::new("bad.nova", vec![0xFF, 0xFE]);
        let result = f(&content).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not valid UTF-8"));
    }

    #[tokio::test]
    async fn lex_fn_error_includes_path() {
        let f = make_lex_fn();
        // null byte causes lex error
        let content = FileContent::new("src/x.nova", vec![0x00]);
        let err = f(&content).await.unwrap_err();
        assert!(err.message.contains("src/x.nova"));
    }

    #[tokio::test]
    async fn parse_fn_parses_valid_source() {
        let lex_out = LexOutput::new(
            "test.nova",
            crate::lexical::transform("namespace test;").unwrap(),
        );
        let f  = make_parse_fn();
        let sr = f(&lex_out).await.unwrap();
        assert_eq!(sr.chunk.namespace.path.len(), 1);
    }

    #[tokio::test]
    async fn parse_fn_error_includes_path() {
        let lex_out = LexOutput::new(
            "src/bad.nova",
            crate::lexical::transform("val x = 1;").unwrap(),
        );
        let f   = make_parse_fn();
        let err = f(&lex_out).await.unwrap_err();
        assert!(err.message.contains("src/bad.nova"),
            "error must mention the path: {}", err.message);
    }

    #[test]
    fn lex_output_round_trips_path() {
        let lo = LexOutput::new("foo.nova", crate::lexical::transform("namespace x;").unwrap());
        assert_eq!(lo.path, "foo.nova");
    }
}
