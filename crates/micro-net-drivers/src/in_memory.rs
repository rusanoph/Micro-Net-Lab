//! Deterministic tick-based in-memory simulation driver.

use micro_net_core::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::{BTreeMap, BTreeSet};

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
        let mut obs_rng = ChaCha8Rng::seed_from_u64(experiment.seed ^ 0x9E3779B97F4A7C15);
        let mut state = SimulationState::default();
        let service_index = self.topology.service_index();
        let service_nodes: Vec<NodeId> = self
            .topology
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Service(_)))
            .map(|n| n.id.clone())
            .collect();
        let logical_services: Vec<LogicalServiceId> = self
            .topology
            .logical_services
            .iter()
            .map(|s| s.id.clone())
            .collect();

        let mut created = 0u64;
        let mut completed = 0u64;
        let mut failed = 0u64;
        let mut drain_completed = 0u64;
        let mut drain_failed = 0u64;
        let mut latencies = Vec::new();
        let mut observed = state.runtime.clone();
        let mut observation_queue: BTreeMap<Tick, RuntimeSnapshot> = BTreeMap::new();
        // Summary-style utilization/pressure stats over the measurement window.
        let mut meas_ticks: u64 = 0;
        let mut node_util_max: f64 = 0.0;
        let mut host_pressure_max: f64 = 0.0;
        let mut db_pressure_max: f64 = 0.0;
        let mut cache_miss_max: f64 = 0.0;
        let mut broker_lag_max: f64 = 0.0;
        let mut err_rate_max: f64 = 0.0;
        let mut node_util_sum: f64 = 0.0;
        let mut host_pressure_sum: f64 = 0.0;
        // Fraction of measurement ticks where any service node has utilization > 1.0.
        let mut util_over_1_ticks: u64 = 0;
        // Replica imbalance: coefficient of variation (std/mean) of replica utilizations.
        let mut replica_cv_sum: f64 = 0.0;
        let mut replica_cv_max: f64 = 0.0;
        // Failure burstiness: max failure rate over a sliding tick window.
        const FAIL_WINDOW_TICKS: usize = 50;
        let mut fail_window: [u64; FAIL_WINDOW_TICKS] = [0; FAIL_WINDOW_TICKS];
        let mut finish_window: [u64; FAIL_WINDOW_TICKS] = [0; FAIL_WINDOW_TICKS];
        let mut window_pos: usize = 0;
        let mut window_fail_sum: u64 = 0;
        let mut window_finish_sum: u64 = 0;
        let mut fail_burstiness_max: f64 = 0.0;

        initialize_runtime(&self.topology, &mut state);
        observed = state.runtime.clone();

        sink.on_event(&TraceEvent::SimulationStarted {
            schema_version: "0.1".into(),
            experiment_id: experiment.id.to_string(),
            seed: experiment.seed,
            policy: policy.name().to_string(),
        })?;

        let full_trace = sink.trace_level() == TraceLevel::Full;
        let drain_start = experiment.duration_ticks.saturating_sub(experiment.drain_ticks);
        for tick in 0..experiment.duration_ticks {
            state.tick = tick;
            if full_trace {
                sink.on_event(&TraceEvent::TickStarted { tick })?;
            }
            let in_measurement = tick >= experiment.warmup_ticks && tick < drain_start;
            let in_drain = tick >= drain_start;
            if tick == experiment.warmup_ticks {
                created = 0;
                completed = 0;
                failed = 0;
                drain_completed = 0;
                drain_failed = 0;
                latencies.clear();
                meas_ticks = 0;
                node_util_max = 0.0;
                host_pressure_max = 0.0;
                db_pressure_max = 0.0;
                cache_miss_max = 0.0;
                broker_lag_max = 0.0;
                err_rate_max = 0.0;
                node_util_sum = 0.0;
                host_pressure_sum = 0.0;
                util_over_1_ticks = 0;
                replica_cv_sum = 0.0;
                replica_cv_max = 0.0;
                fail_window = [0; FAIL_WINDOW_TICKS];
                finish_window = [0; FAIL_WINDOW_TICKS];
                window_pos = 0;
                window_fail_sum = 0;
                window_finish_sum = 0;
                fail_burstiness_max = 0.0;
            }

            let mut still_active = Vec::new();
            let active_requests = std::mem::take(&mut state.active_requests);
            let mut tick_failed_finished: u64 = 0;
            let mut tick_total_finished: u64 = 0;
            for active in active_requests {
                if active.complete_at <= tick {
                    decrement_inflight(&mut state.runtime, &active.chosen);
                    if active.will_fail {
                        if in_measurement {
                            failed += 1;
                        } else if in_drain {
                            drain_failed += 1;
                        }
                        if in_measurement {
                            tick_failed_finished += 1;
                            tick_total_finished += 1;
                        }
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
                        if in_measurement {
                            completed += 1;
                            latencies.push(active.latency_ms);
                        } else if in_drain {
                            drain_completed += 1;
                        }
                        if in_measurement {
                            tick_total_finished += 1;
                        }
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
            apply_synthetic_resource_pressure(
                &self.topology,
                &mut state.runtime,
                tick,
                &experiment.scenario,
            );
            if in_measurement {
                // Capture high-level "run health" summaries without requiring per-request tracing.
                meas_ticks += 1;
                let mut any_service_over_1 = false;
                for node_id in &service_nodes {
                    let st = state.runtime.node_or_default(node_id);
                    if st.utilization > 1.0 {
                        any_service_over_1 = true;
                        break;
                    }
                }
                if any_service_over_1 {
                    util_over_1_ticks += 1;
                }

                // Replica imbalance: per logical service, compute CV(utilization across replicas).
                for svc in &logical_services {
                    let candidates = service_index.candidates_ref(svc);
                    if candidates.len() <= 1 {
                        continue;
                    }
                    let mut sum = 0.0;
                    let mut sum_sq = 0.0;
                    for c in candidates {
                        let u = state.runtime.node_or_default(c).utilization.max(0.0);
                        sum += u;
                        sum_sq += u * u;
                    }
                    let n = candidates.len() as f64;
                    let mean = sum / n;
                    if mean <= 0.0 {
                        continue;
                    }
                    let var = (sum_sq / n - mean * mean).max(0.0);
                    let std = var.sqrt();
                    let cv = std / mean;
                    replica_cv_sum += cv;
                    replica_cv_max = replica_cv_max.max(cv);
                }

                for st in state.runtime.nodes.values() {
                    node_util_max = node_util_max.max(st.utilization);
                    host_pressure_max = host_pressure_max.max(st.host_pressure);
                    db_pressure_max = db_pressure_max.max(st.db_pressure);
                    cache_miss_max = cache_miss_max.max(st.cache_miss_risk);
                    broker_lag_max = broker_lag_max.max(st.broker_lag);
                    err_rate_max = err_rate_max.max(st.error_rate);
                    node_util_sum += st.utilization;
                    host_pressure_sum += st.host_pressure;
                }

                // Failure burstiness window (measurement only).
                window_fail_sum = window_fail_sum.saturating_sub(fail_window[window_pos]);
                window_finish_sum = window_finish_sum.saturating_sub(finish_window[window_pos]);
                fail_window[window_pos] = tick_failed_finished;
                finish_window[window_pos] = tick_total_finished;
                window_fail_sum += tick_failed_finished;
                window_finish_sum += tick_total_finished;
                if window_finish_sum > 0 {
                    let rate = window_fail_sum as f64 / window_finish_sum as f64;
                    fail_burstiness_max = fail_burstiness_max.max(rate);
                }
                window_pos = (window_pos + 1) % FAIL_WINDOW_TICKS;
            }
            // Push ground-truth runtime to a delayed queue; policies see a lagged + noisy view.
            let deliver_at = tick.saturating_add(experiment.observability_lag_ticks);
            observation_queue.insert(deliver_at, state.runtime.clone());
            if let Some(delivered) = observation_queue.remove(&tick) {
                observed = delivered;
            }
            apply_observability_noise(&mut observed, &mut obs_rng, experiment.observability_noise);

            let workload_ctx = WorkloadContext {
                topology: &self.topology,
                tick,
            };
            let generated = if tick < drain_start {
                workload.generate(&workload_ctx)
            } else {
                Vec::new()
            };
            for request in generated {
                if in_measurement {
                    created += 1;
                }
                if full_trace {
                    sink.on_event(&TraceEvent::RequestCreated {
                        tick,
                        request_id: request.id,
                        source: request.source.clone(),
                        target: request.target.clone(),
                    })?;
                }
                let candidates = service_index.candidates_ref(&request.target);
                if candidates.is_empty() {
                    failed += 1;
                    if full_trace {
                        sink.on_event(&TraceEvent::RequestFailed {
                            tick,
                            request_id: request.id,
                            reason: FailureReason::NoCandidates,
                        })?;
                    }
                    continue;
                }

                let routing_ctx = RoutingContext {
                    topology: &self.topology,
                    graph: &self.graph,
                    runtime: &observed,
                    tick,
                };
                let decision = policy.choose(&routing_ctx, &request, candidates);
                if full_trace {
                    sink.on_event(&TraceEvent::RouteChosen {
                        tick,
                        request_id: request.id,
                        algorithm: policy.name().to_string(),
                        candidates: decision.candidates.clone(),
                        chosen: decision.chosen.clone(),
                        score: decision.score,
                        explanations: decision.explanations.clone(),
                    })?;
                }

                if !candidates.contains(&decision.chosen) {
                    failed += 1;
                    if full_trace {
                        sink.on_event(&TraceEvent::RequestFailed {
                            tick,
                            request_id: request.id,
                            reason: FailureReason::NoCandidates,
                        })?;
                    }
                    continue;
                }

                let path_latency_ms = if full_trace {
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
                    path.total_latency_ms
                } else {
                    let Some(totals) = self.graph.shortest_path_totals(&request.source, &decision.chosen) else {
                        failed += 1;
                        continue;
                    };
                    totals.total_latency_ms
                };

                increment_inflight(&mut state.runtime, &decision.chosen);
                let mut latency_ms = path_latency_ms
                    + service_processing_latency(&self.topology, &decision.chosen);
                latency_ms += model_downstream_calls(
                    &self.topology,
                    &self.graph,
                    &state.runtime,
                    &request,
                    &decision.chosen,
                    tick,
                    sink,
                    0,
                    &mut BTreeSet::new(),
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
            if full_trace {
                sink.on_event(&TraceEvent::TickCompleted { tick })?;
            }
        }

        let active_at_end = state.active_requests.len() as u64;
        let measurement_ticks = drain_start.saturating_sub(experiment.warmup_ticks);
        let policy_variant = experiment.policy.clone();
        let policy_family = if policy_variant.starts_with("score") {
            "score-v1".to_string()
        } else {
            policy_variant.clone()
        };
        let summary = SimulationSummary::from_samples(
            experiment.id.to_string(),
            policy_family,
            policy_variant,
            self.topology.name.clone(),
            experiment.scenario.clone(),
            experiment.seed,
            measurement_ticks,
            experiment.warmup_ticks,
            experiment.drain_ticks,
            experiment.requests_per_tick,
            created,
            completed,
            failed,
            active_at_end,
            latencies,
        );
        let mut summary = summary;
        // Save utilization/pressure summaries to `extra`.
        if meas_ticks > 0 {
            let denom = meas_ticks as f64 * (state.runtime.nodes.len().max(1) as f64);
            summary
                .extra
                .insert("node_util_max".to_string(), node_util_max);
            summary
                .extra
                .insert("host_pressure_max".to_string(), host_pressure_max);
            summary
                .extra
                .insert("db_pressure_max".to_string(), db_pressure_max);
            summary
                .extra
                .insert("cache_miss_risk_max".to_string(), cache_miss_max);
            summary
                .extra
                .insert("broker_lag_max".to_string(), broker_lag_max);
            summary
                .extra
                .insert("error_rate_max".to_string(), err_rate_max);
            summary
                .extra
                .insert("node_util_mean".to_string(), node_util_sum / denom);
            summary
                .extra
                .insert("host_pressure_mean".to_string(), host_pressure_sum / denom);
            summary.extra.insert(
                "util_over_1_frac".to_string(),
                util_over_1_ticks as f64 / meas_ticks as f64,
            );
            let svc_count = logical_services.len().max(1) as f64;
            // Note: replica_cv_sum sums over (tick, service) where service had >=2 replicas.
            summary.extra.insert(
                "replica_imbalance_cv_mean".to_string(),
                replica_cv_sum / (meas_ticks as f64 * svc_count),
            );
            summary
                .extra
                .insert("replica_imbalance_cv_max".to_string(), replica_cv_max);
            summary
                .extra
                .insert("fail_burstiness_max".to_string(), fail_burstiness_max);
            summary
                .extra
                .insert("drain_completed".to_string(), drain_completed as f64);
            summary
                .extra
                .insert("drain_failed".to_string(), drain_failed as f64);
            let total_completed = completed + drain_completed;
            let drain_frac = if total_completed == 0 {
                0.0
            } else {
                drain_completed as f64 / total_completed as f64
            };
            summary
                .extra
                .insert("drain_completion_frac".to_string(), drain_frac);
        }
        if full_trace {
            sink.on_event(&TraceEvent::SimulationCompleted {
                experiment_id: experiment.id.to_string(),
                created,
                completed,
                failed,
            })?;
        }
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
    depth: usize,
    visited: &mut BTreeSet<LogicalServiceId>,
) -> anyhow::Result<f64> {
    if depth >= 3 {
        return Ok(0.0);
    }
    if let Some(service_id) = topology.service_of_node(chosen) {
        if !visited.insert(service_id.clone()) {
            return Ok(0.0);
        }
    }

    let mut total = 0.0;
    for dependency in topology.dependencies_for_instance(chosen) {
        for target in topology.resolve_dependency(chosen, dependency) {
            let path_latency = graph
                .shortest_path_totals(chosen, &target)
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
            let mut latency = dependency.probability
                * mode_factor
                * (dependency.base_operation_latency_ms + path_latency + pressure_penalty);
            if matches!(&dependency.target, DependencyTarget::LogicalService(_)) {
                latency += dependency.probability
                    * mode_factor
                    * model_downstream_calls(
                        topology,
                        graph,
                        runtime,
                        request,
                        &target,
                        tick,
                        sink,
                        depth + 1,
                        visited,
                    )?;
            }
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
    scenario: &str,
) {
    let wave = ((tick % 100) as f64 / 100.0).sin().abs();
    for node in &topology.nodes {
        let entry = runtime.nodes.entry(node.id.clone()).or_default();
        match &node.kind {
            NodeKind::Database(_) => {
                let scenario_boost = match scenario {
                    "db-overloaded" => 0.35,
                    "zone-burst" if node.zone.as_ref().map(|z| z.as_str()) == Some("zone-b") => {
                        0.20
                    }
                    _ => 0.0,
                };
                entry.db_pressure =
                    (0.10 + scenario_boost + entry.utilization * 0.8 + wave * 0.08).min(2.5)
            }
            NodeKind::Cache(spec) => {
                let scenario_boost = match scenario {
                    "cache-degraded" => 0.20,
                    "zone-burst" if node.zone.as_ref().map(|z| z.as_str()) == Some("zone-b") => {
                        0.10
                    }
                    _ => 0.0,
                };
                entry.cache_miss_risk = ((1.0 - spec.base_hit_rate)
                    + scenario_boost
                    + entry.utilization * 0.25
                    + wave * 0.04)
                    .min(1.0)
            }
            NodeKind::Broker(spec) => {
                let scenario_boost = match scenario {
                    "broker-lag" => 0.35,
                    _ => 0.0,
                };
                entry.broker_lag = (spec.base_lag_ms / 100.0
                    + scenario_boost
                    + entry.utilization * 0.6
                    + wave * 0.05)
                    .min(2.0)
            }
            NodeKind::Service(_) => {
                if scenario == "partial-failure" && node.id.to_string().ends_with("-3") {
                    entry.error_rate = (entry.error_rate + 0.05 + wave * 0.02).min(0.95);
                    entry.utilization = (entry.utilization + 0.15).min(2.0);
                }
                if scenario == "zone-burst"
                    && node.zone.as_ref().map(|z| z.as_str()) == Some("zone-b")
                {
                    entry.utilization = (entry.utilization + 0.35 + wave * 0.15).min(3.0);
                    entry.error_rate = (entry.error_rate + 0.03 + wave * 0.03).min(0.95);
                }
            }
            _ => {}
        }
    }
}

fn apply_observability_noise(snapshot: &mut RuntimeSnapshot, rng: &mut ChaCha8Rng, stddev: f64) {
    if stddev <= 0.0 {
        return;
    }
    for state in snapshot.nodes.values_mut() {
        // Light, bounded noise; enough to prevent perfect circularity, not enough to dominate signal.
        let mut n = || rng.gen_range(-stddev..stddev);
        state.utilization = (state.utilization + n()).max(0.0);
        state.db_pressure = (state.db_pressure + n()).max(0.0);
        state.cache_miss_risk = (state.cache_miss_risk + n()).clamp(0.0, 1.0);
        state.broker_lag = (state.broker_lag + n()).max(0.0);
        state.error_rate = (state.error_rate + n()).clamp(0.0, 0.95);
        state.host_pressure = (state.host_pressure + n()).max(0.0);
    }
}
