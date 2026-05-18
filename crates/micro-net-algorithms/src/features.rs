//! Built-in score features.

use micro_net_core::{Feature, NodeId, NodeKind, Request, RoutingContext};

/// Direct or shortest-path latency from request source to candidate.
pub struct LatencyFeature;

impl Feature for LatencyFeature {
    fn name(&self) -> &'static str {
        "latency"
    }

    fn value(&self, ctx: &RoutingContext<'_>, request: &Request, candidate: &NodeId) -> f64 {
        ctx.graph
            .shortest_path(&request.source, candidate)
            .map(|p| p.total_latency_ms / 100.0)
            .unwrap_or(10.0)
    }
}

/// Candidate inflight pressure normalized by the service capacity when available.
pub struct InflightFeature;

impl Feature for InflightFeature {
    fn name(&self) -> &'static str {
        "inflight"
    }

    fn value(&self, ctx: &RoutingContext<'_>, _request: &Request, candidate: &NodeId) -> f64 {
        let metrics = ctx.runtime.node_or_default(candidate);
        let capacity = match ctx.topology.node(candidate).map(|n| &n.kind) {
            Some(NodeKind::Service(spec)) => spec.base_capacity_rps.max(1.0),
            _ => 100.0,
        };
        metrics.inflight as f64 / capacity
    }
}

/// Candidate runtime error-rate feature.
pub struct ErrorRateFeature;

impl Feature for ErrorRateFeature {
    fn name(&self) -> &'static str {
        "error_rate"
    }

    fn value(&self, ctx: &RoutingContext<'_>, _request: &Request, candidate: &NodeId) -> f64 {
        ctx.runtime.node_or_default(candidate).error_rate
    }
}

/// Network cost from request source to candidate.
pub struct NetworkCostFeature;

impl Feature for NetworkCostFeature {
    fn name(&self) -> &'static str {
        "network_cost"
    }

    fn value(&self, ctx: &RoutingContext<'_>, request: &Request, candidate: &NodeId) -> f64 {
        ctx.graph
            .shortest_path(&request.source, candidate)
            .map(|p| p.total_cost / 100.0)
            .unwrap_or(10.0)
    }
}

/// Host/placement pressure feature. It captures cases where multiple replicas share one host.
pub struct HostPressureFeature;

impl Feature for HostPressureFeature {
    fn name(&self) -> &'static str {
        "host_pressure"
    }

    fn value(&self, ctx: &RoutingContext<'_>, _request: &Request, candidate: &NodeId) -> f64 {
        ctx.runtime.node_or_default(candidate).host_pressure
    }
}

/// Dependency-aware feature that inspects concrete downstream bindings of a candidate.
///
/// The feature is low when the candidate's concrete database/cache/broker resources and
/// paths are healthy. It becomes high when shared downstream nodes are pressured or the
/// candidate reaches them through expensive links.
pub struct DownstreamPressureFeature;

impl Feature for DownstreamPressureFeature {
    fn name(&self) -> &'static str {
        "downstream_pressure"
    }

    fn value(&self, ctx: &RoutingContext<'_>, _request: &Request, candidate: &NodeId) -> f64 {
        let dependencies = ctx.topology.dependencies_for_instance(candidate);
        if dependencies.is_empty() {
            return 0.0;
        }

        let mut total = 0.0;
        let mut count = 0.0;
        for dependency in dependencies {
            for target in ctx.topology.resolve_dependency(candidate, dependency) {
                let runtime = ctx.runtime.node_or_default(&target);
                let path_penalty = ctx
                    .graph
                    .shortest_path(candidate, &target)
                    .map(|p| p.total_latency_ms / 100.0)
                    .unwrap_or(1.0);
                let node_penalty = runtime.utilization
                    + runtime.db_pressure
                    + runtime.cache_miss_risk
                    + runtime.broker_lag
                    + runtime.error_rate;
                total += dependency.probability * (path_penalty + node_penalty);
                count += dependency.probability.max(0.01);
            }
        }
        if count == 0.0 {
            0.0
        } else {
            total / count
        }
    }
}
