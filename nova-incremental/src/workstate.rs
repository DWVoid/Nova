//! Mutable per-update-cycle graph state.
use std::collections::HashMap;
use crate::node_id::NodeId;
use crate::topology::EdgeId;
use crate::transform::TransformError;
use crate::value::{Value, ValueHash};
#[derive(Debug, Clone)]
pub(crate) enum NodeStatus { Dirty, Clean, Error(TransformError) }
impl NodeStatus { pub(crate) fn is_dirty(&self) -> bool { matches!(self, NodeStatus::Dirty) } }
#[derive(Debug, Clone)]
pub(crate) enum EdgeValue {
    Single  { value: Option<Value>, hash: Option<ValueHash>, dirty: bool },
    Collection(Vec<CollectionElement>),
}
impl EdgeValue {
    pub(crate) fn single()     -> Self { Self::Single { value: None, hash: None, dirty: false } }
    pub(crate) fn collection() -> Self { Self::Collection(vec![]) }
}
#[derive(Debug, Clone)]
pub(crate) struct CollectionElement {
    pub(crate) key: u64, pub(crate) value: Value,
    pub(crate) hash: ValueHash, pub(crate) dirty: bool,
}
#[derive(Debug, Default)]
pub(crate) struct WorkState {
    pub(crate) node_status: HashMap<NodeId, NodeStatus>,
    pub(crate) edge_values: HashMap<EdgeId, EdgeValue>,
}
impl WorkState {
    pub(crate) fn new() -> Self { Self::default() }
    pub(crate) fn mark_dirty(&mut self, id: NodeId) { self.node_status.insert(id, NodeStatus::Dirty); }
    pub(crate) fn mark_clean(&mut self, id: NodeId) { self.node_status.insert(id, NodeStatus::Clean); }
    pub(crate) fn mark_error(&mut self, id: NodeId, err: TransformError) { self.node_status.insert(id, NodeStatus::Error(err)); }
    pub(crate) fn is_dirty(&self, id: NodeId) -> bool { self.node_status.get(&id).map(|s| s.is_dirty()).unwrap_or(true) }
    pub(crate) fn mark_all_dirty<'a>(&mut self, ids: impl Iterator<Item=&'a NodeId>) { for &id in ids { self.mark_dirty(id); } }
    pub(crate) fn mark_all_clean<'a>(&mut self, ids: impl Iterator<Item=&'a NodeId>) { for &id in ids { self.mark_clean(id); } }
    pub(crate) fn init_edge(&mut self, eid: EdgeId, is_collection: bool) {
        let v = if is_collection { EdgeValue::collection() } else { EdgeValue::single() };
        self.edge_values.entry(eid).or_insert(v);
    }
    pub(crate) fn read_single(&self, eid: EdgeId) -> Option<(Value, ValueHash)> {
        match self.edge_values.get(&eid)? {
            EdgeValue::Single { value: Some(v), hash: Some(h), .. } => Some((v.clone(), *h)),
            _ => None,
        }
    }
    pub(crate) fn read_collection(&self, eid: EdgeId) -> &[CollectionElement] {
        match self.edge_values.get(&eid) {
            Some(EdgeValue::Collection(elems)) => elems,
            _ => &[],
        }
    }
    pub(crate) fn write_single(&mut self, eid: EdgeId, value: Value, hash: ValueHash) -> bool {
        match self.edge_values.get_mut(&eid) {
            Some(EdgeValue::Single { value: v, hash: h, dirty: d }) => {
                if h.map(|old| old != hash).unwrap_or(true) { *v = Some(value); *h = Some(hash); *d = true; true } else { false }
            }
            _ => { self.edge_values.insert(eid, EdgeValue::Single { value: Some(value), hash: Some(hash), dirty: true }); true }
        }
    }
    pub(crate) fn preload_single(&mut self, eid: EdgeId, value: Value, hash: ValueHash) {
        self.edge_values.insert(eid, EdgeValue::Single { value: Some(value), hash: Some(hash), dirty: false });
    }
    pub(crate) fn upsert_element(&mut self, eid: EdgeId, key: u64, value: Value, hash: ValueHash) -> bool {
        let elems = self.collection_mut(eid);
        if let Some(el) = elems.iter_mut().find(|e| e.key == key) {
            if el.hash != hash { el.value = value; el.hash = hash; el.dirty = true; true } else { false }
        } else { elems.push(CollectionElement { key, value, hash, dirty: true }); true }
    }
    pub(crate) fn replace_collection(&mut self, eid: EdgeId, items: &[(u64, Value, ValueHash)]) -> bool {
        let new_keys: std::collections::HashSet<u64> = items.iter().map(|(k,_,_)| *k).collect();
        let elems = self.collection_mut(eid);
        let before = elems.len();
        elems.retain(|el| new_keys.contains(&el.key));
        let mut changed = elems.len() < before;
        for (key, value, hash) in items {
            if let Some(el) = elems.iter_mut().find(|e| e.key == *key) {
                if el.hash != *hash { el.value = value.clone(); el.hash = *hash; el.dirty = true; changed = true; }
            } else { elems.push(CollectionElement { key: *key, value: value.clone(), hash: *hash, dirty: true }); changed = true; }
        }
        changed
    }
    pub(crate) fn remove_elements(&mut self, eid: EdgeId, keys: &std::collections::HashSet<u64>) -> bool {
        if let Some(EdgeValue::Collection(elems)) = self.edge_values.get_mut(&eid) {
            let before = elems.len(); elems.retain(|el| !keys.contains(&el.key)); return elems.len() < before;
        }
        false
    }
    pub(crate) fn clear_dirty(&mut self, eid: EdgeId) {
        match self.edge_values.get_mut(&eid) {
            Some(EdgeValue::Single { dirty: d, .. }) => *d = false,
            Some(EdgeValue::Collection(elems)) => { for el in elems.iter_mut() { el.dirty = false; } }
            None => {}
        }
    }
    pub(crate) fn clear_all_values(&mut self) {
        for v in self.edge_values.values_mut() {
            match v {
                EdgeValue::Single { value, hash, dirty } => { *value = None; *hash = None; *dirty = false; }
                EdgeValue::Collection(elems) => elems.clear(),
            }
        }
    }
    pub(crate) fn collection_keys(&self, eid: EdgeId) -> Vec<u64> {
        match self.edge_values.get(&eid) {
            Some(EdgeValue::Collection(elems)) => elems.iter().map(|e| e.key).collect(),
            _ => vec![],
        }
    }
    pub(crate) fn is_edge_dirty(&self, eid: EdgeId) -> bool {
        match self.edge_values.get(&eid) {
            Some(EdgeValue::Single { dirty: d, .. }) => *d,
            Some(EdgeValue::Collection(elems)) => elems.iter().any(|e| e.dirty),
            None => false,
        }
    }
    fn collection_mut(&mut self, eid: EdgeId) -> &mut Vec<CollectionElement> {
        let entry = self.edge_values.entry(eid).or_insert_with(EdgeValue::collection);
        match entry { EdgeValue::Collection(elems) => elems, _ => panic!("collection_mut on non-collection edge") }
    }
}
