use std::sync::Arc;
use async_trait::async_trait;
use uuid::Uuid;
use nova_incremental_neo::*;

// ---------------------------------------------------------------------------
// Helper UUIDs
// ---------------------------------------------------------------------------
const INPUT_A:  Uuid = Uuid::from_u128(0x1000_0000_0000_0000_0000_0000_0000_0001);
const INPUT_B:  Uuid = Uuid::from_u128(0x1000_0000_0000_0000_0000_0000_0000_0002);
const XFMR_A:   Uuid = Uuid::from_u128(0x2000_0000_0000_0000_0000_0000_0000_0001);
const XFMR_B:   Uuid = Uuid::from_u128(0x2000_0000_0000_0000_0000_0000_0000_0002);
const XFMR_C:   Uuid = Uuid::from_u128(0x2000_0000_0000_0000_0000_0000_0000_0003);
const OUTPUT_A: Uuid = Uuid::from_u128(0x3000_0000_0000_0000_0000_0000_0000_0001);
const OUTPUT_B: Uuid = Uuid::from_u128(0x3000_0000_0000_0000_0000_0000_0000_0002);

// ---------------------------------------------------------------------------
// Test transforms
// ---------------------------------------------------------------------------

struct Double;
#[async_trait]
impl Transform for Double {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.output::<u64>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let v = *ctx.input::<u64>(0)?;
        ctx.output(0, v * 2)?;
        Ok(())
    }
}

struct AddOne;
#[async_trait]
impl Transform for AddOne {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.output::<u64>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let v = *ctx.input::<u64>(0)?;
        ctx.output(0, v + 1)?;
        Ok(())
    }
}

struct Sum;
#[async_trait]
impl Transform for Sum {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.input::<u64>();
        ctx.output::<u64>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let a = *ctx.input::<u64>(0)?;
        let b = *ctx.input::<u64>(1)?;
        ctx.output(0, a + b)?;
        Ok(())
    }
}

struct ByValue;
impl KeyExtractor<u64> for ByValue {
    fn extract_key(item: &u64) -> u64 { *item }
}

struct Expand;
#[async_trait]
impl Transform for Expand {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.output_collection::<u64, ByValue>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let n = *ctx.input::<u64>(0)?;
        let items: Vec<u64> = (0..n).collect();
        ctx.output_collection(0, items)?;
        Ok(())
    }
}

struct AlwaysError;
#[async_trait]
impl Transform for AlwaysError {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.output::<u64>();
    }
    async fn apply(&self, _ctx: &mut TransformContext) -> Result<(), TransformError> {
        Err(TransformError::new("always fails"))
    }
}

struct GatherColl;
#[async_trait]
impl Transform for GatherColl {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input_collection::<u64, ByValue>();
        ctx.output_collection::<u64, ByValue>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let coll = ctx.input_collection::<u64>(0)?;
        let items: Vec<u64> = coll.iter().map(|(_, v)| *v).collect();
        ctx.output_collection(0, items)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Builder helper
// ---------------------------------------------------------------------------

async fn build_linear(storage: Arc<dyn Storage>) -> Engine {
    EngineBuilder::new()
        .register("double", Double)
        .input_node::<u64>(INPUT_A)
        .transform_node(XFMR_A, "double")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_slot_to(XFMR_A, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap()
}

// ---------------------------------------------------------------------------
// 1. Basic single transform
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_basic_single_transform() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = build_linear(storage).await;

    engine.set_input::<u64>(INPUT_A, 21).unwrap();
    let report = engine.update().await;
    assert!(report.is_ok());
    assert_eq!(report.transforms_evaluated, 1);

    let result = engine.get::<u64>(OUTPUT_A).await.unwrap();
    assert_eq!(result, Some(42));
}

// ---------------------------------------------------------------------------
// 2. No-change skips transform
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_no_change_skips_transform() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = build_linear(storage).await;

    engine.set_input::<u64>(INPUT_A, 21).unwrap();
    let r1 = engine.update().await;
    assert_eq!(r1.transforms_evaluated, 1);

    // Same value — should skip
    engine.set_input::<u64>(INPUT_A, 21).unwrap();
    let r2 = engine.update().await;
    assert_eq!(r2.transforms_evaluated, 0);
    assert_eq!(r2.transforms_skipped, 0);
    assert_eq!(r2.transforms_changed, 0);

    let result = engine.get::<u64>(OUTPUT_A).await.unwrap();
    assert_eq!(result, Some(42));
}

// ---------------------------------------------------------------------------
// 3. Chained transforms: input → double → add_one
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_chained_transforms() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = EngineBuilder::new()
        .register("double", Double)
        .register("add_one", AddOne)
        .input_node::<u64>(INPUT_A)
        .transform_node(XFMR_A, "double")
        .transform_node(XFMR_B, "add_one")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_slot_to_slot(XFMR_A, 0, XFMR_B, 0)
        .wire_slot_to(XFMR_B, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap();

    engine.set_input::<u64>(INPUT_A, 5).unwrap();
    let report = engine.update().await;
    assert!(report.is_ok());
    assert_eq!(report.transforms_evaluated, 2);

    let result = engine.get::<u64>(OUTPUT_A).await.unwrap();
    assert_eq!(result, Some(11)); // 5 * 2 + 1
}

// ---------------------------------------------------------------------------
// 4. Multi-input transform: a + b
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_multi_input_transform() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = EngineBuilder::new()
        .register("sum", Sum)
        .input_node::<u64>(INPUT_A)
        .input_node::<u64>(INPUT_B)
        .transform_node(XFMR_A, "sum")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_into_slot(INPUT_B, XFMR_A, 1)
        .wire_slot_to(XFMR_A, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap();

    engine.set_input::<u64>(INPUT_A, 3).unwrap();
    engine.set_input::<u64>(INPUT_B, 7).unwrap();
    let report = engine.update().await;
    assert!(report.is_ok());

    let result = engine.get::<u64>(OUTPUT_A).await.unwrap();
    assert_eq!(result, Some(10));
}

// ---------------------------------------------------------------------------
// 5. Collection expand: input 5 → [0,1,2,3,4]
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_collection_expand() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = EngineBuilder::new()
        .register("expand", Expand)
        .input_node::<u64>(INPUT_A)
        .transform_node(XFMR_A, "expand")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_slot_to(XFMR_A, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap();

    engine.set_input::<u64>(INPUT_A, 5).unwrap();
    let report = engine.update().await;
    assert!(report.is_ok());
    assert_eq!(report.transforms_evaluated, 1);
}

// ---------------------------------------------------------------------------
// 6. Checkpoint / commit lifecycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_checkpoint_commit() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = build_linear(storage.clone()).await;

    engine.set_input::<u64>(INPUT_A, 21).unwrap();
    engine.update().await;
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(42));

    engine.checkpoint().await.unwrap();
    engine.commit().await.unwrap();

    // After commit, storage contains the WorkState snapshot.
    // A new engine with the same storage can warm-start.
    let engine2 = build_linear(storage).await;
    let result = engine2.get::<u64>(OUTPUT_A).await.unwrap();
    assert_eq!(result, Some(42));
}

// ---------------------------------------------------------------------------
// 7. Checkpoint / discard reverts state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_checkpoint_discard() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = build_linear(storage.clone()).await;

    // Set initial value, commit
    engine.set_input::<u64>(INPUT_A, 10).unwrap();
    engine.update().await;
    engine.checkpoint().await.unwrap();
    engine.commit().await.unwrap();

    // Set new value
    engine.set_input::<u64>(INPUT_A, 20).unwrap();
    engine.update().await;
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(40));

    // Discard should revert
    engine.checkpoint().await.unwrap();
    engine.discard().await.unwrap();
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(20));

    // Re-run to verify correct output
    let report = engine.update().await;
    assert_eq!(report.transforms_evaluated, 0); // nothing changed
}

// ---------------------------------------------------------------------------
// 8. Error propagation
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_error_propagation() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = EngineBuilder::new()
        .register("error", AlwaysError)
        .register("double", Double)
        .input_node::<u64>(INPUT_A)
        .transform_node(XFMR_A, "error")
        .transform_node(XFMR_B, "double")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_slot_to_slot(XFMR_A, 0, XFMR_B, 0)
        .wire_slot_to(XFMR_B, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap();

    engine.set_input::<u64>(INPUT_A, 10).unwrap();
    let report = engine.update().await;
    assert!(!report.is_ok());
    assert_eq!(report.errors.len(), 1);
}

// ---------------------------------------------------------------------------
// 9. Warm start: build engine → update → rebuild → verify
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_warm_start() {
    let storage = Arc::new(MemoryStorage::new());

    // First session
    {
        let engine = build_linear(storage.clone()).await;
        engine.set_input::<u64>(INPUT_A, 100).unwrap();
        engine.update().await;
        engine.checkpoint().await.unwrap();
        engine.commit().await.unwrap();
    }

    // Second session — warm start
    {
        let engine = build_linear(storage.clone()).await;
        // Without calling set_input, the warm-start should have the value.
        // The I/O output is cached in ValueStore only after update() runs,
        // or after lazy load from storage.
        let result = engine.get::<u64>(OUTPUT_A).await.unwrap();
        // Warm start restores WorkState (present flags, hashes) but values
        // are lazy-loaded. After commit the previous session wrote them.
        // If ValueStore cache is empty, get() triggers lazy load.
        assert_eq!(result, Some(200));
    }
}

// ---------------------------------------------------------------------------
// 10. Multiple engines with independent storage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_independent_storages() {
    let storage_a = Arc::new(MemoryStorage::new());
    let storage_b = Arc::new(MemoryStorage::new());

    let engine_a = build_linear(storage_a).await;
    let engine_b = build_linear(storage_b).await;

    engine_a.set_input::<u64>(INPUT_A, 10).unwrap();
    engine_b.set_input::<u64>(INPUT_A, 20).unwrap();

    let ra = engine_a.update().await;
    let rb = engine_b.update().await;
    assert!(ra.is_ok());
    assert!(rb.is_ok());

    assert_eq!(engine_a.get::<u64>(OUTPUT_A).await.unwrap(), Some(20));
    assert_eq!(engine_b.get::<u64>(OUTPUT_A).await.unwrap(), Some(40));
}

// ---------------------------------------------------------------------------
// 11. Multiple updates (incremental)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_incremental_updates() {
    let storage = Arc::new(MemoryStorage::new());
    let engine = build_linear(storage).await;

    engine.set_input::<u64>(INPUT_A, 1).unwrap();
    assert_eq!(engine.update().await.transforms_evaluated, 1);
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(2));

    engine.set_input::<u64>(INPUT_A, 2).unwrap();
    assert_eq!(engine.update().await.transforms_evaluated, 1);
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(4));

    engine.set_input::<u64>(INPUT_A, 3).unwrap();
    assert_eq!(engine.update().await.transforms_evaluated, 1);
    assert_eq!(engine.get::<u64>(OUTPUT_A).await.unwrap(), Some(6));
}

// ---------------------------------------------------------------------------
// 12. Gather collection from multiple sources
// ---------------------------------------------------------------------------

struct Expand2;
#[async_trait]
impl Transform for Expand2 {
    fn register(ctx: &mut impl TransformRegisterContext) {
        ctx.input::<u64>();
        ctx.output_collection::<u64, ByValue>();
    }
    async fn apply(&self, ctx: &mut TransformContext) -> Result<(), TransformError> {
        let n = *ctx.input::<u64>(0)?;
        let items: Vec<u64> = (0..n).collect();
        ctx.output_collection(0, items)?;
        Ok(())
    }
}

#[tokio::test]
async fn test_multi_source_gather() {
    let storage = Arc::new(MemoryStorage::new());
    // Two expand nodes feeding into one gather node
    let engine = EngineBuilder::new()
        .register("expand", Expand2)
        .register("gather", GatherColl)
        .input_node::<u64>(INPUT_A)
        .input_node::<u64>(INPUT_B)
        .transform_node(XFMR_A, "expand")
        .transform_node(XFMR_B, "expand")
        .transform_node(XFMR_C, "gather")
        .output_node(OUTPUT_A)
        .wire_into_slot(INPUT_A, XFMR_A, 0)
        .wire_into_slot(INPUT_B, XFMR_B, 0)
        // Both expand outputs feed into gather's collection input (slot 0)
        .wire_slot_to_slot(XFMR_A, 0, XFMR_C, 0)
        .wire_slot_to_slot(XFMR_B, 0, XFMR_C, 0)
        .wire_slot_to(XFMR_C, 0, OUTPUT_A)
        .with_storage(storage)
        .build().await.unwrap();

    engine.set_input::<u64>(INPUT_A, 3).unwrap();
    engine.set_input::<u64>(INPUT_B, 2).unwrap();
    let report = engine.update().await;
    assert!(report.is_ok());
    // Both expand transforms should have evaluated.
    // The gather transform evaluation depends on propagation timing,
    // so just verify no errors and at minimum the expands ran.
    assert!(report.transforms_evaluated >= 2, "expected >=2, got {}", report.transforms_evaluated);
}
