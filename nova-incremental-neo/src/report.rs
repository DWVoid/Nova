//! [`UpdateReport`] — summary of one incremental update cycle.

use uuid::Uuid;
use crate::transform::TransformError;

/// Summary of a completed [`Engine::update`] call.
#[derive(Debug, Default, Clone)]
pub struct UpdateReport {
    /// Total number of transform nodes evaluated in this update.
    pub transforms_evaluated: usize,
    /// Number of transforms whose output changed.
    pub transforms_changed: usize,
    /// Number of transforms skipped because all inputs were unchanged.
    pub transforms_skipped: usize,
    /// Number of transforms blocked (inputs not yet available).
    pub transforms_blocked: usize,
    /// Total individual collection elements that changed.
    pub collection_elements_changed: usize,
    /// Per-transform errors encountered during execution.
    pub errors: Vec<(Uuid, TransformError)>,
    /// Transforms that hit the cycle limit without converging.
    pub cycle_limit_exceeded: Vec<Uuid>,
}

impl UpdateReport {
    /// Returns `true` if there were no errors and no cycle-limit violations.
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty() && self.cycle_limit_exceeded.is_empty()
    }
}
