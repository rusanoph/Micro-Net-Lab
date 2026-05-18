//! Random routing baseline.

use micro_net_core::{NodeId, Request, RoutingContext, RoutingDecision, RoutingPolicy};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Deterministic random policy backed by a seedable RNG.
pub struct RandomPolicy {
    rng: ChaCha8Rng,
}

impl RandomPolicy {
    /// Creates a new random policy.
    pub fn new(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl RoutingPolicy for RandomPolicy {
    fn name(&self) -> &'static str {
        "random"
    }

    fn choose(
        &mut self,
        _ctx: &RoutingContext<'_>,
        _request: &Request,
        candidates: &[NodeId],
    ) -> RoutingDecision {
        let chosen = if candidates.is_empty() {
            NodeId::new("<none>")
        } else {
            candidates[self.rng.gen_range(0..candidates.len())].clone()
        };
        RoutingDecision {
            chosen,
            candidates: candidates.to_vec(),
            score: None,
            explanations: Vec::new(),
            metadata: Default::default(),
        }
    }
}
