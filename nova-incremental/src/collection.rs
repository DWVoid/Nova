//! Variable-length, sorter-ordered collection carried on a graph edge.
//!
//! ## Design
//!
//! A [`CollectionEdge`] is the payload of a `Collection`-typed [`crate::graph::ValueEdge`].
//! Rather than creating N graph edges for N elements, all elements are tracked
//! inside a single `CollectionEdge`.  Per-element dirty flags let the scheduler
//! re-evaluate only the elements that changed.
//!
//! ## Nesting
//!
//! When a transform's `Collection` output slot is connected to another
//! transform's `Collection` input slot, each element of the outer collection
//! is itself a `CollectionEdge` (stored in `CollectionElement::nested`).  The
//! nesting depth is bounded by the user's schema design; the engine does not
//! impose a hard limit.
//!
//! ## Sorter
//!
//! The comparator used to maintain stable element order is looked up by
//! `sorter_key` in the [`crate::registry::SorterRegistry`] at sort time.
//! It must be a pure, deterministic `Fn(&Value, &Value) -> Ordering`.

use crate::value::{Value, ValueHash};

// ---------------------------------------------------------------------------
// ElementKey
// ---------------------------------------------------------------------------

/// A 64-bit opaque key derived from an element's sort position within a
/// collection.  Computed by hashing the element's serialised bytes (so two
/// logically-equal values from different sources share the same key).
///
/// Used to identify an element across successive collection states for diff
/// computation.
pub type ElementKey = u64;

// ---------------------------------------------------------------------------
// CollectionElement
// ---------------------------------------------------------------------------

/// One element inside a [`CollectionEdge`].
#[derive(Clone, Debug)]
pub struct CollectionElement {
    /// Stable identity key for this element within the collection.
    pub key: ElementKey,
    /// The element's value.
    pub value: Value,
    /// Hash of the element's serialised bytes.
    pub hash: ValueHash,
    /// `true` if this element was added or changed since the last evaluation
    /// of the downstream transform.
    pub dirty: bool,
    /// For nested `Collection→Collection` connections: the inner collection
    /// carried by this element.
    pub nested: Option<Box<CollectionEdge>>,
}

// ---------------------------------------------------------------------------
// CollectionEdge
// ---------------------------------------------------------------------------

/// The live state of a `Collection`-typed graph edge.
///
/// Owned by the [`crate::graph::ValueEdge`] whose payload is
/// `EdgePayload::Collection(_)`.
#[derive(Clone, Debug, Default)]
pub struct CollectionEdge {
    /// Elements, kept in sorter-defined order.
    pub elements: Vec<CollectionElement>,
    /// Hash of the concatenated element hashes (in sorted order).
    /// Computed after each mutation and used for whole-collection hash-based
    /// early-exit in the scheduler.
    pub full_hash: ValueHash,
    /// `true` if any element is dirty (or elements were added/removed) since
    /// the last time the downstream transform was evaluated.
    pub dirty: bool,
}

impl CollectionEdge {
    /// Create an empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return `true` if there are no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Return the number of elements.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Find the position of element `key` using binary-search on the
    /// key-sorted `elements` slice, or return `Err(insert_pos)` if absent.
    pub fn find_by_key(&self, key: ElementKey) -> Result<usize, usize> {
        self.elements.partition_point(|e| e.key < key).pipe(|pos| {
            if pos < self.elements.len() && self.elements[pos].key == key {
                Ok(pos)
            } else {
                Err(pos)
            }
        })
    }

    /// Re-sort elements using the provided comparator and rebuild
    /// `ElementKey`s from the element hashes after sorting.
    ///
    /// Returns a [`CollectionDiff`] comparing the old ordered state to the new.
    pub fn sort_and_diff(
        &mut self,
        cmp: &(dyn Fn(&Value, &Value) -> std::cmp::Ordering + Send + Sync),
        old_elements: Vec<CollectionElement>,
    ) -> CollectionDiff {
        // Sort in place.
        self.elements.sort_by(|a, b| cmp(&a.value, &b.value));
        // Rebuild keys from position-independent hash (element content hash).
        for el in &mut self.elements {
            el.key = el.hash; // key == content hash for identity
        }
        // Compute diff.
        CollectionDiff::compute(&old_elements, &self.elements)
    }

    /// Recompute `full_hash` from element hashes.
    pub fn recompute_full_hash(&mut self) {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        for el in &self.elements {
            el.hash.hash(&mut h);
        }
        self.full_hash = h.finish();
    }

    /// Mark all elements clean and the collection as a whole clean.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
        for el in &mut self.elements {
            el.dirty = false;
            if let Some(nested) = &mut el.nested {
                nested.mark_clean();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper trait for pipe
// ---------------------------------------------------------------------------

trait Pipe: Sized {
    fn pipe<F, R>(self, f: F) -> R where F: FnOnce(Self) -> R { f(self) }
}
impl<T> Pipe for T {}

// ---------------------------------------------------------------------------
// CollectionDiff
// ---------------------------------------------------------------------------

/// Describes the difference between two successive states of a collection.
///
/// Passed to transform functions whose input slot is `Collection`, so they
/// can process only the changed elements instead of the entire list.
#[derive(Clone, Debug, Default)]
pub struct CollectionDiff {
    /// Elements added to the collection.
    pub added: Vec<(ElementKey, Value)>,
    /// Element keys removed from the collection.
    pub removed: Vec<ElementKey>,
    /// Elements whose value changed: `(key, old_value, new_value)`.
    pub changed: Vec<(ElementKey, Value, Value)>,
}

impl CollectionDiff {
    /// Compute a diff between `old` and `new` element slices (both assumed
    /// sorted by `ElementKey`).
    pub fn compute(old: &[CollectionElement], new: &[CollectionElement]) -> Self {
        let mut diff = CollectionDiff::default();
        let mut oi = 0usize;
        let mut ni = 0usize;
        while oi < old.len() && ni < new.len() {
            let ok = old[oi].key;
            let nk = new[ni].key;
            match ok.cmp(&nk) {
                std::cmp::Ordering::Equal => {
                    if old[oi].hash != new[ni].hash {
                        diff.changed.push((ok, old[oi].value.clone(), new[ni].value.clone()));
                    }
                    oi += 1; ni += 1;
                }
                std::cmp::Ordering::Less => {
                    diff.removed.push(ok);
                    oi += 1;
                }
                std::cmp::Ordering::Greater => {
                    diff.added.push((nk, new[ni].value.clone()));
                    ni += 1;
                }
            }
        }
        while oi < old.len() { diff.removed.push(old[oi].key); oi += 1; }
        while ni < new.len() { diff.added.push((new[ni].key, new[ni].value.clone())); ni += 1; }
        diff
    }

    /// Return `true` if there are no changes.
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}
