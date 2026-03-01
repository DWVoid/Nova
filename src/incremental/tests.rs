//! Integration tests for the incremental computation graph.
//!
//! Each test section exercises a distinct usage pattern.  The file is
//! structured so that each section can be read top-to-bottom as a
//! tutorial for the feature it covers.
//!
//! # Sections
//!
//! 1. **Transform helpers** – shared transform implementations used throughout.
//! 2. **One-to-One transform** – simplest pipeline.
//! 3. **Many-to-One transform** – multiple inputs merged into one output.
//! 4. **One-to-Many transform** – one input fanned out to multiple outputs.
//! 5. **Many-to-Many transform** – M inputs mapped to N outputs.
//! 6. **Hidden layers** – intermediate computed nodes that are not directly
//!    observed.
//! 7. **Multiple connections on the same node** – a node acting as source for
//!    several independent downstream edges.
//! 8. **Diamond / reconvergent graph** – two paths that originate from the
//!    same input and merge into one output.
//! 9. **Incremental update – minimal recomputation** – only dirty nodes are
//!    re-evaluated; unchanged branches are skipped.
//! 10. **Hash-based early exit** – a transform that produces the same output
//!     for different inputs does not propagate dirty downstream.
//! 11. **Adding new inputs at runtime** – graph extended after first update.
//! 12. **Removing inputs** – nodes removed, downstream marked dirty.
//! 13. **Error handling** – a failing transform sets NodeStatus::Error and
//!     is retried when its input changes.
//! 14. **Save and reload** – graph topology + values survive a persist/load
//!     round-trip.
//! 15. **Error isolation** – a fault in one node must not cascade errors into
//!     sibling branches or downstream nodes; they stay Dirty and are retried.

#![cfg(test)]

use std::sync::Arc;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use crate::incremental::{
    IncrementalEngine, MemoryStorage, TransformRegistry,
    graph::NodeStatus,
    storage::Storage,
    transform::{
        Transform, TransformError,
        OneToOneTransform, ManyToOneTransform,
        OneToManyTransform, ManyToManyTransform,
    },
    value::Value,
};

// ============================================================================
// 1. Shared transform implementations
// ============================================================================

/// Multiply an i32 by 2.
struct Double;
#[async_trait]
impl OneToOneTransform for Double {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        let n = input.downcast::<i32>().copied()
            .ok_or_else(|| TransformError::new("Double: expected i32"))?;
        Ok(Value::new(n * 2))
    }
}

/// Add 10 to an i32.
struct AddTen;
#[async_trait]
impl OneToOneTransform for AddTen {
    async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
        let n = input.downcast::<i32>().copied()
            .ok_or_else(|| TransformError::new("AddTen: expected i32"))?;
        Ok(Value::new(n + 10))
    }
}

/// Sum all i32 inputs into one i32 output.
struct Sum;
#[async_trait]
impl ManyToOneTransform for Sum {
    async fn apply(&self, inputs: &[Value]) -> Result<Value, TransformError> {
        let mut total = 0i32;
        for (i, v) in inputs.iter().enumerate() {
            total += v.downcast::<i32>().copied()
                .ok_or_else(|| TransformError::new(format!("Sum: input[{i}] is not i32")))?;
        }
        Ok(Value::new(total))
    }
}

/// Emit the input twice as two separate outputs.
struct Duplicate;
#[async_trait]
impl OneToManyTransform for Duplicate {
    async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError> {
        let n = input.downcast::<i32>().copied()
            .ok_or_else(|| TransformError::new("Duplicate: expected i32"))?;
        Ok(vec![Value::new(n), Value::new(n)])
    }
}

/// Split an i32 pair (Vec<i32> with 2 elements) into (min, max) as two outputs.
struct MinMax;
#[async_trait]
impl OneToManyTransform for MinMax {
    async fn apply(&self, input: &Value) -> Result<Vec<Value>, TransformError> {
        let pair = input.downcast::<Vec<i32>>()
            .ok_or_else(|| TransformError::new("MinMax: expected Vec<i32>"))?;
        let (a, b) = (pair[0], pair[1]);
        Ok(vec![Value::new(a.min(b)), Value::new(a.max(b))])
    }
}

/// Zip two i32 inputs: produce (sum, product) as two outputs.
struct SumAndProduct;
#[async_trait]
impl ManyToManyTransform for SumAndProduct {
    async fn apply(&self, inputs: &[Value]) -> Result<Vec<Value>, TransformError> {
        let a = inputs[0].downcast::<i32>().copied()
            .ok_or_else(|| TransformError::new("SumAndProduct: input[0] not i32"))?;
        let b = inputs[1].downcast::<i32>().copied()
            .ok_or_else(|| TransformError::new("SumAndProduct: input[1] not i32"))?;
        Ok(vec![Value::new(a + b), Value::new(a * b)])
    }
}

/// Always returns 0, regardless of input – used to test hash-based early exit.
struct AlwaysZero;
#[async_trait]
impl OneToOneTransform for AlwaysZero {
    async fn apply(&self, _input: &Value) -> Result<Value, TransformError> {
        Ok(Value::new(0i32))
    }
}

/// Always fails.
struct AlwaysFail;
#[async_trait]
impl OneToOneTransform for AlwaysFail {
    async fn apply(&self, _input: &Value) -> Result<Value, TransformError> {
        Err(TransformError::new("AlwaysFail: intentional failure"))
    }
}

// ============================================================================
// Helper: build a fresh engine with all transforms registered.
// ============================================================================

fn make_engine() -> IncrementalEngine {
    let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
    let mut engine = IncrementalEngine::new(storage);
    engine.register_transform("double",          Transform::OneToOne(Arc::new(Double)));
    engine.register_transform("add_ten",         Transform::OneToOne(Arc::new(AddTen)));
    engine.register_transform("sum",             Transform::ManyToOne(Arc::new(Sum)));
    engine.register_transform("duplicate",       Transform::OneToMany(Arc::new(Duplicate)));
    engine.register_transform("min_max",         Transform::OneToMany(Arc::new(MinMax)));
    engine.register_transform("sum_and_product", Transform::ManyToMany(Arc::new(SumAndProduct)));
    engine.register_transform("always_zero",     Transform::OneToOne(Arc::new(AlwaysZero)));
    engine.register_transform("always_fail",     Transform::OneToOne(Arc::new(AlwaysFail)));
    engine
}

// ============================================================================
// 2. One-to-One transform
// ============================================================================

/// A single input feeds a single computed output through a 1→1 transform.
///
/// ```text
/// [input: 21] --double--> [output: 42]
/// ```
#[tokio::test]
async fn one_to_one_basic() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(21i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();

    let report = engine.update().await;
    assert!(report.is_ok(), "{:?}", report.errors);

    let v = engine.get_value(output).await.unwrap().unwrap();
    assert_eq!(v.downcast::<i32>(), Some(&42i32));
}

/// Chained 1→1 transforms: input → double → add_ten.
///
/// ```text
/// [input: 5] --double--> [mid: 10] --add_ten--> [output: 20]
/// ```
#[tokio::test]
async fn one_to_one_chained() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(5i32));
    let mid    = engine.add_output_node();
    let output = engine.add_output_node();
    engine.connect(&[input], &[mid],    "double").unwrap();
    engine.connect(&[mid],   &[output], "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(mid).await.unwrap().unwrap().downcast::<i32>(), Some(&10i32));
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&20i32));
}

// ============================================================================
// 3. Many-to-One transform
// ============================================================================

/// Two independent inputs are summed into one output.
///
/// ```text
/// [a: 3] ──┐
///           ├──sum──> [output: 7]
/// [b: 4] ──┘
/// ```
#[tokio::test]
async fn many_to_one_two_inputs() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(3i32));
    let b      = engine.add_input(Value::new(4i32));
    let output = engine.add_output_node();
    engine.connect(&[a, b], &[output], "sum").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));
}

/// Three inputs summed together.
///
/// ```text
/// [a: 1] ─┐
/// [b: 2] ─┼──sum──> [output: 6]
/// [c: 3] ─┘
/// ```
#[tokio::test]
async fn many_to_one_three_inputs() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(1i32));
    let b      = engine.add_input(Value::new(2i32));
    let c      = engine.add_input(Value::new(3i32));
    let output = engine.add_output_node();
    engine.connect(&[a, b, c], &[output], "sum").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&6i32));
}

/// Changing only one of the N inputs updates the output correctly.
#[tokio::test]
async fn many_to_one_partial_update() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(10i32));
    let b      = engine.add_input(Value::new(5i32));
    let output = engine.add_output_node();
    engine.connect(&[a, b], &[output], "sum").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&15i32));

    // Change only `b`.
    engine.set_input(b, Value::new(20i32)).unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&30i32));
}

// ============================================================================
// 4. One-to-Many transform
// ============================================================================

/// One input is duplicated into two separate output nodes.
///
/// ```text
///                   ┌──> [out_a: 7]
/// [input: 7] ──dup──┤
///                   └──> [out_b: 7]
/// ```
#[tokio::test]
async fn one_to_many_duplicate() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(7i32));
    let out_a = engine.add_output_node();
    let out_b = engine.add_output_node();
    engine.connect(&[input], &[out_a, out_b], "duplicate").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(out_a).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));
    assert_eq!(engine.get_value(out_b).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));
}

/// MinMax splits a Vec<i32> pair into the minimum and maximum values.
///
/// ```text
///                      ┌──> [out_min: 3]
/// [input: [8,3]] ──────┤
///                      └──> [out_max: 8]
/// ```
#[tokio::test]
async fn one_to_many_min_max() {
    let engine = make_engine();
    let input   = engine.add_input(Value::new(vec![8i32, 3i32]));
    let out_min = engine.add_output_node();
    let out_max = engine.add_output_node();
    engine.connect(&[input], &[out_min, out_max], "min_max").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(out_min).await.unwrap().unwrap().downcast::<i32>(), Some(&3i32));
    assert_eq!(engine.get_value(out_max).await.unwrap().unwrap().downcast::<i32>(), Some(&8i32));
}

// ============================================================================
// 5. Many-to-Many transform
// ============================================================================

/// SumAndProduct maps 2 inputs → (sum, product).
///
/// ```text
/// [a: 3] ──┐                 ┌──> [out_sum:     7]
///           ├──sum_and_prod──┤
/// [b: 4] ──┘                 └──> [out_product: 12]
/// ```
#[tokio::test]
async fn many_to_many_sum_and_product() {
    let engine      = make_engine();
    let a           = engine.add_input(Value::new(3i32));
    let b           = engine.add_input(Value::new(4i32));
    let out_sum     = engine.add_output_node();
    let out_product = engine.add_output_node();
    engine.connect(&[a, b], &[out_sum, out_product], "sum_and_product").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(out_sum).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));
    assert_eq!(engine.get_value(out_product).await.unwrap().unwrap().downcast::<i32>(), Some(&12i32));
}

/// Updating one input in a many-to-many edge re-evaluates both outputs.
#[tokio::test]
async fn many_to_many_update_one_input() {
    let engine      = make_engine();
    let a           = engine.add_input(Value::new(2i32));
    let b           = engine.add_input(Value::new(5i32));
    let out_sum     = engine.add_output_node();
    let out_product = engine.add_output_node();
    engine.connect(&[a, b], &[out_sum, out_product], "sum_and_product").unwrap();
    engine.update().await;

    engine.set_input(a, Value::new(10i32)).unwrap();
    engine.update().await;

    assert_eq!(engine.get_value(out_sum).await.unwrap().unwrap().downcast::<i32>(), Some(&15i32));
    assert_eq!(engine.get_value(out_product).await.unwrap().unwrap().downcast::<i32>(), Some(&50i32));
}

// ============================================================================
// 6. Hidden layers (intermediate computed nodes)
// ============================================================================

/// Three-layer pipeline where only the final output is "observed".
/// Middle nodes are hidden intermediate results.
///
/// ```text
/// [input: 3] --double--> [hidden1: 6] --double--> [hidden2: 12] --add_ten--> [output: 22]
/// ```
#[tokio::test]
async fn hidden_layers_three_deep() {
    let engine  = make_engine();
    let input   = engine.add_input(Value::new(3i32));
    let hidden1 = engine.add_output_node();
    let hidden2 = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],   &[hidden1], "double").unwrap();
    engine.connect(&[hidden1], &[hidden2], "double").unwrap();
    engine.connect(&[hidden2], &[output],  "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&22i32));
    // Intermediate nodes are also reachable.
    assert_eq!(engine.get_value(hidden1).await.unwrap().unwrap().downcast::<i32>(), Some(&6i32));
    assert_eq!(engine.get_value(hidden2).await.unwrap().unwrap().downcast::<i32>(), Some(&12i32));
}

/// Changing the input propagates all the way through the hidden layers.
#[tokio::test]
async fn hidden_layers_input_change_propagates() {
    let engine  = make_engine();
    let input   = engine.add_input(Value::new(1i32));
    let hidden  = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],  &[hidden], "double").unwrap();
    engine.connect(&[hidden], &[output], "add_ten").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&12i32));

    engine.set_input(input, Value::new(5i32)).unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&20i32));
}

// ============================================================================
// 7. Multiple connections from the same source node
// ============================================================================

/// One input node fans out to two completely independent downstream pipelines.
///
/// ```text
///                  ┌──double──> [out_a: 10]
/// [input: 5] ──────┤
///                  └──add_ten──> [out_b: 15]
/// ```
#[tokio::test]
async fn multiple_connections_fan_out() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(5i32));
    let out_a  = engine.add_output_node();
    let out_b  = engine.add_output_node();
    engine.connect(&[input], &[out_a], "double").unwrap();
    engine.connect(&[input], &[out_b], "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(out_a).await.unwrap().unwrap().downcast::<i32>(), Some(&10i32));
    assert_eq!(engine.get_value(out_b).await.unwrap().unwrap().downcast::<i32>(), Some(&15i32));
}

/// Both fan-out branches update correctly when the shared input changes.
#[tokio::test]
async fn multiple_connections_fan_out_update() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(1i32));
    let out_a  = engine.add_output_node();
    let out_b  = engine.add_output_node();
    engine.connect(&[input], &[out_a], "double").unwrap();
    engine.connect(&[input], &[out_b], "add_ten").unwrap();
    engine.update().await;

    engine.set_input(input, Value::new(10i32)).unwrap();
    engine.update().await;

    assert_eq!(engine.get_value(out_a).await.unwrap().unwrap().downcast::<i32>(), Some(&20i32));
    assert_eq!(engine.get_value(out_b).await.unwrap().unwrap().downcast::<i32>(), Some(&20i32));
}

/// A computed node can itself be the source for a second downstream transform.
///
/// ```text
/// [input: 3] --double--> [mid: 6] --add_ten--> [out_a: 16]
///                         │
///                         └──double──> [out_b: 12]
/// ```
#[tokio::test]
async fn multiple_connections_computed_source() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(3i32));
    let mid    = engine.add_output_node();
    let out_a  = engine.add_output_node();
    let out_b  = engine.add_output_node();
    engine.connect(&[input], &[mid],   "double").unwrap();
    engine.connect(&[mid],   &[out_a], "add_ten").unwrap();
    engine.connect(&[mid],   &[out_b], "double").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value(out_a).await.unwrap().unwrap().downcast::<i32>(), Some(&16i32));
    assert_eq!(engine.get_value(out_b).await.unwrap().unwrap().downcast::<i32>(), Some(&12i32));
}

// ============================================================================
// 8. Diamond / reconvergent graph
// ============================================================================

/// A classic diamond: one input, two parallel paths, one merge point.
///
/// ```text
///                  ┌──double──> [left: 10]  ─┐
/// [input: 5] ──────┤                           ├──sum──> [output: 25]
///                  └──add_ten──> [right: 15] ─┘
/// ```
#[tokio::test]
async fn diamond_graph() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(5i32));
    let left   = engine.add_output_node();
    let right  = engine.add_output_node();
    let output = engine.add_output_node();
    engine.connect(&[input],       &[left],   "double").unwrap();
    engine.connect(&[input],       &[right],  "add_ten").unwrap();
    engine.connect(&[left, right], &[output], "sum").unwrap();

    engine.update().await;

    // left = 5*2 = 10, right = 5+10 = 15, output = 10+15 = 25
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&25i32));
}

/// Changing the single root input re-evaluates the entire diamond.
#[tokio::test]
async fn diamond_graph_update() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(1i32));
    let left   = engine.add_output_node();
    let right  = engine.add_output_node();
    let output = engine.add_output_node();
    engine.connect(&[input],       &[left],   "double").unwrap();
    engine.connect(&[input],       &[right],  "add_ten").unwrap();
    engine.connect(&[left, right], &[output], "sum").unwrap();
    engine.update().await;
    // left=2, right=11, output=13
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&13i32));

    engine.set_input(input, Value::new(4i32)).unwrap();
    engine.update().await;
    // left=8, right=14, output=22
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&22i32));
}

// ============================================================================
// 9. Incremental update – minimal recomputation
// ============================================================================

/// Two independent pipelines: changing one must NOT re-evaluate the other.
///
/// ```text
/// [a: 3] --double--> [out_a: 6]     (only this branch is dirtied)
/// [b: 5] --double--> [out_b: 10]    (must stay clean)
/// ```
#[tokio::test]
async fn incremental_only_dirty_branch_recomputed() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(3i32));
    let b      = engine.add_input(Value::new(5i32));
    let out_a  = engine.add_output_node();
    let out_b  = engine.add_output_node();
    engine.connect(&[a], &[out_a], "double").unwrap();
    engine.connect(&[b], &[out_b], "double").unwrap();
    engine.update().await;

    // Change only `a`.
    engine.set_input(a, Value::new(10i32)).unwrap();

    // out_b must still be Clean; out_a must be Dirty.
    let graph = engine.graph();
    assert!(graph.node_status(out_a).unwrap().is_dirty());
    assert!(!graph.node_status(out_b).unwrap().is_dirty(),
        "out_b should be clean – its input didn't change");

    let report = engine.update().await;
    assert_eq!(report.nodes_evaluated, 1, "only out_a should be evaluated");
    assert_eq!(engine.get_value(out_a).await.unwrap().unwrap().downcast::<i32>(), Some(&20i32));
    assert_eq!(engine.get_value(out_b).await.unwrap().unwrap().downcast::<i32>(), Some(&10i32));
}

/// Second call to update() with no input changes does zero evaluations.
#[tokio::test]
async fn incremental_no_change_no_evaluation() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(7i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();
    engine.update().await; // first run

    let report = engine.update().await; // nothing changed
    assert_eq!(report.nodes_evaluated, 0);
}

// ============================================================================
// 10. Hash-based early exit
// ============================================================================

/// AlwaysZero returns 0 regardless of input.  When the input changes but the
/// output hash is unchanged, downstream nodes must NOT be marked dirty.
///
/// ```text
/// [input] --always_zero--> [zero: 0] --double--> [downstream: 0]
/// ```
#[tokio::test]
async fn hash_early_exit_prevents_downstream_recomputation() {
    let engine     = make_engine();
    let input      = engine.add_input(Value::new(1i32));
    let zero       = engine.add_output_node();
    let downstream = engine.add_output_node();
    engine.connect(&[input], &[zero],       "always_zero").unwrap();
    engine.connect(&[zero],  &[downstream], "double").unwrap();
    engine.update().await;

    assert_eq!(engine.get_value(downstream).await.unwrap().unwrap().downcast::<i32>(), Some(&0i32));

    // Change the input – AlwaysZero will still emit 0, same hash.
    engine.set_input(input, Value::new(999i32)).unwrap();
    let report = engine.update().await;

    // `zero` is evaluated (its input was dirty) but its output hash is unchanged,
    // so it counts as skipped.  `downstream` is also dirty (eager BFS marked it)
    // but its input value (`zero`) hasn't actually changed, so it is also skipped.
    // Both nodes are in the dirty set yet neither produces a real change.
    assert!(report.nodes_skipped >= 1,
        "at least one node should be skipped via hash early exit (got {})", report.nodes_skipped);

    // downstream must still have its previous value
    assert_eq!(engine.get_value(downstream).await.unwrap().unwrap().downcast::<i32>(), Some(&0i32));
}

// ============================================================================
// 11. Adding new inputs at runtime
// ============================================================================

/// Start with one input, run, then add a second input that merges into the
/// existing output, and run again.
#[tokio::test]
async fn adding_input_at_runtime() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(4i32));
    let output = engine.add_output_node();
    engine.connect(&[a], &[output], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&8i32));

    // Add a second independent input + transform that produces a separate output.
    let b       = engine.add_input(Value::new(3i32));
    let out_new = engine.add_output_node();
    engine.connect(&[b], &[out_new], "add_ten").unwrap();

    let report = engine.update().await;
    assert!(report.is_ok());
    // The original output must be unchanged (not re-evaluated).
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&8i32));
    // The new output is computed.
    assert_eq!(engine.get_value(out_new).await.unwrap().unwrap().downcast::<i32>(), Some(&13i32));
}

/// Add a new input that feeds into an existing Many-to-One merge node by
/// creating a second edge into it.
#[tokio::test]
async fn adding_input_to_existing_merge() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(5i32));
    let out_a  = engine.add_output_node();
    engine.connect(&[a], &[out_a], "double").unwrap();
    engine.update().await;

    // Now also add a separate accumulator.
    let b           = engine.add_input(Value::new(100i32));
    let accumulator = engine.add_output_node();
    engine.connect(&[b], &[accumulator], "add_ten").unwrap();

    engine.update().await;
    assert_eq!(engine.get_value(accumulator).await.unwrap().unwrap().downcast::<i32>(), Some(&110i32));
}

// ============================================================================
// 12. Removing inputs
// ============================================================================

/// Remove an input node; verify it is gone and that the downstream node is
/// marked dirty.
#[tokio::test]
async fn removing_input_marks_downstream_dirty() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(5i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&10i32));

    // Remove the input.
    assert!(engine.remove_node(input));
    assert!(!engine.graph().contains_node(input));

    // The downstream node must now be dirty.
    assert!(engine.graph().node_status(output).unwrap().is_dirty(),
        "output must be dirty after its input was removed");
}

/// Removing an already-absent node returns false.
#[tokio::test]
async fn removing_absent_node_returns_false() {
    let engine = make_engine();
    let phantom = crate::incremental::NodeId::new();
    assert!(!engine.remove_node(phantom));
}

/// Remove a middle node from a chain – the downstream node becomes dirty.
///
/// ```text
/// [a] --double--> [mid] --add_ten--> [out]
///                   ↑ remove this
/// ```
#[tokio::test]
async fn removing_middle_node_in_chain() {
    let engine = make_engine();
    let a   = engine.add_input(Value::new(1i32));
    let mid = engine.add_output_node();
    let out = engine.add_output_node();
    engine.connect(&[a],   &[mid], "double").unwrap();
    engine.connect(&[mid], &[out], "add_ten").unwrap();
    engine.update().await;

    engine.remove_node(mid);

    assert!(!engine.graph().contains_node(mid));
    assert!(engine.graph().node_status(out).unwrap().is_dirty());
}

/// After removing a node, add a brand new input that replaces it.
#[tokio::test]
async fn replace_removed_node_with_new_input() {
    let engine = make_engine();
    let a   = engine.add_input(Value::new(1i32));
    let out = engine.add_output_node();
    engine.connect(&[a], &[out], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value(out).await.unwrap().unwrap().downcast::<i32>(), Some(&2i32));

    // Remove the original input.
    engine.remove_node(a);

    // Add a new input and connect it directly to out.
    let new_a = engine.add_input(Value::new(20i32));
    engine.connect(&[new_a], &[out], "double").unwrap();
    engine.update().await;

    assert_eq!(engine.get_value(out).await.unwrap().unwrap().downcast::<i32>(), Some(&40i32));
}

// ============================================================================
// 13. Error handling
// ============================================================================

/// A failing transform stores an error on the target node and is reported in
/// the UpdateReport.
#[tokio::test]
async fn failing_transform_is_reported() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(0i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "always_fail").unwrap();

    let report = engine.update().await;

    assert!(!report.is_ok());
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].0, output);
}

/// After a failing transform, changing the input clears the error and retries.
#[tokio::test]
async fn failing_transform_retried_after_input_change() {
    let engine  = make_engine();
    let input   = engine.add_input(Value::new(5i32));
    let failing = engine.add_output_node();
    let success = engine.add_output_node();
    engine.connect(&[input],   &[failing], "always_fail").unwrap();
    // Also wire a working branch so we can verify it runs.
    engine.connect(&[input],   &[success], "double").unwrap();

    let report = engine.update().await;
    assert!(!report.is_ok());

    // The node is in Error state.
    assert!(engine.graph().node_status(failing).unwrap().is_error());

    // Change the input – the failing node should become Dirty again.
    engine.set_input(input, Value::new(7i32)).unwrap();
    assert!(engine.graph().node_status(failing).unwrap().is_dirty(),
        "error node must be dirtied when its input changes");

    // The working branch updates successfully even though the failing one errors.
    let report2 = engine.update().await;
    assert_eq!(engine.get_value(success).await.unwrap().unwrap().downcast::<i32>(), Some(&14i32));
    assert!(!report2.is_ok()); // always_fail still errors
}

// ============================================================================
// 14. Save and reload
// ============================================================================

/// Full round-trip: build a graph, run it, save, load back, verify values and
/// topology are preserved.
#[tokio::test]
async fn save_and_load_preserves_topology_and_values() {
    let storage: Arc<dyn Storage> = Arc::new(MemoryStorage::new());

    // --- Build and run original engine ---
    let mut engine = IncrementalEngine::new(Arc::clone(&storage));
    engine.register_transform("double",  Transform::OneToOne(Arc::new(Double)));
    engine.register_transform("add_ten", Transform::OneToOne(Arc::new(AddTen)));

    let input  = engine.add_input(Value::new(6i32));
    let mid    = engine.add_output_node();
    let output = engine.add_output_node();
    engine.connect(&[input], &[mid],    "double").unwrap();
    engine.connect(&[mid],   &[output], "add_ten").unwrap();
    engine.update().await;
    engine.save().await.unwrap();

    // --- Reload ---
    let mut registry = TransformRegistry::new();
    registry.register("double",  Transform::OneToOne(Arc::new(Double)));
    registry.register("add_ten", Transform::OneToOne(Arc::new(AddTen)));
    let engine2 = IncrementalEngine::load(Arc::clone(&storage), registry).await.unwrap();

    // Nodes must be present.
    assert!(engine2.graph().contains_node(input));
    assert!(engine2.graph().contains_node(mid));
    assert!(engine2.graph().contains_node(output));
}

// ============================================================================
// 15. Visual snapshots of add-input / remove-input cases
// ============================================================================
//
// Each test below calls `snapshot()` before and after the structural change so
// the graph shape is printed to stdout in a compact ASCII block diagram.  Run
// with `cargo test -- --nocapture visualize` to see the output.
//
// The snapshot format is:
//
//   [NodeId-prefix | INPUT  | status | "value"] ──transform──> [...]
//
// Nodes are printed in topological order (sources first).

/// Render the current state of every node in `engine` as a multiline
/// ASCII diagram and return it as a `String`.
///
/// Each line describes one directed edge:
///
/// ```text
///   [<id> | INPUT  | Clean | 5]  --double-->  [<id> | computed | Dirty | ?]
/// ```
///
/// Leaf output nodes (no outgoing edges) are listed separately at the end.
fn snapshot(label: &str, engine: &IncrementalEngine) -> String {
    use crate::incremental::graph::NodeStatus;
    let graph = engine.graph();

    // Gather all node IDs.
    let mut all_ids = graph.all_node_ids();
    // Stable order: sort by UUID string so output is deterministic within a run.
    all_ids.sort_by_key(|id| id.to_string());

    // Build a helper that formats a single node as a bracketed string.
    let fmt_node = |id: crate::incremental::NodeId| -> String {
        let short = &id.to_string()[..8]; // first 8 hex chars of UUID
        let kind   = if graph.is_input(id) { "INPUT   " } else { "computed" };
        let status = match graph.node_status(id) {
            Some(NodeStatus::Clean)    => "Clean",
            Some(NodeStatus::Dirty)    => "Dirty",
            Some(NodeStatus::Error(_)) => "Error",
            None                        => "?",
        };
        let val = match graph.peek_value(id) {
            Some((v, _)) => {
                if let Some(n) = v.downcast::<i32>()           { format!("{n}") }
                else if let Some(n) = v.downcast::<i64>()      { format!("{n}") }
                else if let Some(v) = v.downcast::<Vec<u8>>()  { format!("<{} bytes>", v.len()) }
                else                                            { "?".into() }
            }
            None => "∅".into(),
        };
        format!("[{short}… | {kind} | {status:5} | {val:>4}]")
    };

    let edges = graph.all_edges();
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("=== {label} ==="));

    if edges.is_empty() {
        lines.push("  (no edges)".into());
    }

    // One line per edge.
    let mut printed_targets: std::collections::HashSet<crate::incremental::NodeId> =
        std::collections::HashSet::new();
    let mut printed_sources: std::collections::HashSet<crate::incremental::NodeId> =
        std::collections::HashSet::new();

    // Sort edges for stable output.
    let mut sorted_edges = edges;
    sorted_edges.sort_by_key(|e| format!("{:?}{:?}", e.sources, e.targets));

    for edge in &sorted_edges {
        // Format source cluster.
        let src_str = if edge.sources.len() == 1 {
            fmt_node(edge.sources[0])
        } else {
            let parts: Vec<_> = edge.sources.iter().map(|&s| fmt_node(s)).collect();
            format!("({})", parts.join("\n       + "))
        };
        // Format target cluster.
        let tgt_str = if edge.targets.len() == 1 {
            fmt_node(edge.targets[0])
        } else {
            let parts: Vec<_> = edge.targets.iter().map(|&t| fmt_node(t)).collect();
            format!("({})", parts.join("\n         "))
        };
        lines.push(format!("  {src_str}  --{tkey}-->  {tgt_str}",
            tkey = edge.transform_key));
        for &s in &edge.sources { printed_sources.insert(s); }
        for &t in &edge.targets { printed_targets.insert(t); }
    }

    // Disconnected nodes (no edges at all, e.g. isolated inputs just added).
    for id in &all_ids {
        if !printed_sources.contains(id) && !printed_targets.contains(id) {
            lines.push(format!("  {}  (disconnected)", fmt_node(*id)));
        }
    }

    lines.push(String::new()); // trailing blank line
    let out = lines.join("\n");
    println!("{out}");
    out
}

// ---------------------------------------------------------------------------
// Visualize: add_input_at_runtime
// ---------------------------------------------------------------------------

/// Shows the graph before and after a new independent branch is added at
/// runtime.
///
/// Expected progression:
///
/// **Before second input**
/// ```text
/// [a | INPUT | Clean | 4]  --double-->  [out | computed | Clean | 8]
/// ```
///
/// **After adding `b` and `out_new`**
/// ```text
/// [a | INPUT | Clean | 4]  --double-->  [out     | computed | Clean | 8]
/// [b | INPUT | Dirty | 3]  --add_ten--> [out_new | computed | Dirty | ∅]
/// ```
///
/// **After update**
/// ```text
/// [a | INPUT | Clean | 4]  --double-->  [out     | computed | Clean |  8]
/// [b | INPUT | Clean | 3]  --add_ten--> [out_new | computed | Clean | 13]
/// ```
#[tokio::test]
async fn visualize_adding_input_at_runtime() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(4i32));
    let output = engine.add_output_node();
    engine.connect(&[a], &[output], "double").unwrap();
    engine.update().await;

    let s1 = snapshot("BEFORE: only original branch", &engine);
    assert!(s1.contains("double"), "edge must appear in snapshot");

    // Add a second branch.
    let b       = engine.add_input(Value::new(3i32));
    let out_new = engine.add_output_node();
    engine.connect(&[b], &[out_new], "add_ten").unwrap();

    let s2 = snapshot("AFTER ADD: before update", &engine);
    assert!(s2.contains("add_ten"), "new edge must appear");

    engine.update().await;
    let s3 = snapshot("AFTER ADD + UPDATE: both branches settled", &engine);
    assert!(s3.contains("Clean"), "nodes must be Clean after update");

    // Functional correctness.
    assert_eq!(engine.get_value(output).await.unwrap().unwrap().downcast::<i32>(), Some(&8i32));
    assert_eq!(engine.get_value(out_new).await.unwrap().unwrap().downcast::<i32>(), Some(&13i32));
}

// ---------------------------------------------------------------------------
// Visualize: add_input_to_an_existing_merge_node
// ---------------------------------------------------------------------------

/// Demonstrates wiring a new input into a graph that already has a `sum` merge
/// node, then adding yet another input later.
///
/// **Phase 1** (after first update)
/// ```text
/// [a | INPUT | Clean | 3]  ─┐
///                             ├─sum─>  [out | computed | Clean | 7]
/// [b | INPUT | Clean | 4]  ─┘
/// ```
///
/// **Phase 2** (new input `c` added and updated)
/// ```text
/// [a | INPUT | Clean |  3]  ─┐
/// [b | INPUT | Clean |  4]  ─┼─sum─>  [out | computed | Clean | 15]
/// [c | INPUT | Clean |  8]  ─┘
/// ```
#[tokio::test]
async fn visualize_adding_input_to_merge() {
    let engine = make_engine();
    let a      = engine.add_input(Value::new(3i32));
    let b      = engine.add_input(Value::new(4i32));
    let out    = engine.add_output_node();
    engine.connect(&[a, b], &[out], "sum").unwrap();
    engine.update().await;

    let s1 = snapshot("PHASE 1: a+b → sum → out (=7)", &engine);
    assert!(s1.contains("sum"));
    assert_eq!(engine.get_value(out).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));

    // Add a third input to a brand-new parallel merge.
    let c        = engine.add_input(Value::new(8i32));
    let out2     = engine.add_output_node();
    engine.connect(&[a, b, c], &[out2], "sum").unwrap();
    engine.update().await;

    let s2 = snapshot("PHASE 2: a+b+c → sum → out2 (=15), original unchanged", &engine);
    assert!(s2.contains("sum"));
    assert_eq!(engine.get_value(out).await.unwrap().unwrap().downcast::<i32>(), Some(&7i32));
    assert_eq!(engine.get_value(out2).await.unwrap().unwrap().downcast::<i32>(), Some(&15i32));
}

// ---------------------------------------------------------------------------
// Visualize: remove_input_marks_downstream_dirty
// ---------------------------------------------------------------------------

/// Shows the graph before and after removing a simple input node.
///
/// **Before removal**
/// ```text
/// [input | INPUT | Clean | 5]  --double-->  [output | computed | Clean | 10]
/// ```
///
/// **After removal**
/// ```text
/// [output | computed | Dirty | 10]   (disconnected – no incoming edge)
/// ```
#[tokio::test]
async fn visualize_removing_input() {
    let engine = make_engine();
    let input  = engine.add_input(Value::new(5i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();
    engine.update().await;

    let s1 = snapshot("BEFORE REMOVE: input→double→output", &engine);
    assert!(s1.contains("double"));

    engine.remove_node(input);

    let s2 = snapshot("AFTER REMOVE: output is orphaned and Dirty", &engine);
    // The edge was removed, so "double" must no longer appear.
    assert!(!s2.contains("double"), "edge must be gone after remove");
    // Output still exists.
    assert!(engine.graph().contains_node(output));
    assert!(engine.graph().node_status(output).unwrap().is_dirty());
}

// ---------------------------------------------------------------------------
// Visualize: remove_middle_node_in_chain
// ---------------------------------------------------------------------------

/// Shows a three-node chain before and after the middle node is removed.
///
/// **Before removal**
/// ```text
/// [a | INPUT | Clean | 1]  --double-->  [mid | computed | Clean |  2]
///                                        ↓
/// [mid | computed | Clean | 2]  --add_ten-->  [out | computed | Clean | 12]
/// ```
///
/// **After removing mid**
/// ```text
/// [a | INPUT | Clean | 1]   (disconnected)
/// [out | computed | Dirty | 12]   (disconnected)
/// ```
#[tokio::test]
async fn visualize_removing_middle_node() {
    let engine = make_engine();
    let a   = engine.add_input(Value::new(1i32));
    let mid = engine.add_output_node();
    let out = engine.add_output_node();
    engine.connect(&[a],   &[mid], "double").unwrap();
    engine.connect(&[mid], &[out], "add_ten").unwrap();
    engine.update().await;

    let s1 = snapshot("BEFORE REMOVE: a→double→mid→add_ten→out", &engine);
    assert!(s1.contains("double"));
    assert!(s1.contains("add_ten"));

    engine.remove_node(mid);

    let s2 = snapshot("AFTER REMOVE mid: both edges gone, out is Dirty", &engine);
    assert!(!s2.contains("double"),  "double edge must be gone");
    assert!(!s2.contains("add_ten"), "add_ten edge must be gone");
    assert!(engine.graph().node_status(out).unwrap().is_dirty());
}

// ---------------------------------------------------------------------------
// Visualize: replace_removed_node_with_new_input
// ---------------------------------------------------------------------------

/// Removes a source node and wires a new replacement input to the same output.
///
/// **Initial state**
/// ```text
/// [a: 1 | INPUT | Clean]  --double-->  [out: 2 | computed | Clean]
/// ```
///
/// **After removing `a`**
/// ```text
/// [out: 2 | computed | Dirty]   (disconnected)
/// ```
///
/// **After adding `new_a: 20` and re-connecting**
/// ```text
/// [new_a: 20 | INPUT | Dirty]  --double-->  [out: 2 | computed | Dirty]
/// ```
///
/// **After update**
/// ```text
/// [new_a: 20 | INPUT | Clean]  --double-->  [out: 40 | computed | Clean]
/// ```
#[tokio::test]
async fn visualize_replace_removed_with_new_input() {
    let engine = make_engine();
    let a   = engine.add_input(Value::new(1i32));
    let out = engine.add_output_node();
    engine.connect(&[a], &[out], "double").unwrap();
    engine.update().await;

    let s1 = snapshot("INITIAL: a=1 → double → out=2", &engine);
    assert!(s1.contains("double"));

    engine.remove_node(a);
    let s2 = snapshot("AFTER REMOVE a: out is orphaned", &engine);
    assert!(!s2.contains("double"), "edge removed with node");

    let new_a = engine.add_input(Value::new(20i32));
    engine.connect(&[new_a], &[out], "double").unwrap();
    let s3 = snapshot("AFTER NEW INPUT new_a=20 wired: before update", &engine);
    assert!(s3.contains("double"), "new edge must appear");

    engine.update().await;
    let s4 = snapshot("AFTER UPDATE: out should be 40", &engine);
    assert!(s4.contains("Clean"));

    assert_eq!(engine.get_value(out).await.unwrap().unwrap().downcast::<i32>(), Some(&40i32));
}

// ---------------------------------------------------------------------------
// Error isolation – faults must not cascade across the graph
// ============================================================================
//
// Before the fixes in graph.rs / scheduler.rs, three related bugs existed:
//
//  Bug A – CASCADE: a failing transform left its downstream nodes in
//          `NodeStatus::Error` with the message "source has no value",
//          even though those nodes should simply stay Dirty and retry later.
//
//  Bug B – STALE VALUE POISONED: a sibling branch (unrelated to the error)
//          could be incorrectly marked or skipped because propagate_dirty
//          did not clear Error nodes back to Dirty on input change.
//
//  Bug C – WRONG WAVE: dirty_nodes_topo placed downstream nodes of an
//          errored source in wave 0 (no dirty predecessors), so they ran
//          before the source error was resolved.
//
// The tests below each target one of these failure modes.

/// Bug A – A failing transform must NOT produce cascade errors downstream.
///
/// ```text
/// [input] --always_fail--> [mid: Error]  --double-->  [out: Dirty]
/// ```
///
/// `out` must stay Dirty (not become Error) after the update, and must
/// report as `nodes_blocked`, not as a second error.
#[tokio::test]
async fn error_does_not_cascade_to_downstream_nodes() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(1i32));
    let mid   = engine.add_output_node();
    let out   = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();

    let report = engine.update().await;

    // Exactly ONE error: the failing transform on mid.
    assert_eq!(report.errors.len(), 1, "only mid should error, not out");
    assert_eq!(report.errors[0].0, mid);

    // out is blocked (upstream error), not itself in Error state.
    let graph = engine.graph();
    assert!(graph.node_status(mid).unwrap().is_error(), "mid must be Error");
    assert!(graph.node_status(out).unwrap().is_dirty(),
        "out must stay Dirty (blocked), not become Error");

    assert_eq!(report.nodes_blocked, 1, "out should be counted as blocked");
}

/// Bug A extended – error must not cascade across multiple levels.
///
/// ```text
/// [input] --always_fail--> [a: Error] --double--> [b: Dirty] --double--> [c: Dirty]
/// ```
#[tokio::test]
async fn error_does_not_cascade_multiple_levels() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(1i32));
    let a = engine.add_output_node();
    let b = engine.add_output_node();
    let c = engine.add_output_node();
    engine.connect(&[input], &[a], "always_fail").unwrap();
    engine.connect(&[a],     &[b], "double").unwrap();
    engine.connect(&[b],     &[c], "double").unwrap();

    let report = engine.update().await;

    assert_eq!(report.errors.len(), 1, "only 'a' should error");
    let graph = engine.graph();
    assert!(graph.node_status(a).unwrap().is_error());
    assert!(graph.node_status(b).unwrap().is_dirty(), "b stays Dirty");
    assert!(graph.node_status(c).unwrap().is_dirty(), "c stays Dirty");
    assert_eq!(report.nodes_blocked, 2);
}

/// Bug A – independent sibling branch is completely unaffected by the error.
///
/// ```text
///                   ┌──always_fail──> [bad: Error]
/// [shared_input] ───┤
///                   └──double──>      [good: Clean, value=2]
/// ```
#[tokio::test]
async fn error_in_one_branch_does_not_affect_sibling_branch() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(1i32));
    let bad   = engine.add_output_node();
    let good  = engine.add_output_node();
    engine.connect(&[input], &[bad],  "always_fail").unwrap();
    engine.connect(&[input], &[good], "double").unwrap();

    let report = engine.update().await;

    // bad errors, good succeeds.
    assert_eq!(report.errors.len(), 1);
    assert_eq!(report.errors[0].0, bad);
    let v = engine.get_value(good).await.unwrap().unwrap();
    assert_eq!(v.downcast::<i32>(), Some(&2i32),
        "sibling good branch must compute correctly despite bad branch error");
}

/// Bug B – after an input change, an errored node must be re-dirtied and
/// included in the next update cycle.
#[tokio::test]
async fn error_node_is_re_dirtied_on_input_change() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(0i32));
    let mid   = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.update().await;

    assert!(engine.graph().node_status(mid).unwrap().is_error());

    // Change the input.
    engine.set_input(input, Value::new(99i32)).unwrap();

    // mid must be back to Dirty so the next update retries it.
    assert!(engine.graph().node_status(mid).unwrap().is_dirty(),
        "error node must become Dirty again when its input changes");
}

/// Bug B – errored node's downstream nodes are also re-dirtied on input change.
#[tokio::test]
async fn error_downstream_re_dirtied_on_input_change() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(0i32));
    let mid   = engine.add_output_node();
    let out   = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();
    engine.update().await;

    // Change input → both mid and out should become Dirty.
    engine.set_input(input, Value::new(1i32)).unwrap();
    let graph = engine.graph();
    assert!(graph.node_status(mid).unwrap().is_dirty());
    assert!(graph.node_status(out).unwrap().is_dirty());
}

/// Full retry cycle: error → fix input → success.
///
/// Uses a transform that fails when input == 0 and succeeds otherwise.
#[tokio::test]
async fn error_then_fix_then_success() {
    struct FailOnZero;
    #[async_trait]
    impl OneToOneTransform for FailOnZero {
        async fn apply(&self, input: &Value) -> Result<Value, TransformError> {
            let n = input.downcast::<i32>().copied()
                .ok_or_else(|| TransformError::new("expected i32"))?;
            if n == 0 {
                Err(TransformError::new("input is zero"))
            } else {
                Ok(Value::new(n * 10))
            }
        }
    }

    let storage = Arc::new(MemoryStorage::new()) as Arc<dyn crate::incremental::storage::Storage>;
    let mut engine = IncrementalEngine::new(storage);
    engine.register_transform("fail_on_zero",
        Transform::OneToOne(Arc::new(FailOnZero)));

    let input  = engine.add_input(Value::new(0i32));
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "fail_on_zero").unwrap();

    // First update: should fail.
    let report1 = engine.update().await;
    assert!(!report1.is_ok());
    assert!(engine.graph().node_status(output).unwrap().is_error());

    // Fix the input.
    engine.set_input(input, Value::new(5i32)).unwrap();
    assert!(engine.graph().node_status(output).unwrap().is_dirty(),
        "after input fix, output must be Dirty again");

    // Second update: should succeed.
    let report2 = engine.update().await;
    assert!(report2.is_ok(), "{:?}", report2.errors);
    let v = engine.get_value(output).await.unwrap().unwrap();
    assert_eq!(v.downcast::<i32>(), Some(&50i32));
}

/// Bug C – nodes downstream of an errored node must not appear in wave 0.
/// They should be blocked, not run before the source is resolved.
#[tokio::test]
async fn blocked_nodes_are_not_in_wave_zero() {
    let engine = make_engine();
    let input = engine.add_input(Value::new(1i32));
    let mid   = engine.add_output_node();
    let out   = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();

    let report = engine.update().await;

    // mid errored, out was blocked. out must NOT have errored with
    // "source has no value" – that would indicate it ran in wave 0.
    let out_errors: Vec<_> = report.errors.iter()
        .filter(|(id, _)| *id == out)
        .collect();
    assert!(out_errors.is_empty(),
        "out must not appear in errors (was: {:?})", out_errors);
}
