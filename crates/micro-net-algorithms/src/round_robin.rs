//! Round-robin routing baseline.

use micro_net_core::{
    LogicalServiceId, NodeId, Request, RoutingContext, RoutingDecision, RoutingPolicy,
};
use std::collections::BTreeMap;

/// Stateful round-robin policy with one cursor per logical service.
#[derive(Default)]
pub struct RoundRobinPolicy {
    cursors: BTreeMap<LogicalServiceId, usize>,
}

impl RoundRobinPolicy {
    /// Creates an empty round-robin policy.
    pub fn new() -> Self {
        Self::default()
    }
}

impl RoutingPolicy for RoundRobinPolicy {
    fn name(&self) -> &'static str {
        "round-robin"
    }

    fn choose(
        &mut self,
        _ctx: &RoutingContext<'_>,
        request: &Request,
        candidates: &[NodeId],
    ) -> RoutingDecision {
        let chosen = if candidates.is_empty() {
            NodeId::new("<none>")
        } else {
            let cursor = self.cursors.entry(request.target.clone()).or_insert(0);
            let chosen = candidates[*cursor % candidates.len()].clone();
            *cursor += 1;
            chosen
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

#[cfg(test)]
mod tests {
    use super::*;
    use micro_net_core::{RuntimeSnapshot, TopologySpec};

    #[test]
    fn rotates_candidates() {
        struct EmptyGraph;
        impl micro_net_core::GraphBackend for EmptyGraph {
            fn node(&self, _: &micro_net_core::NodeId) -> Option<&micro_net_core::Node> {
                None
            }
            fn edge(&self, _: &micro_net_core::EdgeId) -> Option<&micro_net_core::Edge> {
                None
            }
            fn neighbors(&self, _: &micro_net_core::NodeId) -> Vec<micro_net_core::NodeId> {
                vec![]
            }
            fn edges_from(&self, _: &micro_net_core::NodeId) -> Vec<micro_net_core::EdgeId> {
                vec![]
            }
            fn shortest_path(
                &self,
                _: &micro_net_core::NodeId,
                _: &micro_net_core::NodeId,
            ) -> Option<micro_net_core::Path> {
                None
            }
            fn k_paths(
                &self,
                _: &micro_net_core::NodeId,
                _: &micro_net_core::NodeId,
                _: usize,
            ) -> Vec<micro_net_core::Path> {
                vec![]
            }
        }
        let topology = TopologySpec {
            schema_version: "0.1".into(),
            name: "t".into(),
            logical_services: vec![],
            nodes: vec![],
            edges: vec![],
            dependency_bindings: vec![],
        };
        let graph = EmptyGraph;
        let runtime = RuntimeSnapshot::default();
        let ctx = RoutingContext {
            topology: &topology,
            graph: &graph,
            runtime: &runtime,
            tick: 0,
        };
        let request = Request {
            id: 1,
            created_at: 0,
            source: "gateway".into(),
            target: "payments".into(),
            class: "default".into(),
            timeout_budget_ticks: None,
        };
        let candidates = vec!["a".into(), "b".into()];
        let mut policy = RoundRobinPolicy::new();
        assert_eq!(
            policy.choose(&ctx, &request, &candidates).chosen.as_str(),
            "a"
        );
        assert_eq!(
            policy.choose(&ctx, &request, &candidates).chosen.as_str(),
            "b"
        );
    }
}
