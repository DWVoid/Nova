//! [`ParseFile`] – incremental transform: [`LexicalResult`] → [`SyntaxResult`].
//!
//! ## Design: Same Pattern as `LexFile`
//!
//! `ParseFile` follows the same thin-wrapper pattern as [`LexFile`]: it
//! downcasts the input [`Value`], delegates to `syntax::transform`, and
//! wraps the output.  No parsing logic lives here.
//!
//! ## Design: Path Comes From a Captured String
//!
//! Unlike `LexFile`, which can read the path from the [`FileContent`] input,
//! the parser's input is a [`LexicalResult`] which does not carry a path.
//! We therefore capture the path at transform construction time so that
//! error messages can still name the source file.
//!
//! This follows the same approach as [`LoadFile`]: one transform instance per
//! file, registered under `"parse:<path>"`.
use async_trait::async_trait;
use crate::incremental::{
    value::Value,
    transform::{OneToOneTransform, TransformError},
};
use crate::lexical::LexicalResult;
/// A 1→1 incremental transform that parses a lexed source file.
///
/// **Input**: a [`Value`] wrapping a [`LexicalResult`].
/// **Output**: a [`Value`] wrapping a [`crate::syntax::SyntaxResult`].
pub struct ParseFile {
    /// Captured path, used only for error message context.
    pub path: String,
}
impl ParseFile {
    /// Create a parse transform that labels errors with `path`.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
    /// Return the registry key for the parse transform of `path`.
    ///
    /// The key is `"parse:<path>"`.
    pub fn registry_key(path: &str) -> String {
        format!("parse:{path}")
    }
}
#[async_trait]
impl OneToOneTransform for ParseFile {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        let lex = input.downcast::<LexicalResult>().ok_or_else(|| {
            TransformError::new(format!(
                "ParseFile({}): input is not a LexicalResult", self.path
            ))
        })?;
        let syntax_result = crate::syntax::transform(lex.clone()).map_err(|e| {
            TransformError::with_source(
                format!("ParseFile({}): syntax error at {}:{}", self.path,
                    e.position.line(), e.position.column()),
                e.message.clone(),
            )
        })?;
        Ok(Value::new(syntax_result))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::SyntaxResult;
    use crate::incremental::transform::OneToOneTransform;
    fn lex(src: &str) -> LexicalResult {
        crate::lexical::transform(src).expect("lex failed")
    }
    #[tokio::test]
    async fn parses_valid_source() {
        let t   = ParseFile::new("test.nova");
        let src = "namespace test;";
        let out = t.apply(&Value::new(lex(src))).await.unwrap();
        let sr  = out.downcast::<SyntaxResult>().unwrap();
        assert_eq!(sr.chunk.namespace.path.len(), 1);
    }
    #[tokio::test]
    async fn wrong_input_type_is_error() {
        let t = ParseFile::new("f.nova");
        let result = t.apply(&Value::new(42i32)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not a LexicalResult"));
    }
    #[tokio::test]
    async fn syntax_error_includes_path() {
        let t = ParseFile::new("src/bad.nova");
        let bad_lex = lex("val x = 1;");
        let err = t.apply(&Value::new(bad_lex)).await.unwrap_err();
        assert!(err.message.contains("src/bad.nova"),
            "error must mention the path: {}", err.message);
    }
    #[test]
    fn registry_key_format() {
        assert_eq!(ParseFile::registry_key("src/foo.nova"), "parse:src/foo.nova");
    }
}
