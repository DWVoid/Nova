//! [`LexFile`] – incremental transform: [`FileContent`] → [`LexicalResult`].
//!
//! ## Design: Thin Wrapper, Not Reimplementation
//!
//! The lexer already lives in `crate::lexical::transform`.  This module is
//! just the incremental plumbing that:
//!
//! 1. Downcasts the input [`Value`] to [`FileContent`].
//! 2. Calls `lexical::transform` on the UTF-8 source text.
//! 3. Wraps the result in a [`Value`].
//!
//! Keeping the transform thin means the lexer itself stays unchanged and
//! testable in isolation; only the incremental wiring lives here.
//!
//! ## Design: Path in the Error Message
//!
//! [`FileContent`] carries the originating path alongside the bytes.  We
//! include it in every [`TransformError`] so that error messages in the IDE
//! or build output always identify which file failed, without needing a
//! separate source-tracking mechanism.
//!
//! ## Design: Registry Key
//!
//! The key is `"lex:<path>"`, mirroring the `"load:<path>"` convention from
//! [`LoadFile`].  One transform instance is registered per file so each file
//! gets an independent dirty flag and error state.

use async_trait::async_trait;

use nova_incremental::{
    value::Value,
    transform::{OneToOneTransform, TransformError},
};
use crate::semantic::file_content::FileContent;

/// A 1→1 incremental transform that lexes one source file.
///
/// **Input**: a [`Value`] wrapping a [`FileContent`].  
/// **Output**: a [`Value`] wrapping a [`crate::lexical::LexicalResult`].
pub struct LexFile;

impl LexFile {
    /// Return the registry key for the lex transform of `path`.
    ///
    /// The key is `"lex:<path>"`.
    pub fn registry_key(path: &str) -> String {
        format!("lex:{path}")
    }
}

#[async_trait]
impl OneToOneTransform for LexFile {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        let content = input.downcast::<FileContent>().ok_or_else(|| {
            TransformError::new("LexFile: input is not a FileContent")
        })?;

        let src = content.as_str().map_err(|e| {
            TransformError::with_source(
                format!("LexFile({}): file is not valid UTF-8", content.path),
                e.to_string(),
            )
        })?;

        let lex_result = crate::lexical::transform(src).map_err(|e| {
            TransformError::with_source(
                format!("LexFile({}): lexical error at {}:{}", content.path,
                    e.position.line(), e.position.column()),
                e.message.clone(),
            )
        })?;

        Ok(Value::new(lex_result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic::file_content::FileContent;
    use crate::lexical::LexicalResult;
    use nova_incremental::transform::OneToOneTransform;

    #[tokio::test]
    async fn lexes_valid_source() {
        let t = LexFile;
        let content = FileContent::new("test.nova", b"namespace test;".to_vec());
        let out = t.apply(&Value::new(content)).await.unwrap();
        let lex = out.downcast::<LexicalResult>().unwrap();
        // Should have at least the namespace keyword, identifier, semicolon and EOF.
        assert!(lex.tokens.len() >= 3);
    }

    #[tokio::test]
    async fn wrong_input_type_is_error() {
        let t = LexFile;
        let result = t.apply(&Value::new(42i32)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not a FileContent"));
    }

    #[tokio::test]
    async fn invalid_utf8_is_error() {
        let t = LexFile;
        let content = FileContent::new("bad.nova", vec![0xFF, 0xFE]);
        let result = t.apply(&Value::new(content)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("not valid UTF-8"));
    }

    #[tokio::test]
    async fn lexical_error_includes_path_and_position() {
        let t = LexFile;
        // Null byte is rejected by the lexer.
        let content = FileContent::new("src/main.nova", vec![0x00]);
        let err = t.apply(&Value::new(content)).await.unwrap_err();
        assert!(err.message.contains("src/main.nova"),
            "error must mention the path: {}", err.message);
    }

    #[test]
    fn registry_key_format() {
        assert_eq!(LexFile::registry_key("src/foo.nova"), "lex:src/foo.nova");
    }
}
