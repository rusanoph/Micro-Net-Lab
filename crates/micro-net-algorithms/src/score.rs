//! Explainable weighted score routing policy.

use crate::features::{
    DownstreamPressureFeature, ErrorRateFeature, HostPressureFeature, InflightFeature,
    LatencyFeature, NetworkCostFeature,
};
use micro_net_core::{
    CandidateScoreExplanation, Feature, FeatureContribution, NodeId, Request, RoutingContext,
    RoutingDecision, RoutingPolicy,
};
use std::collections::BTreeMap;

/// Weighted score policy. Lower score wins.
pub struct ScorePolicyV1 {
    features: Vec<(Box<dyn Feature>, f64)>,
}

impl ScorePolicyV1 {
    /// Creates a policy from feature/weight pairs.
    pub fn new(features: Vec<(Box<dyn Feature>, f64)>) -> Self {
        Self { features }
    }

    /// Creates the default dependency-aware score policy.
    pub fn dependency_aware_default() -> Self {
        Self::new(vec![
            (Box::new(LatencyFeature), 0.30),
            (Box::new(InflightFeature), 0.25),
            (Box::new(ErrorRateFeature), 0.15),
            (Box::new(NetworkCostFeature), 0.10),
            (Box::new(DownstreamPressureFeature), 0.15),
            (Box::new(HostPressureFeature), 0.05),
        ])
    }
}

impl Default for ScorePolicyV1 {
    fn default() -> Self {
        Self::dependency_aware_default()
    }
}

impl RoutingPolicy for ScorePolicyV1 {
    fn name(&self) -> &'static str {
        "score-v1"
    }

    fn choose(
        &mut self,
        ctx: &RoutingContext<'_>,
        request: &Request,
        candidates: &[NodeId],
    ) -> RoutingDecision {
        let mut explanations = Vec::new();
        for candidate in candidates {
            let mut features = Vec::new();
            let mut score = 0.0;
            for (feature, weight) in &self.features {
                let raw_value = feature.value(ctx, request, candidate);
                let normalized_value = raw_value.max(0.0);
                let contribution = normalized_value * *weight;
                score += contribution;
                features.push(FeatureContribution {
                    feature: feature.name().to_string(),
                    raw_value,
                    normalized_value,
                    weight: *weight,
                    contribution,
                });
            }
            explanations.push(CandidateScoreExplanation {
                candidate: candidate.clone(),
                features,
                score,
            });
        }

        explanations.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| a.candidate.cmp(&b.candidate))
        });
        let chosen = explanations
            .first()
            .map(|e| e.candidate.clone())
            .unwrap_or_else(|| NodeId::new("<none>"));
        let score = explanations.first().map(|e| e.score);
        let mut metadata = BTreeMap::new();
        metadata.insert("selection_rule".into(), "min_score".into());
        RoutingDecision {
            chosen,
            candidates: candidates.to_vec(),
            score,
            explanations,
            metadata,
        }
    }
}
