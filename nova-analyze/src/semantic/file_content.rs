//! [`FileContent`] – the output value produced by the load transform.
//!
//! ## Design: Bytes, Not String
//!
//! The load transform emits raw `Vec<u8>` wrapped in `FileContent`.  We do
//! not convert to `String` here because:
//!
//! - The lexer already handles UTF-8 validation and normalisation.
//! - Storing bytes keeps the load layer agnostic of encoding details.
//! - Two files with the same bytes will have the same content hash, so the
//!   hash-based early-exit optimisation works even if the same content is
//!   reached by a different path (e.g. a symlink).
//!
//! ## Design: Newtype Over `Vec<u8>`
//!
//! Wrapping `Vec<u8>` in a named struct makes type errors visible at compile
//! time.  A transform that expects `FileContent` will not accidentally accept
//! a bare `Vec<u8>` from a different source.

use serde::{Serialize, Deserialize};

/// The raw byte content of a source file, as returned by [`FileAccess`].
///
/// Produced by the [`LoadFile`] transform.  Downstream transforms (lexer,
/// parser, …) receive this value and decode it as needed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileContent {
    /// Raw file bytes.
    pub bytes: Vec<u8>,

    /// The canonical path this content was loaded from.
    ///
    /// Retained here so downstream transforms can include the path in error
    /// messages without needing a separate source-tracking mechanism.
    pub path: String,
}

impl FileContent {
    /// Wrap `bytes` with the originating `path`.
    pub fn new(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { path: path.into(), bytes }
    }

    /// Return the content as a `&str`, or an error if not valid UTF-8.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.bytes)
    }

    /// Return the number of bytes.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Return `true` if the file has no content.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_succeeds_for_valid_utf8() {
        let c = FileContent::new("a.nova", b"hello world".to_vec());
        assert_eq!(c.as_str().unwrap(), "hello world");
    }

    #[test]
    fn as_str_fails_for_invalid_utf8() {
        let c = FileContent::new("a.nova", vec![0xFF, 0xFE]);
        assert!(c.as_str().is_err());
    }

    #[test]
    fn len_reflects_bytes() {
        let c = FileContent::new("a.nova", vec![1, 2, 3]);
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn serde_round_trip() {
        let c = FileContent::new("src/lib.nova", b"-- code".to_vec());
        let bytes = rmp_serde::to_vec(&c).unwrap();
        let back: FileContent = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn equality_is_content_and_path() {
        let a = FileContent::new("x.nova", b"abc".to_vec());
        let b = FileContent::new("x.nova", b"abc".to_vec());
        let c = FileContent::new("y.nova", b"abc".to_vec());
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
