//! [`FileStat`] – the input value for a source file node.
//!
//! ## Design: Stat as the Canonical Input
//!
//! The incremental system needs a stable, cheap-to-compare value that
//! represents "what we know about a file without reading it".  File system
//! `stat` metadata fills this role perfectly:
//!
//! - **Path** is the stable identity (used to derive the [`NodeId`] via
//!   UUID v5 so the same file always maps to the same node across restarts).
//! - **Size + mtime** are compared by the hash-based early-exit mechanism.
//!   If both are unchanged, the downstream load transform is skipped entirely.
//! - The struct is `serde`-serialisable so it can be persisted through the
//!   incremental storage backend.
//!
//! ## Design: Path as String, Not PathBuf
//!
//! `PathBuf` is not `serde`-friendly on all platforms (Windows UNC paths can
//! be lossy).  We store the canonical UTF-8 path string and treat it as an
//! opaque identifier.  The [`FileAccess`] trait receives the same string when
//! loading, so no conversion is needed.
use serde::{Serialize, Deserialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
/// Metadata for one source file, used as an input node value.
///
/// Two `FileStat` values are considered **equal** (same hash) when their
/// `path`, `size_bytes`, and `modified_secs` all match.  The incremental
/// engine will skip re-loading a file whose stat is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileStat {
    /// Canonical UTF-8 path that uniquely identifies the file within the
    /// project.  Used both as the node identity key and as the argument
    /// passed to [`FileAccess::read_file`].
    pub path: String,
    /// File size in bytes at last observation.
    pub size_bytes: u64,
    /// Seconds since UNIX epoch at last modification.
    ///
    /// Stored as `u64` rather than `SystemTime` so that the value is
    /// trivially serialisable and comparable.
    pub modified_secs: u64,
}
impl FileStat {
    /// Construct a `FileStat` from its component parts.
    pub fn new(path: impl Into<String>, size_bytes: u64, modified_secs: u64) -> Self {
        Self {
            path: path.into(),
            size_bytes,
            modified_secs,
        }
    }
    /// Convenience constructor that accepts a [`SystemTime`] for the mtime.
    ///
    /// Times before the UNIX epoch are clamped to 0.
    pub fn from_system_time(
        path: impl Into<String>,
        size_bytes: u64,
        modified: SystemTime,
    ) -> Self {
        let modified_secs = modified
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        Self::new(path, size_bytes, modified_secs)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equality_matches_all_fields() {
        let a = FileStat::new("foo.nova", 100, 1_000_000);
        let b = FileStat::new("foo.nova", 100, 1_000_000);
        assert_eq!(a, b);
    }
    #[test]
    fn different_path_not_equal() {
        let a = FileStat::new("foo.nova", 100, 0);
        let b = FileStat::new("bar.nova", 100, 0);
        assert_ne!(a, b);
    }
    #[test]
    fn different_size_not_equal() {
        let a = FileStat::new("f.nova", 10, 0);
        let b = FileStat::new("f.nova", 20, 0);
        assert_ne!(a, b);
    }
    #[test]
    fn serde_round_trip() {
        let s = FileStat::new("src/main.nova", 512, 1_700_000_000);
        let bytes = rmp_serde::to_vec(&s).unwrap();
        let back: FileStat = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(s, back);
    }
    #[test]
    fn from_system_time_is_consistent() {
        let t = UNIX_EPOCH + Duration::from_secs(9999);
        let s = FileStat::from_system_time("f.nova", 1, t);
        assert_eq!(s.modified_secs, 9999);
    }
}
