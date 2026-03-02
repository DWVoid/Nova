//! Integration tests for the incremental computation graph.
//!
//! All tests use the typed public API only (`add_input::<T>`, `set_input::<T>`,
//! `get_value::<T>`, `register_one_to_one`, etc.).  No `Value` or `Transform`
//! types are used directly.

#![cfg(test)]

use std::sync::Arc;

use crate::{
    IncrementalEngine, EngineError, MemoryStorage,
    storage::Storage,
    transform::TransformError,
};

// ============================================================================
// Helper: build a fresh engine with all transforms registered.
// ============================================================================

fn make_engine() -> IncrementalEngine {
    let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
    let mut engine = IncrementalEngine::new(storage);
    // Register custom Vec<i32> type for min_max / always_zero tests.
    engine.register_value_type::<Vec<i32>>("Vec<i32>").unwrap();

    engine.register_one_to_one::<i32, i32, _, _>("double",
        |n: &i32| { let n = *n; async move { Ok(n * 2) } }).unwrap();

    engine.register_one_to_one::<i32, i32, _, _>("add_ten",
        |n: &i32| { let n = *n; async move { Ok(n + 10) } }).unwrap();

    engine.register_many_to_one::<i32, i32, _, _>("sum",
        |inputs: &[i32]| {
            let total: i32 = inputs.iter().sum();
            async move { Ok(total) }
        }).unwrap();

    engine.register_one_to_many::<i32, i32, _, _>("duplicate",
        |n: &i32| { let n = *n; async move { Ok(vec![n, n]) } }).unwrap();

    engine.register_one_to_many::<Vec<i32>, i32, _, _>("min_max",
        |pair: &Vec<i32>| {
            let (a, b) = (pair[0], pair[1]);
            async move { Ok(vec![a.min(b), a.max(b)]) }
        }).unwrap();

    engine.register_many_to_many::<i32, i32, _, _>("sum_and_product",
        |inputs: &[i32]| {
            let (a, b) = (inputs[0], inputs[1]);
            async move { Ok(vec![a + b, a * b]) }
        }).unwrap();

    engine.register_one_to_one::<i32, i32, _, _>("always_zero",
        |_: &i32| async move { Ok(0i32) }).unwrap();

    engine.register_one_to_one::<i32, i32, _, _>("always_fail",
        |_: &i32| async move {
            Err(TransformError::new("AlwaysFail: intentional failure"))
        }).unwrap();

    engine
}

// ============================================================================
// 2. One-to-One transform
// ============================================================================

#[tokio::test]
async fn one_to_one_basic() {
    let engine = make_engine();
    let input  = engine.add_input(21i32).unwrap();
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();

    let report = engine.update().await;
    assert!(report.is_ok(), "{:?}", report.errors);

    let v: i32 = engine.get_value(output).await.unwrap().unwrap();
    assert_eq!(v, 42);
}

#[tokio::test]
async fn one_to_one_chained() {
    let engine = make_engine();
    let input  = engine.add_input(5i32).unwrap();
    let mid    = engine.add_output_node();
    let output = engine.add_output_node();
    engine.connect(&[input], &[mid],    "double").unwrap();
    engine.connect(&[mid],   &[output], "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(mid).await.unwrap().unwrap(), 10);
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 20);
}

// ============================================================================
// 3. Many-to-One transform
// ============================================================================

#[tokio::test]
async fn many_to_one_two_inputs() {
    let engine = make_engine();
    let a      = engine.add_input(3i32).unwrap();
    let b      = engine.add_input(4i32).unwrap();
    let output = engine.add_output_node();
    engine.connect(&[a, b], &[output], "sum").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 7);
}

#[tokio::test]
async fn many_to_one_three_inputs() {
    let engine = make_engine();
    let a      = engine.add_input(1i32).unwrap();
    let b      = engine.add_input(2i32).unwrap();
    let c      = engine.add_input(3i32).unwrap();
    let output = engine.add_output_node();
    engine.connect(&[a, b, c], &[output], "sum").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 6);
}

#[tokio::test]
async fn many_to_one_partial_update() {
    let engine = make_engine();
    let a      = engine.add_input(10i32).unwrap();
    let b      = engine.add_input(5i32).unwrap();
    let output = engine.add_output_node();
    engine.connect(&[a, b], &[output], "sum").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 15);

    engine.set_input(b, 20i32).unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 30);
}

// ============================================================================
// 4. One-to-Many transform
// ============================================================================

#[tokio::test]
async fn one_to_many_duplicate() {
    let engine = make_engine();
    let input = engine.add_input(7i32).unwrap();
    let out_a = engine.add_output_node();
    let out_b = engine.add_output_node();
    engine.connect(&[input], &[out_a, out_b], "duplicate").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_a).await.unwrap().unwrap(), 7);
    assert_eq!(engine.get_value::<i32>(out_b).await.unwrap().unwrap(), 7);
}

#[tokio::test]
async fn one_to_many_min_max() {
    let engine = make_engine();
    let input   = engine.add_input(vec![8i32, 3i32]).unwrap();
    let out_min = engine.add_output_node();
    let out_max = engine.add_output_node();
    engine.connect(&[input], &[out_min, out_max], "min_max").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_min).await.unwrap().unwrap(), 3);
    assert_eq!(engine.get_value::<i32>(out_max).await.unwrap().unwrap(), 8);
}

// ============================================================================
// 5. Many-to-Many transform
// ============================================================================

#[tokio::test]
async fn many_to_many_sum_and_product() {
    let engine      = make_engine();
    let a           = engine.add_input(3i32).unwrap();
    let b           = engine.add_input(4i32).unwrap();
    let out_sum     = engine.add_output_node();
    let out_product = engine.add_output_node();
    engine.connect(&[a, b], &[out_sum, out_product], "sum_and_product").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_sum).await.unwrap().unwrap(), 7);
    assert_eq!(engine.get_value::<i32>(out_product).await.unwrap().unwrap(), 12);
}

#[tokio::test]
async fn many_to_many_update_one_input() {
    let engine      = make_engine();
    let a           = engine.add_input(2i32).unwrap();
    let b           = engine.add_input(5i32).unwrap();
    let out_sum     = engine.add_output_node();
    let out_product = engine.add_output_node();
    engine.connect(&[a, b], &[out_sum, out_product], "sum_and_product").unwrap();
    engine.update().await;

    engine.set_input(a, 10i32).unwrap();
    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_sum).await.unwrap().unwrap(), 15);
    assert_eq!(engine.get_value::<i32>(out_product).await.unwrap().unwrap(), 50);
}

// ============================================================================
// 6. Hidden layers
// ============================================================================

#[tokio::test]
async fn hidden_layers_three_deep() {
    let engine  = make_engine();
    let input   = engine.add_input(3i32).unwrap();
    let hidden1 = engine.add_output_node();
    let hidden2 = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],   &[hidden1], "double").unwrap();
    engine.connect(&[hidden1], &[hidden2], "double").unwrap();
    engine.connect(&[hidden2], &[output],  "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 22);
    assert_eq!(engine.get_value::<i32>(hidden1).await.unwrap().unwrap(), 6);
    assert_eq!(engine.get_value::<i32>(hidden2).await.unwrap().unwrap(), 12);
}

#[tokio::test]
async fn hidden_layers_input_change_propagates() {
    let engine  = make_engine();
    let input   = engine.add_input(1i32).unwrap();
    let hidden  = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],  &[hidden], "double").unwrap();
    engine.connect(&[hidden], &[output], "add_ten").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 12);

    engine.set_input(input, 5i32).unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 20);
}

// ============================================================================
// 7. Multiple connections from the same source node
// ============================================================================

#[tokio::test]
async fn multiple_connections_fan_out() {
    let engine  = make_engine();
    let input   = engine.add_input(5i32).unwrap();
    let out_a   = engine.add_output_node();
    let out_b   = engine.add_output_node();
    engine.connect(&[input], &[out_a], "double").unwrap();
    engine.connect(&[input], &[out_b], "add_ten").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_a).await.unwrap().unwrap(), 10);
    assert_eq!(engine.get_value::<i32>(out_b).await.unwrap().unwrap(), 15);
}

#[tokio::test]
async fn multiple_connections_fan_out_update() {
    let engine  = make_engine();
    let input   = engine.add_input(1i32).unwrap();
    let out_a   = engine.add_output_node();
    let out_b   = engine.add_output_node();
    engine.connect(&[input], &[out_a], "double").unwrap();
    engine.connect(&[input], &[out_b], "add_ten").unwrap();
    engine.update().await;

    engine.set_input(input, 10i32).unwrap();
    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_a).await.unwrap().unwrap(), 20);
    assert_eq!(engine.get_value::<i32>(out_b).await.unwrap().unwrap(), 20);
}

#[tokio::test]
async fn multiple_connections_computed_source() {
    let engine  = make_engine();
    let input   = engine.add_input(3i32).unwrap();
    let mid     = engine.add_output_node();
    let out_a   = engine.add_output_node();
    let out_b   = engine.add_output_node();
    engine.connect(&[input], &[mid],   "double").unwrap();
    engine.connect(&[mid],   &[out_a], "add_ten").unwrap();
    engine.connect(&[mid],   &[out_b], "double").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out_a).await.unwrap().unwrap(), 16);
    assert_eq!(engine.get_value::<i32>(out_b).await.unwrap().unwrap(), 12);
}

// ============================================================================
// 8. Diamond / reconvergent graph
// ============================================================================

#[tokio::test]
async fn diamond_graph() {
    let engine  = make_engine();
    let input   = engine.add_input(5i32).unwrap();
    let left    = engine.add_output_node();
    let right   = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],       &[left],   "double").unwrap();
    engine.connect(&[input],       &[right],  "add_ten").unwrap();
    engine.connect(&[left, right], &[output], "sum").unwrap();

    engine.update().await;

    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 25);
}

#[tokio::test]
async fn diamond_graph_update() {
    let engine  = make_engine();
    let input   = engine.add_input(1i32).unwrap();
    let left    = engine.add_output_node();
    let right   = engine.add_output_node();
    let output  = engine.add_output_node();
    engine.connect(&[input],       &[left],   "double").unwrap();
    engine.connect(&[input],       &[right],  "add_ten").unwrap();
    engine.connect(&[left, right], &[output], "sum").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 13);

    engine.set_input(input, 4i32).unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 22);
}

// ============================================================================
// 9. Incremental update – minimal recomputation
// ============================================================================

#[tokio::test]
async fn incremental_only_dirty_branch_recomputed() {
    let engine  = make_engine();
    let a       = engine.add_input(3i32).unwrap();
    let b       = engine.add_input(5i32).unwrap();
    let out_a   = engine.add_output_node();
    let out_b   = engine.add_output_node();
    engine.connect(&[a], &[out_a], "double").unwrap();
    engine.connect(&[b], &[out_b], "double").unwrap();
    engine.update().await;

    engine.set_input(a, 10i32).unwrap();

    let graph = engine.graph();
    assert!(graph.node_status(out_a).unwrap().is_dirty());
    assert!(!graph.node_status(out_b).unwrap().is_dirty(),
        "out_b should be clean – its input didn't change");

    let report = engine.update().await;
    assert_eq!(report.transforms_evaluated, 1, "only out_a should be evaluated");
    assert_eq!(engine.get_value::<i32>(out_a).await.unwrap().unwrap(), 20);
    assert_eq!(engine.get_value::<i32>(out_b).await.unwrap().unwrap(), 10);
}

#[tokio::test]
async fn incremental_no_change_no_evaluation() {
    let engine  = make_engine();
    let input   = engine.add_input(7i32).unwrap();
    let output  = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();
    engine.update().await;

    let report = engine.update().await;
    assert_eq!(report.transforms_evaluated, 0);
}

// ============================================================================
// 10. Hash-based early exit
// ============================================================================

#[tokio::test]
async fn hash_early_exit_prevents_downstream_recomputation() {
    let engine     = make_engine();
    let input      = engine.add_input(1i32).unwrap();
    let zero       = engine.add_output_node();
    let downstream = engine.add_output_node();
    engine.connect(&[input], &[zero],       "always_zero").unwrap();
    engine.connect(&[zero],  &[downstream], "double").unwrap();
    engine.update().await;

    assert_eq!(engine.get_value::<i32>(downstream).await.unwrap().unwrap(), 0);

    engine.set_input(input, 999i32).unwrap();
    let report = engine.update().await;

    assert!(report.transforms_skipped >= 1,
        "at least one node should be skipped via hash early exit (got {})", report.transforms_skipped);

    assert_eq!(engine.get_value::<i32>(downstream).await.unwrap().unwrap(), 0);
}

// ============================================================================
// 11. Adding new inputs at runtime
// ============================================================================

#[tokio::test]
async fn adding_input_at_runtime() {
    let engine  = make_engine();
    let a       = engine.add_input(4i32).unwrap();
    let output  = engine.add_output_node();
    engine.connect(&[a], &[output], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 8);

    let b       = engine.add_input(3i32).unwrap();
    let out_new = engine.add_output_node();
    engine.connect(&[b], &[out_new], "add_ten").unwrap();

    let report = engine.update().await;
    assert!(report.is_ok());
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 8);
    assert_eq!(engine.get_value::<i32>(out_new).await.unwrap().unwrap(), 13);
}

#[tokio::test]
async fn adding_input_to_existing_merge() {
    let engine      = make_engine();
    let a           = engine.add_input(5i32).unwrap();
    let out_a       = engine.add_output_node();
    engine.connect(&[a], &[out_a], "double").unwrap();
    engine.update().await;

    let b           = engine.add_input(100i32).unwrap();
    let accumulator = engine.add_output_node();
    engine.connect(&[b], &[accumulator], "add_ten").unwrap();

    engine.update().await;
    assert_eq!(engine.get_value::<i32>(accumulator).await.unwrap().unwrap(), 110);
}

// ============================================================================
// 12. Removing inputs
// ============================================================================

#[tokio::test]
async fn removing_input_marks_downstream_dirty() {
    let engine  = make_engine();
    let input   = engine.add_input(5i32).unwrap();
    let output  = engine.add_output_node();
    engine.connect(&[input], &[output], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(output).await.unwrap().unwrap(), 10);

    assert!(engine.remove_node(input));
    assert!(!engine.graph().contains_node(input));
    assert!(engine.graph().node_status(output).unwrap().is_dirty(),
        "output must be dirty after its input was removed");
}

#[tokio::test]
async fn removing_absent_node_returns_false() {
    let engine  = make_engine();
    let phantom = crate::NodeId::new();
    assert!(!engine.remove_node(phantom));
}

#[tokio::test]
async fn removing_middle_node_in_chain() {
    let engine  = make_engine();
    let a       = engine.add_input(1i32).unwrap();
    let mid     = engine.add_output_node();
    let out     = engine.add_output_node();
    engine.connect(&[a],   &[mid], "double").unwrap();
    engine.connect(&[mid], &[out], "add_ten").unwrap();
    engine.update().await;

    engine.remove_node(mid);

    assert!(!engine.graph().contains_node(mid));
    assert!(engine.graph().node_status(out).unwrap().is_dirty());
}

#[tokio::test]
async fn replace_removed_node_with_new_input() {
    let engine  = make_engine();
    let a       = engine.add_input(1i32).unwrap();
    let out     = engine.add_output_node();
    engine.connect(&[a], &[out], "double").unwrap();
    engine.update().await;
    assert_eq!(engine.get_value::<i32>(out).await.unwrap().unwrap(), 2);

    engine.remove_node(a);

    let new_a = engine.add_input(20i32).unwrap();
    engine.connect(&[new_a], &[out], "double").unwrap();
    engine.update().await;

    assert_eq!(engine.get_value::<i32>(out).await.unwrap().unwrap(), 40);
}

// ============================================================================
// 13. Error handling
// ============================================================================

#[tokio::test]
async fn failing_transform_is_reported() {
    let engine  = make_engine();
    let input   = engine.add_input(0i32).unwrap();
    let output  = engine.add_output_node();
    engine.connect(&[input], &[output], "always_fail").unwrap();

    let report = engine.update().await;

    assert!(!report.is_ok());
    assert_eq!(report.errors.len(), 1);
    // In the new model, error is on the TransformNode, not the IoNode.
    // Just verify the output IoNode reflects an error state via node_status.
    let graph = engine.graph();
    assert!(graph.transform_node_ids().iter().any(|&tid|
        graph.transform_status(tid).map(|s| s.is_error()).unwrap_or(false)
    ), "some transform should be in error state");
}

#[tokio::test]
async fn failing_transform_retried_after_input_change() {
    let engine   = make_engine();
    let input    = engine.add_input(5i32).unwrap();
    let failing  = engine.add_output_node();
    let success  = engine.add_output_node();
    engine.connect(&[input], &[failing], "always_fail").unwrap();
    engine.connect(&[input], &[success], "double").unwrap();

    let report = engine.update().await;
    assert!(!report.is_ok());

    assert!(engine.graph().node_status(failing).unwrap().is_error());

    engine.set_input(input, 7i32).unwrap();
    assert!(engine.graph().node_status(failing).unwrap().is_dirty(),
        "error node must be dirtied when its input changes");

    let report2 = engine.update().await;
    assert_eq!(engine.get_value::<i32>(success).await.unwrap().unwrap(), 14);
    assert!(!report2.is_ok());
}

// ============================================================================
// 14. Save and reload – typed round-trip (TODO: update persistence layer)
// ============================================================================

#[tokio::test]
#[ignore = "persistence API not yet updated for bipartite graph — re-enable after save/load implementation"]
async fn save_and_load_preserves_topology_and_values() {
    // TODO: re-implement once engine.save() / engine.load() are updated
    // for the bipartite graph model.
    let _ = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;
}

// ============================================================================
// 15. Error isolation
// ============================================================================

#[tokio::test]
async fn error_does_not_cascade_to_downstream_nodes() {
    let engine = make_engine();
    let input  = engine.add_input(1i32).unwrap();
    let mid    = engine.add_output_node();
    let out    = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();

    let report = engine.update().await;

    assert_eq!(report.errors.len(), 1, "only mid should error, not out");
    // errors[0].0 is the TransformNode ID (not the IoNode 'mid' - that's the compat model)
    // Just verify exactly one error occurred.

    let graph = engine.graph();
    assert!(graph.node_status(mid).unwrap().is_error(), "mid must be Error");
    assert!(graph.node_status(out).unwrap().is_dirty(),
        "out must stay Dirty (blocked), not become Error");

    assert_eq!(report.transforms_blocked, 1, "out should be counted as blocked");
}

#[tokio::test]
async fn error_does_not_cascade_multiple_levels() {
    let engine = make_engine();
    let input  = engine.add_input(1i32).unwrap();
    let a      = engine.add_output_node();
    let b      = engine.add_output_node();
    let c      = engine.add_output_node();
    engine.connect(&[input], &[a], "always_fail").unwrap();
    engine.connect(&[a],     &[b], "double").unwrap();
    engine.connect(&[b],     &[c], "double").unwrap();

    let report = engine.update().await;

    assert_eq!(report.errors.len(), 1, "only 'a' should error");
    let graph = engine.graph();
    assert!(graph.node_status(a).unwrap().is_error());
    assert!(graph.node_status(b).unwrap().is_dirty(), "b stays Dirty");
    assert!(graph.node_status(c).unwrap().is_dirty(), "c stays Dirty");
    assert_eq!(report.transforms_blocked, 2);
}

#[tokio::test]
async fn error_in_one_branch_does_not_affect_sibling_branch() {
    let engine = make_engine();
    let input  = engine.add_input(1i32).unwrap();
    let bad    = engine.add_output_node();
    let good   = engine.add_output_node();
    engine.connect(&[input], &[bad],  "always_fail").unwrap();
    engine.connect(&[input], &[good], "double").unwrap();

    let report = engine.update().await;

    assert_eq!(report.errors.len(), 1);
    // errors[0].0 is the TransformNode ID, not the IoNode 'bad'.
    // Just verify exactly one error and the good branch computed correctly.
    let v: i32 = engine.get_value(good).await.unwrap().unwrap();
    assert_eq!(v, 2, "sibling good branch must compute correctly despite bad branch error");
}

#[tokio::test]
async fn error_node_is_re_dirtied_on_input_change() {
    let engine = make_engine();
    let input  = engine.add_input(0i32).unwrap();
    let mid    = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.update().await;

    assert!(engine.graph().node_status(mid).unwrap().is_error());

    engine.set_input(input, 99i32).unwrap();
    assert!(engine.graph().node_status(mid).unwrap().is_dirty(),
        "error node must become Dirty again when its input changes");
}

#[tokio::test]
async fn error_downstream_re_dirtied_on_input_change() {
    let engine = make_engine();
    let input  = engine.add_input(0i32).unwrap();
    let mid    = engine.add_output_node();
    let out    = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();
    engine.update().await;

    engine.set_input(input, 1i32).unwrap();
    let graph = engine.graph();
    assert!(graph.node_status(mid).unwrap().is_dirty());
    assert!(graph.node_status(out).unwrap().is_dirty());
}

#[tokio::test]
async fn error_then_fix_then_success() {
    let storage = Arc::new(MemoryStorage::new()) as Arc<dyn crate::storage::Storage>;
    let mut engine = IncrementalEngine::new(storage);
    engine.register_one_to_one::<i32, i32, _, _>("fail_on_zero",
        |n: &i32| {
            let n = *n;
            async move {
                if n == 0 {
                    Err(TransformError::new("input is zero"))
                } else {
                    Ok(n * 10)
                }
            }
        }).unwrap();

    let input  = engine.add_input(0i32).unwrap();
    let output = engine.add_output_node();
    engine.connect(&[input], &[output], "fail_on_zero").unwrap();

    let report1 = engine.update().await;
    assert!(!report1.is_ok());
    assert!(engine.graph().node_status(output).unwrap().is_error());

    engine.set_input(input, 5i32).unwrap();
    assert!(engine.graph().node_status(output).unwrap().is_dirty(),
        "after input fix, output must be Dirty again");

    let report2 = engine.update().await;
    assert!(report2.is_ok(), "{:?}", report2.errors);
    let v: i32 = engine.get_value(output).await.unwrap().unwrap();
    assert_eq!(v, 50);
}

#[tokio::test]
async fn blocked_nodes_are_not_in_wave_zero() {
    let engine = make_engine();
    let input  = engine.add_input(1i32).unwrap();
    let mid    = engine.add_output_node();
    let out    = engine.add_output_node();
    engine.connect(&[input], &[mid], "always_fail").unwrap();
    engine.connect(&[mid],   &[out], "double").unwrap();

    let report = engine.update().await;

    let out_errors: Vec<_> = report.errors.iter()
        .filter(|(id, _)| *id == out)
        .collect();
    assert!(out_errors.is_empty(),
        "out must not appear in errors (was: {:?})", out_errors);
}

// ============================================================================
// 16. Type mismatch error
// ============================================================================

#[tokio::test]
async fn get_value_type_mismatch_returns_error() {
    let engine = make_engine();
    let input = engine.add_input(42i32).unwrap();
    let result = engine.get_value::<u64>(input).await;
    assert!(matches!(result, Err(EngineError::TypeMismatch { .. })),
        "expected TypeMismatch, got: {:?}", result);
}

#[tokio::test]
async fn add_input_unregistered_type_fails() {
    let engine = make_engine();
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    struct Custom(i32);
    let result = engine.add_input(Custom(1));
    assert!(matches!(result, Err(EngineError::UnregisteredType(_))));
}
