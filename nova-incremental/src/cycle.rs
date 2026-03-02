//! Cycle detection and validation for the incremental computation graph.
//!
//! ## Design
//!
//! The incremental engine allows cycles under a single specific condition:
//! the **back-edge** (the edge closing the cycle) must connect to a
//! `Collection`-typed input slot of a [`TransformNode`].  This enables
//! "growing gather" patterns where a transform adds new members to a
//! collection, which itself may feed back and add more members.
//!
//! Any other cycle (back-edge targeting a `Single` input slot) is rejected
//! immediately as [`CycleError::IllegalCycle`].
//!
//! Legal cycles are stored as [`SccGroup`]s and processed with a bounded
//! fixed-point loop in the scheduler (see `scheduler.rs`).
//!
//! ## Algorithm
//!
//! We use **Tarjan's algorithm** to find all Strongly Connected Components
//! (SCCs) in the `TransformNode` dependency graph.  An SCC with more than
//! one member (or a self-loop) is a cycle.

use std::collections::HashMap;
use crate::node_id::NodeId;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum CycleError {
    /// A cycle was detected whose back-edge targets a `Single` input slot,
    /// which is not allowed.
    IllegalCycle {
        members: Vec<NodeId>,
        message: String,
    },
}

impl std::fmt::Display for CycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CycleError::IllegalCycle { message, .. } => write!(f, "illegal cycle: {message}"),
        }
    }
}

impl std::error::Error for CycleError {}

// ---------------------------------------------------------------------------
// SccGroup
// ---------------------------------------------------------------------------

/// A set of `TransformNode` IDs that form a legal (collection-gated) cycle.
#[derive(Debug, Clone, Default)]
pub struct SccGroup {
    pub members: Vec<NodeId>,
}

impl SccGroup {
    pub fn new(members: Vec<NodeId>) -> Self { Self { members } }
    pub fn contains(&self, id: NodeId) -> bool { self.members.contains(&id) }
}

// ---------------------------------------------------------------------------
// Tarjan's SCC
// ---------------------------------------------------------------------------

/// State for Tarjan's SCC algorithm over the `TransformNode` graph.
pub struct TarjanScc<'a> {
    /// Adjacency list: for each transform node, which transform nodes it
    /// feeds into (via its output edges → downstream transform input slots).
    adj: &'a HashMap<NodeId, Vec<NodeId>>,
    index_counter: u32,
    stack: Vec<NodeId>,
    on_stack: HashMap<NodeId, bool>,
    index: HashMap<NodeId, u32>,
    lowlink: HashMap<NodeId, u32>,
    sccs: Vec<Vec<NodeId>>,
}

impl<'a> TarjanScc<'a> {
    pub fn new(adj: &'a HashMap<NodeId, Vec<NodeId>>) -> Self {
        let n = adj.len();
        Self {
            adj,
            index_counter: 0,
            stack: Vec::new(),
            on_stack: HashMap::with_capacity(n),
            index: HashMap::with_capacity(n),
            lowlink: HashMap::with_capacity(n),
            sccs: Vec::new(),
        }
    }

    /// Run the algorithm and return all SCCs (each as a `Vec<NodeId>`).
    /// SCCs with a single member and no self-loop are trivial (not cycles).
    pub fn run(mut self, nodes: impl IntoIterator<Item = NodeId>) -> Vec<Vec<NodeId>> {
        let all_nodes: Vec<NodeId> = nodes.into_iter().collect();
        for &n in &all_nodes {
            if !self.index.contains_key(&n) {
                self.strongconnect(n);
            }
        }
        self.sccs
    }

    fn strongconnect(&mut self, v: NodeId) {
        let idx = self.index_counter;
        self.index.insert(v, idx);
        self.lowlink.insert(v, idx);
        self.index_counter += 1;
        self.stack.push(v);
        self.on_stack.insert(v, true);

        let neighbours: Vec<NodeId> = self.adj
            .get(&v)
            .map(|ns| ns.clone())
            .unwrap_or_default();

        for w in neighbours {
            if !self.index.contains_key(&w) {
                self.strongconnect(w);
                let w_low = self.lowlink[&w];
                let v_low = self.lowlink.get_mut(&v).unwrap();
                *v_low = (*v_low).min(w_low);
            } else if *self.on_stack.get(&w).unwrap_or(&false) {
                let w_idx = self.index[&w];
                let v_low = self.lowlink.get_mut(&v).unwrap();
                *v_low = (*v_low).min(w_idx);
            }
        }

        if self.lowlink[&v] == self.index[&v] {
            let mut scc = Vec::new();
            loop {
                let w = self.stack.pop().unwrap();
                self.on_stack.insert(w, false);
                scc.push(w);
                if w == v { break; }
            }
            self.sccs.push(scc);
        }
    }
}
