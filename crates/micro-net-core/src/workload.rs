//! Workload generation extension points.

use crate::ids::*;
use crate::simulation::Request;
use crate::topology::TopologySpec;
use serde::{Deserialize, Serialize};

/// Read-only context passed to workload generators.
pub struct WorkloadContext<'a> {
    /// Serializable topology specification.
    pub topology: &'a TopologySpec,
    /// Current tick.
    pub tick: Tick,
}

/// Deterministic workload generator.
pub trait WorkloadGenerator: Send {
    /// Stable generator name.
    fn name(&self) -> &'static str;

    /// Generates requests for one tick.
    fn generate(&mut self, ctx: &WorkloadContext<'_>) -> Vec<Request>;
}

/// Simple deterministic workload configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstantWorkloadConfig {
    /// Number of requests created at each tick.
    pub requests_per_tick: u64,
    /// Source node.
    pub source: NodeId,
    /// Target logical services. Targets are rotated deterministically.
    pub targets: Vec<LogicalServiceId>,
    /// Request class assigned to generated requests.
    pub class: RequestClassId,
}

/// Constant workload generator used by the first CLI slice.
pub struct ConstantWorkloadGenerator {
    config: ConstantWorkloadConfig,
    next_request_id: RequestId,
    cursor: usize,
}

impl ConstantWorkloadGenerator {
    /// Creates a new constant workload generator.
    pub fn new(config: ConstantWorkloadConfig) -> Self {
        Self {
            config,
            next_request_id: 1,
            cursor: 0,
        }
    }
}

impl WorkloadGenerator for ConstantWorkloadGenerator {
    fn name(&self) -> &'static str {
        "constant"
    }

    fn generate(&mut self, ctx: &WorkloadContext<'_>) -> Vec<Request> {
        if self.config.targets.is_empty() {
            return Vec::new();
        }
        let mut requests = Vec::with_capacity(self.config.requests_per_tick as usize);
        for _ in 0..self.config.requests_per_tick {
            let target = self.config.targets[self.cursor % self.config.targets.len()].clone();
            self.cursor += 1;
            let id = self.next_request_id;
            self.next_request_id += 1;
            requests.push(Request {
                id,
                created_at: ctx.tick,
                source: self.config.source.clone(),
                target,
                class: self.config.class.clone(),
                timeout_budget_ticks: None,
            });
        }
        requests
    }
}
