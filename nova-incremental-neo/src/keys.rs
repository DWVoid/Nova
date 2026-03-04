//! Key types for addressing persisted values in storage.
//!
//! [`SlotStateKey`] identifies one output slot of one node instance.
//! [`ElementKey`] identifies one element within a collection output slot.
//! Both map to [`StorageKey`] (a UUID) via deterministic UUIDv5 derivation.

use uuid::Uuid;
use serde::{Serialize, Deserialize};
use crate::storage::StorageKey;

// ---------------------------------------------------------------------------
// Primitive ID types
// ---------------------------------------------------------------------------

/// Identifies a subgraph within the topology (0 = root).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) struct SubgraphId(pub(crate) u32);

/// Identifies one instance of a subgraph (element key from [`KeyExtractor`]).
/// The root subgraph always has instance key 0 (`UNIT_INSTANCE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) struct InstanceKey(pub(crate) u64);

/// The single instance of the root subgraph.
pub(crate) const UNIT_INSTANCE: InstanceKey = InstanceKey(0);

/// Identifies a node within the topology by its UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) struct NodeId(pub(crate) Uuid);

impl NodeId {
    pub(crate) fn from_uuid(u: Uuid) -> Self { Self(u) }
    pub(crate) fn as_uuid(self) -> Uuid { self.0 }
}

/// Index of a slot on a node (0-based).
pub(crate) type SlotIndex = usize;

/// Unique identifier for an edge within the topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct EdgeId(pub(crate) u32);

// ---------------------------------------------------------------------------
// SlotStateKey
// ---------------------------------------------------------------------------

/// Composite key that uniquely identifies one output slot for one node instance.
///
/// This is the primary key for both [`WorkState`](crate::workstate::WorkState)
/// flags and [`ValueStore`](crate::value_store::ValueStore) cache entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) struct SlotStateKey {
    pub(crate) subgraph: SubgraphId,
    pub(crate) instance: InstanceKey,
    pub(crate) node: NodeId,
    pub(crate) slot: SlotIndex,
}

impl SlotStateKey {
    pub(crate) fn new(
        subgraph: SubgraphId,
        instance: InstanceKey,
        node: NodeId,
        slot: SlotIndex,
    ) -> Self {
        Self { subgraph, instance, node, slot }
    }

    /// Derive a [`StorageKey`] via UUIDv5 over the serialised key tuple.
    pub(crate) fn to_storage_key(self) -> StorageKey {
        // Namespace: a fixed UUID representing "nova-incremental-neo slot"
        const NS: Uuid = Uuid::from_bytes([
            0x6e, 0x69, 0x6e, 0x2d, 0x73, 0x6c, 0x6f, 0x74,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        let bytes = slot_key_bytes(self);
        StorageKey::from_uuid(Uuid::new_v5(&NS, &bytes))
    }

    /// Derive a [`StorageKey`] for the collection-index record of this slot.
    /// (Distinct from the slot value key itself.)
    pub(crate) fn collection_index_storage_key(self) -> StorageKey {
        const NS: Uuid = Uuid::from_bytes([
            0x6e, 0x69, 0x6e, 0x2d, 0x63, 0x69, 0x64, 0x78,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        let bytes = slot_key_bytes(self);
        StorageKey::from_uuid(Uuid::new_v5(&NS, &bytes))
    }
}

fn slot_key_bytes(k: SlotStateKey) -> Vec<u8> {
    let mut b = Vec::with_capacity(28);
    b.extend_from_slice(&k.subgraph.0.to_le_bytes());
    b.extend_from_slice(&k.instance.0.to_le_bytes());
    b.extend_from_slice(k.node.0.as_bytes());
    b.extend_from_slice(&(k.slot as u64).to_le_bytes());
    b
}

// ---------------------------------------------------------------------------
// ElementKey
// ---------------------------------------------------------------------------

/// Identifies one element within a collection output slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ElementKey {
    pub(crate) slot: SlotStateKey,
    pub(crate) element_key: u64,
}

impl ElementKey {
    pub(crate) fn new(slot: SlotStateKey, element_key: u64) -> Self {
        Self { slot, element_key }
    }

    /// Derive a [`StorageKey`] via UUIDv5 over the element key bytes.
    pub(crate) fn to_storage_key(self) -> StorageKey {
        const NS: Uuid = Uuid::from_bytes([
            0x6e, 0x69, 0x6e, 0x2d, 0x65, 0x6c, 0x65, 0x6d,
            0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]);
        let slot_bytes = slot_key_bytes(self.slot);
        let mut b = Vec::with_capacity(slot_bytes.len() + 8);
        b.extend_from_slice(&slot_bytes);
        b.extend_from_slice(&self.element_key.to_le_bytes());
        StorageKey::from_uuid(Uuid::new_v5(&NS, &b))
    }
}

// ---------------------------------------------------------------------------
// NodeInstanceKey — used by WorkState for locking
// ---------------------------------------------------------------------------

/// Identifies a node instance (subgraph + instance + node, without a slot).
/// Used as the key for the enqueue-deduplication set and NodeInstanceState map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct NodeInstanceKey {
    pub(crate) subgraph: SubgraphId,
    pub(crate) instance: InstanceKey,
    pub(crate) node: NodeId,
}

impl NodeInstanceKey {
    pub(crate) fn new(subgraph: SubgraphId, instance: InstanceKey, node: NodeId) -> Self {
        Self { subgraph, instance, node }
    }

    /// The [`SlotStateKey`] for output slot `slot` of this instance.
    pub(crate) fn slot_key(self, slot: SlotIndex) -> SlotStateKey {
        SlotStateKey::new(self.subgraph, self.instance, self.node, slot)
    }
}

// ---------------------------------------------------------------------------
// SubgraphInstanceKey — used by WorkState instances map
// ---------------------------------------------------------------------------

/// Identifies a (parent subgraph, parent instance) pair that owns child instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SubgraphInstanceKey {
    pub(crate) subgraph: SubgraphId,
    pub(crate) parent_instance: InstanceKey,
}

impl SubgraphInstanceKey {
    pub(crate) fn new(subgraph: SubgraphId, parent_instance: InstanceKey) -> Self {
        Self { subgraph, parent_instance }
    }
}

// ---------------------------------------------------------------------------
// WorkState snapshot UUID
// ---------------------------------------------------------------------------

/// Fixed well-known [`StorageKey`] for the serialised WorkState snapshot.
pub(crate) fn workstate_storage_key() -> StorageKey {
    const STATE_UUID: Uuid = Uuid::from_bytes([
        0x6e, 0x69, 0x6e, 0x2d, 0x77, 0x73, 0x74, 0x61,
        0x74, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ]);
    StorageKey::from_uuid(STATE_UUID)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_state_key_to_storage_key_is_deterministic() {
        let k = SlotStateKey::new(
            SubgraphId(0),
            UNIT_INSTANCE,
            NodeId::from_uuid(Uuid::nil()),
            0,
        );
        assert_eq!(k.to_storage_key(), k.to_storage_key());
    }

    #[test]
    fn test_distinct_slots_produce_distinct_keys() {
        let node = NodeId::from_uuid(Uuid::new_v4());
        let k0 = SlotStateKey::new(SubgraphId(0), UNIT_INSTANCE, node, 0);
        let k1 = SlotStateKey::new(SubgraphId(0), UNIT_INSTANCE, node, 1);
        assert_ne!(k0.to_storage_key(), k1.to_storage_key());
    }

    #[test]
    fn test_element_key_distinct_from_slot_key() {
        let node = NodeId::from_uuid(Uuid::new_v4());
        let sk = SlotStateKey::new(SubgraphId(0), UNIT_INSTANCE, node, 0);
        let ek = ElementKey::new(sk, 42);
        assert_ne!(sk.to_storage_key(), ek.to_storage_key());
    }
}
