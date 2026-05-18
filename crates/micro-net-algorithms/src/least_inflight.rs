//! Least-inflight routing baseline.

use micro_net_core::{NodeId, Request, RoutingContext, RoutingDecision, RoutingPolicy};

/// Chooses the candidate with the smallest current `inflight` value.
#[derive(Default)]
pub struct LeastInflightPolicy;

impl LeastInflightPolicy {
    /// Creates a least-inflight policy.
    pub fn new() -> Self {
        Self
    }
}

impl RoutingPolicy for LeastInflightPolicy {
    fn name(&self) -> &'static str {
        "least-inflight"
    }

    fn choose(
        &mut self,
        ctx: &RoutingContext<'_>,
        _request: &Request,
        candidates: &[NodeId],
    ) -> RoutingDecision {
        let chosen = candidates
            .iter()
            .min_by_key(|candidate| ctx.runtime.node_or_default(candidate).inflight)
            .cloned()
            .unwrap_or_else(|| NodeId::new("<none>"));
        RoutingDecision {
            chosen,
            candidates: candidates.to_vec(),
            score: None,
            explanations: Vec::new(),
            metadata: Default::default(),
        }
    }
}
