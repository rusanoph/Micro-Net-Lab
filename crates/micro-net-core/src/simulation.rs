//! Simulation model, request lifecycle and result types.

use crate::ids::*;
use crate::metrics::RuntimeSnapshot;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Logical request flowing through the simulator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Stable request identifier.
    pub id: RequestId,
    /// Tick at which the request was created.
    pub created_at: Tick,
    /// Source node, usually a client or gateway.
    pub source: NodeId,
    /// Target logical service. Routing chooses a concrete service instance for this value.
    pub target: LogicalServiceId,
    /// Request class/profile.
    pub class: RequestClassId,
    /// Optional deadline in ticks.
    pub timeout_budget_ticks: Option<u64>,
}

/// Experiment configuration that must be persisted for reproducibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentSpec {
    /// Schema version of the artifact.
    pub schema_version: String,
    /// Experiment identifier.
    pub id: ExperimentId,
    /// Random seed used by workload, policies and failure injectors.
    pub seed: u64,
    /// Number of simulation ticks to run.
    pub duration_ticks: Tick,
    /// Policy name used by the CLI/runner.
    pub policy: String,
    /// Requests generated per tick.
    pub requests_per_tick: u64,
    /// Source node used by the simple constant workload.
    pub source: NodeId,
    /// Target logical services used by the workload.
    pub targets: Vec<LogicalServiceId>,
}

/// In-flight request in the discrete-event simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRequest {
    /// Original request.
    pub request: Request,
    /// Concrete chosen service instance.
    pub chosen: NodeId,
    /// Tick at which the request should finish.
    pub complete_at: Tick,
    /// Total modeled latency.
    pub latency_ms: f64,
    /// Whether the request is expected to fail.
    pub will_fail: bool,
}

/// Mutable simulation state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationState {
    /// Current tick.
    pub tick: Tick,
    /// Next request id to allocate.
    pub next_request_id: RequestId,
    /// Active requests.
    #[serde(default)]
    pub active_requests: Vec<ActiveRequest>,
    /// Current runtime metric snapshot.
    pub runtime: RuntimeSnapshot,
}

/// Final experiment summary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimulationSummary {
    /// Schema version of the summary artifact.
    pub schema_version: String,
    /// Experiment id.
    pub experiment_id: String,
    /// Policy name.
    pub policy: String,
    /// Seed.
    pub seed: u64,
    /// Total created logical requests.
    pub created: u64,
    /// Completed requests.
    pub completed: u64,
    /// Failed requests.
    pub failed: u64,
    /// Requests still active at the end of the run.
    pub active_at_end: u64,
    /// Success rate in `[0, 1]`.
    pub success_rate: f64,
    /// Error rate in `[0, 1]`.
    pub error_rate: f64,
    /// Average latency in milliseconds.
    pub avg_latency_ms: f64,
    /// 95th percentile latency in milliseconds.
    pub p95_latency_ms: f64,
    /// 99th percentile latency in milliseconds.
    pub p99_latency_ms: f64,
    /// Throughput in completed requests per tick.
    pub throughput_per_tick: f64,
    /// Additional scalar values.
    #[serde(default)]
    pub extra: BTreeMap<String, f64>,
}

impl SimulationSummary {
    /// Builds a summary from observed counters and latency samples.
    pub fn from_samples(
        experiment_id: String,
        policy: String,
        seed: u64,
        duration_ticks: Tick,
        created: u64,
        completed: u64,
        failed: u64,
        active_at_end: u64,
        mut latencies: Vec<f64>,
    ) -> Self {
        latencies.sort_by(|a, b| a.total_cmp(b));
        let avg = if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        };
        let p = |q: f64, xs: &Vec<f64>| -> f64 {
            if xs.is_empty() {
                return 0.0;
            }
            let idx = ((xs.len() as f64 - 1.0) * q).round() as usize;
            xs[idx.min(xs.len() - 1)]
        };
        let total_finished = completed + failed;
        Self {
            schema_version: "0.1".to_string(),
            experiment_id,
            policy,
            seed,
            created,
            completed,
            failed,
            active_at_end,
            success_rate: if total_finished == 0 {
                0.0
            } else {
                completed as f64 / total_finished as f64
            },
            error_rate: if total_finished == 0 {
                0.0
            } else {
                failed as f64 / total_finished as f64
            },
            avg_latency_ms: avg,
            p95_latency_ms: p(0.95, &latencies),
            p99_latency_ms: p(0.99, &latencies),
            throughput_per_tick: if duration_ticks == 0 {
                0.0
            } else {
                completed as f64 / duration_ticks as f64
            },
            extra: BTreeMap::new(),
        }
    }
}

/// Error reason associated with failed logical requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    /// No concrete service instance exists for the target logical service.
    NoCandidates,
    /// No network path exists between the caller and selected backend.
    NoPath,
    /// Synthetic backend failure.
    BackendFailure,
    /// Synthetic timeout.
    Timeout,
}
