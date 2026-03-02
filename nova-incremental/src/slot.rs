//! Slot schema — describes the typed input/output shape of a [`TransformNode`].
//!
//! Every [`crate::transform::TransformFn`] declares a [`TransformSchema`]
//! which lists the kind and type-key of each input and output slot.  The
//! engine validates connections against this schema at wiring time.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SlotKind
// ---------------------------------------------------------------------------

/// Whether a slot carries a single value or a variable-length ordered
/// collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotKind {
    /// The slot holds exactly one value at a time.
    Single,
    /// The slot holds a variable-length, sorter-ordered list of values.
    ///
    /// `sorter_key` must be registered in the engine via
    /// [`crate::engine::IncrementalEngine::register_sorter`].  It identifies
    /// the comparator function used to maintain a stable element order,
    /// which is required for deterministic incremental diff.
    Collection {
        sorter_key: String,
    },
}

impl SlotKind {
    /// Return `true` if this slot is a collection slot.
    pub fn is_collection(&self) -> bool {
        matches!(self, SlotKind::Collection { .. })
    }

    /// Return the sorter key if this is a `Collection` slot, else `None`.
    pub fn sorter_key(&self) -> Option<&str> {
        match self {
            SlotKind::Collection { sorter_key } => Some(sorter_key),
            SlotKind::Single => None,
        }
    }
}

// ---------------------------------------------------------------------------
// SlotDescriptor
// ---------------------------------------------------------------------------

/// Description of one input or output slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotDescriptor {
    /// Whether this slot is `Single` or `Collection`.
    pub kind: SlotKind,
    /// The registered type key of values flowing through this slot.
    ///
    /// For `Collection` slots, this is the element type (the collection
    /// itself is not a registered type — the engine manages it internally).
    pub type_key: String,
}

impl SlotDescriptor {
    /// Convenience constructor for a single-value slot.
    pub fn single(type_key: impl Into<String>) -> Self {
        Self { kind: SlotKind::Single, type_key: type_key.into() }
    }

    /// Convenience constructor for a collection slot.
    pub fn collection(type_key: impl Into<String>, sorter_key: impl Into<String>) -> Self {
        Self {
            kind: SlotKind::Collection { sorter_key: sorter_key.into() },
            type_key: type_key.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// TransformSchema
// ---------------------------------------------------------------------------

/// The full slot schema for a transform: how many inputs, how many outputs,
/// and the kind/type of each.
///
/// Schemas are immutable after registration.  The engine uses them to:
/// - Validate connections at wiring time.
/// - Determine how to assemble [`SlotInput`]s and interpret [`SlotOutput`]s.
/// - Route collection elements through per-element transform invocations.
///
/// [`SlotInput`]: crate::transform::SlotInput
/// [`SlotOutput`]: crate::transform::SlotOutput
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformSchema {
    /// Ordered input slot descriptors.
    pub inputs: Vec<SlotDescriptor>,
    /// Ordered output slot descriptors.
    pub outputs: Vec<SlotDescriptor>,
}

impl TransformSchema {
    /// Create a schema with the given input and output descriptors.
    pub fn new(inputs: Vec<SlotDescriptor>, outputs: Vec<SlotDescriptor>) -> Self {
        Self { inputs, outputs }
    }

    /// Convenience: a simple 1-input, 1-output single-value schema.
    pub fn one_to_one(in_type: impl Into<String>, out_type: impl Into<String>) -> Self {
        Self {
            inputs:  vec![SlotDescriptor::single(in_type)],
            outputs: vec![SlotDescriptor::single(out_type)],
        }
    }

    /// Convenience: N single-value inputs → 1 single-value output.
    pub fn many_to_one(
        in_types: impl IntoIterator<Item = impl Into<String>>,
        out_type: impl Into<String>,
    ) -> Self {
        Self {
            inputs:  in_types.into_iter().map(|t| SlotDescriptor::single(t)).collect(),
            outputs: vec![SlotDescriptor::single(out_type)],
        }
    }

    /// Convenience: 1 single-value input → M single-value outputs.
    pub fn one_to_many(
        in_type: impl Into<String>,
        out_types: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            inputs:  vec![SlotDescriptor::single(in_type)],
            outputs: out_types.into_iter().map(|t| SlotDescriptor::single(t)).collect(),
        }
    }
}
