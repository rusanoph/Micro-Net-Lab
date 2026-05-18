//! Deterministic tick-based in-memory simulation driver.

use micro_net_core::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

/// Pure in-memory discrete-event simulation engine.
///
/// The engine knows only about core traits: `GraphBackend`, `RoutingPolicy`,
/// `WorkloadGenerator` and `EventSink`. Docker/K8s/stub integrations can be
/// added later as different drivers without rewriting policy/domain code.
pub struct InMemorySimulationEngine<G: GraphBackend> {
    topology: TopologySpec,
    graph: G,
}

impl<G: GraphBackend> InMemorySimulationEngine<G> {
    /// Creates a new engine from a serializable topology and a graph backend.
    pub fn new(topology: TopologySpec, graph: G) -> Self {
        Self { topology, graph }
    }

    /// Runs one deterministic experiment.
    pub fn run(
        &self,
        experiment: &ExperimentSpec,
        policy: &mut dyn RoutingPolicy,
        workload: &mut dyn WorkloadGenerator,
        sink: &mut dyn EventSink,
    ) -> anyhow::Result<SimulationSummary> {
        let mut rng = ChaCha8Rng::seed_from_u64(experiment.seed);
        let mut state = SimulationState::default();
        let service_index = self.topology.service_index();
        let mut created = 0u64;
        let mut completed = 0u64;
        let mut failed = 0u64;
        let mut latencies = Vec::new();

        initialize_runtime(&self.topology, &mut state);

        sink.on_event(&TraceEvent::SimulationStarted {
            schema_version: "0.1".into(),
            experiment_id: experiment.id.to_string(),
            seed: experiment.seed,
            policy: policy.name().to_string(),
        })?;

        for tick in 0..experiment.duration_ticks {
            state.tick = tick;
            sink.on_event(&TraceEvent::TickStarted { tick })?;

            let mut still_active = Vec::new();
            let active_requests = std::mem::take(&mut state.active_requests);
            for active in active_requests {
                if active.complete_at <= tick {
                    decrement_inflight(&mut state.runtime, &active.chosen);
                    if active.will_fail {
                        failed += 1;
                        update_finished_metrics(
                            &mut state.runtime,
                            &active.chosen,
                            active.latency_ms,
                            true,
                        );
                        let event = TraceEvent::RequestFailed {
                            tick,
                            request_id: active.request.id,
                            reason: FailureReason::BackendFailure,
                        };
                        sink.on_event(&event)?;
                        policy.on_event(&event);
                    } else {
                        completed += 1;
                        latencies.push(active.latency_ms);
                        update_finished_metrics(
                            &mut state.runtime,
                            &active.chosen,
                            active.latency_ms,
                            false,
                        );
                        let event = TraceEvent::RequestCompleted {
                            tick,
                            request_id: active.request.id,
                            chosen: active.chosen,
                            latency_ms: active.latency_ms,
                        };
                        sink.on_event(&event)?;
                        policy.on_event(&event);
                    }
                } else {
                    still_active.push(active);
                }
            }
            state.active_requests = still_active;
            refresh_utilization(&self.topology, &mut state.runtime);

            let workload_ctx = WorkloadContext {
                topology: &self.topology,
                tick,
            };
            for request in workload.generate(&workload_ctx) {
                created += 1;
                sink.on_event(&TraceEvent::RequestCreated {
                    tick,
                    request_id: request.id,
                    source: request.source.clone(),
                    target: request.target.clone(),
                })?;
                let candidates = service_index.candidates(&request.target);
                if candidates.is_empty() {
                    failed += 1;
                    sink.on_event(&TraceEvent::RequestFailed {
                        tick,
                        request_id: request.id,
                        reason: FailureReason::NoCandidates,
                    })?;
                    continue;
                }

                let routing_ctx = RoutingContext {
                    topology: &self.topology,
                    graph: &self.graph,
                    runtime: &state.runtime,
                    tick,
                };
                let decision = policy.choose(&routing_ctx, &request, &candidates);
                sink.on_event(&TraceEvent::RouteChosen {
                    tick,
                    request_id: request.id,
                    algorithm: policy.name().to_string(),
                    candidates: decision.candidates.clone(),
                    chosen: decision.chosen.clone(),
                    score: decision.score,
                    explanations: decision.explanations.clone(),
                })?;

                if !candidates.contains(&decision.chosen) {
                    failed += 1;
                    sink.on_event(&TraceEvent::RequestFailed {
                        tick,
                        request_id: request.id,
                        reason: FailureReason::NoCandidates,
                    })?;
                    continue;
                }

                let Some(path) = self.graph.shortest_path(&request.source, &decision.chosen) else {
                    failed += 1;
                    sink.on_event(&TraceEvent::RequestFailed {
                        tick,
                        request_id: request.id,
                        reason: FailureReason::NoPath,
                    })?;
                    continue;
                };
                for edge_id in &path.edges {
                    if let Some(edge) = self.graph.edge(edge_id) {
                        sink.on_event(&TraceEvent::EdgeTraversed {
                            tick,
                            request_id: request.id,
                            edge_id: edge.id.clone(),
                            from: edge.from.clone(),
                            to: edge.to.clone(),
                            latency_ms: edge.latency_ms,
                        })?;
                    }
                }

                increment_inflight(&mut state.runtime, &decision.chosen);
                let mut latency_ms = path.total_latency_ms
                    + service_processing_latency(&self.topology, &decision.chosen);
                latency_ms += model_downstream_calls(
                    &self.topology,
                    &self.graph,
                    &state.runtime,
                    &request,
                    &decision.chosen,
                    tick,
                    sink,
                )?;
                let node_metrics = state.runtime.node_or_default(&decision.chosen);
                let failure_probability = (node_metrics.error_rate
                    + 0.02 * node_metrics.utilization.max(0.0))
                .clamp(0.0, 0.95);
                let will_fail = rng.gen_bool(failure_probability);
                let ticks_to_complete = (latency_ms / 10.0).ceil().max(1.0) as u64;
                state.active_requests.push(ActiveRequest {
                    request,
                    chosen: decision.chosen,
                    complete_at: tick + ticks_to_complete,
                    latency_ms,
                    will_fail,
                });
            }

            apply_synthetic_resource_pressure(&self.topology, &mut state.runtime, tick);
            sink.on_event(&TraceEvent::TickCompleted { tick })?;
        }

        let active_at_end = state.active_requests.len() as u64;
        let summary = SimulationSummary::from_samples(
            experiment.id.to_string(),
            policy.name().to_string(),
            experiment.seed,
            experiment.duration_ticks,
            created,
            completed,
            failed,
            active_at_end,
            latencies,
        );
        sink.on_event(&TraceEvent::SimulationCompleted {
            experiment_id: experiment.id.to_string(),
            created,
            completed,
            failed,
        })?;
        Ok(summary)
    }
}

fn initialize_runtime(topology: &TopologySpec, state: &mut SimulationState) {
    for node in &topology.nodes {
        let mut runtime = NodeRuntimeState::default();
        match &node.kind {
            NodeKind::Database(_) => runtime.db_pressure = 0.10,
            NodeKind::Cache(spec) => runtime.cache_miss_risk = 1.0 - spec.base_hit_rate,
            NodeKind::Broker(spec) => runtime.broker_lag = spec.base_lag_ms / 100.0,
            _ => {}
        }
        state.runtime.nodes.insert(node.id.clone(), runtime);
    }
}

fn increment_inflight(runtime: &mut RuntimeSnapshot, node: &NodeId) {
    runtime.nodes.entry(node.clone()).or_default().inflight += 1;
}

fn decrement_inflight(runtime: &mut RuntimeSnapshot, node: &NodeId) {
    let entry = runtime.nodes.entry(node.clone()).or_default();
    entry.inflight = entry.inflight.saturating_sub(1);
}

fn update_finished_metrics(
    runtime: &mut RuntimeSnapshot,
    node: &NodeId,
    latency_ms: f64,
    failed: bool,
) {
    let entry = runtime.nodes.entry(node.clone()).or_default();
    if failed {
        entry.failed += 1;
    } else {
        entry.completed += 1;
    }
    let n = (entry.completed + entry.failed).max(1) as f64;
    entry.avg_latency_ms += (latency_ms - entry.avg_latency_ms) / n;
    entry.error_rate = entry.failed as f64 / n;
}

fn refresh_utilization(topology: &TopologySpec, runtime: &mut RuntimeSnapshot) {
    let mut host_inflight: BTreeMap<HostId, u64> = BTreeMap::new();
    for node in &topology.nodes {
        if let Some(host) = &node.host {
            let inflight = runtime.node_or_default(&node.id).inflight;
            *host_inflight.entry(host.clone()).or_default() += inflight;
        }
    }

    for node in &topology.nodes {
        let entry = runtime.nodes.entry(node.id.clone()).or_default();
        let capacity = match &node.kind {
            NodeKind::Service(spec) => spec.base_capacity_rps.max(1.0),
            NodeKind::Database(spec) => spec.max_connections as f64,
            NodeKind::Cache(_) => 250.0,
            NodeKind::Broker(spec) => (spec.partitions as f64 * 50.0).max(1.0),
            _ => 1000.0,
        };
        entry.utilization = entry.inflight as f64 / capacity;
        entry.host_pressure = node
            .host
            .as_ref()
            .and_then(|h| host_inflight.get(h))
            .copied()
            .unwrap_or(0) as f64
            / capacity;
    }
}

fn service_processing_latency(topology: &TopologySpec, node: &NodeId) -> f64 {
    match topology.node(node).map(|n| &n.kind) {
        Some(NodeKind::Service(spec)) => spec.base_processing_latency_ms,
        _ => 1.0,
    }
}

fn model_downstream_calls(
    topology: &TopologySpec,
    graph: &dyn GraphBackend,
    runtime: &RuntimeSnapshot,
    request: &Request,
    chosen: &NodeId,
    tick: Tick,
    sink: &mut dyn EventSink,
) -> anyhow::Result<f64> {
    let mut total = 0.0;
    for dependency in topology.dependencies_for_instance(chosen) {
        for target in topology.resolve_dependency(chosen, dependency) {
            let path_latency = graph
                .shortest_path(chosen, &target)
                .map(|p| p.total_latency_ms)
                .unwrap_or(25.0);
            let target_runtime = runtime.node_or_default(&target);
            let pressure_penalty = 10.0
                * (target_runtime.utilization
                    + target_runtime.db_pressure
                    + target_runtime.cache_miss_risk
                    + target_runtime.broker_lag);
            let mode_factor = match dependency.call_mode {
                CallMode::Synchronous => 1.0,
                CallMode::Asynchronous => 0.25,
                CallMode::FireAndForget => 0.10,
            };
            let latency = dependency.probability
                * mode_factor
                * (dependency.base_operation_latency_ms + path_latency + pressure_penalty);
            total += latency;
            sink.on_event(&TraceEvent::DependencyTouched {
                tick,
                request_id: request.id,
                caller: chosen.clone(),
                dependency: dependency.id.clone(),
                target,
                latency_ms: latency,
            })?;
        }
    }
    Ok(total)
}

fn apply_synthetic_resource_pressure(
    topology: &TopologySpec,
    runtime: &mut RuntimeSnapshot,
    tick: Tick,
) {
    let wave = ((tick % 100) as f64 / 100.0).sin().abs();
    for node in &topology.nodes {
        let entry = runtime.nodes.entry(node.id.clone()).or_default();
        match &node.kind {
            NodeKind::Database(_) => {
                entry.db_pressure = (0.10 + entry.utilization * 0.8 + wave * 0.08).min(2.0)
            }
            NodeKind::Cache(spec) => {
                entry.cache_miss_risk =
                    ((1.0 - spec.base_hit_rate) + entry.utilization * 0.25 + wave * 0.04).min(1.0)
            }
            NodeKind::Broker(spec) => {
                entry.broker_lag =
                    (spec.base_lag_ms / 100.0 + entry.utilization * 0.6 + wave * 0.05).min(2.0)
            }
            _ => {}
        }
    }
}
