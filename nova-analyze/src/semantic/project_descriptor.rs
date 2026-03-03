//! [`ProjectDescriptor`] – the single input value that describes the full
//! set of source files to be compiled.
//!
//! ## Design
//!
//! Instead of one input node per file, the pipeline starts with a single
//! `ProjectDescriptor` input node that carries the entire list of `FileStat`
//! values.  An `ExpandTransform` fans this out into a `Collection<FileStat>`,
//! which the subsequent per-element transforms process incrementally.

use serde::{Serialize, Deserialize};
use crate::semantic::file_stat::FileStat;

/// Aggregate input that drives the entire compilation pipeline.
///
/// Write a new `ProjectDescriptor` whenever the file set changes.  The
/// incremental engine will diff the resulting `Collection<FileStat>` against
/// the previous snapshot, re-running only the transforms whose inputs changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDescriptor {
    /// All source files to compile, in any order.  The engine sorts elements
    /// by their `KeyExtractor`-derived key, so order is not significant.
    pub files: Vec<FileStat>,
}

impl ProjectDescriptor {
    pub fn new(files: Vec<FileStat>) -> Self { Self { files } }
    pub fn empty() -> Self { Self { files: vec![] } }
}
