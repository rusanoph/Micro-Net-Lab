//! Routing policy and score feature extension points.

use crate::graph::GraphBackend;
use crate::ids::*;
use crate::metrics::RuntimeSnapshot;
use crate::simulation::Request;
use crate::topology::TopologySpec;
use crate::trace::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Read-only context passed to a routing policy.
pub struct RoutingContext<'a> {
    /// Serializable topology specification.
    pub topology: &'a TopologySpec,
    /// Graph backend used for path metrics.
    pub graph: &'a dyn GraphBackend,
    /// Runtime metric snapshot.
    pub runtime: &'a RuntimeSnapshot,
    /// Current simulation tick.
    pub tick: Tick,
}

/// Per-feature contribution used for explainable score-based decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureContribution {
    /// Feature name.
    pub feature: String,
    /// Raw feature value.
    pub raw_value: f64,
    /// Normalized feature value. In the first MVP raw and normalized may match.
    pub normalized_value: f64,
    /// Weight used by the score policy.
    pub weight: f64,
    /// Weighted contribution to the final score.
    pub contribution: f64,
}

/// Per-candidate explanation for a score-based routing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateScoreExplanation {
    /// Candidate node.
    pub candidate: NodeId,
    /// Feature contributions for the candidate.
    pub features: Vec<FeatureContribution>,
    /// Final score. Lower is better.
    pub score: f64,
}

/// Result of one routing decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingDecision {
    /// Selected concrete backend instance.
    pub chosen: NodeId,
    /// Candidate list considered by the policy.
    pub candidates: Vec<NodeId>,
    /// Optional scalar score of the chosen candidate.
    pub score: Option<f64>,
    /// Optional explainability payload for score-based policies.
    #[serde(default)]
    pub explanations: Vec<CandidateScoreExplanation>,
    /// Optional policy-specific metadata.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

/// Routing/load-balancing policy. It chooses a concrete service instance for a target logical service.
pub trait RoutingPolicy: Send {
    /// Stable policy name.
    fn name(&self) -> &'static str;

    /// Chooses one concrete backend from the candidate set.
    fn choose(
        &mut self,
        ctx: &RoutingContext<'_>,
        request: &Request,
        candidates: &[NodeId],
    ) -> RoutingDecision;

    /// Optional feedback hook for stateful policies such as EWMA, adaptive control or bandits.
    fn on_event(&mut self, _event: &TraceEvent) {}
}

/// Scalar feature used by score-based routing.
pub trait Feature: Send + Sync {
    /// Stable feature name.
    fn name(&self) -> &'static str;

    /// Computes a value for one routing candidate. Lower values are assumed better by `ScorePolicyV1`.
    fn value(&self, ctx: &RoutingContext<'_>, request: &Request, candidate: &NodeId) -> f64;
}
