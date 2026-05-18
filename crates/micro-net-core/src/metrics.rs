//! Extensible metrics model.

use crate::ids::NodeId;
use crate::trace::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Namespaced metric identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MetricKey {
    /// Logical metric namespace, for example `node` or `summary`.
    pub namespace: String,
    /// Metric name inside the namespace.
    pub name: String,
}

impl MetricKey {
    /// Creates a new metric key.
    pub fn new(namespace: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

/// Histogram snapshot used in JSON reports.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HistogramSnapshot {
    /// Number of observed samples.
    pub count: u64,
    /// Average value.
    pub avg: f64,
    /// 95th percentile.
    pub p95: f64,
    /// 99th percentile.
    pub p99: f64,
}

/// Runtime metric value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum MetricValue {
    /// Monotonic counter.
    Counter(u64),
    /// Floating point gauge.
    Gauge(f64),
    /// Histogram snapshot.
    Histogram(HistogramSnapshot),
    /// Raw distribution for offline analysis.
    Distribution(Vec<f64>),
}

/// A snapshot of runtime metrics.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricSnapshot {
    /// Metric values keyed by namespaced metric ids.
    #[serde(default)]
    pub values: BTreeMap<MetricKey, MetricValue>,
}

/// Runtime state of one node at a tick.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeRuntimeState {
    /// Number of active requests currently assigned to the node.
    pub inflight: u64,
    /// Queue length approximation.
    pub queue_len: u64,
    /// Number of completed requests.
    pub completed: u64,
    /// Number of failed requests.
    pub failed: u64,
    /// Moving-average latency approximation.
    pub avg_latency_ms: f64,
    /// Current error rate estimate in `[0, 1]`.
    pub error_rate: f64,
    /// Abstract utilization in `[0, +inf)` where `1` means saturated.
    pub utilization: f64,
    /// Database pressure in `[0, +inf)`.
    pub db_pressure: f64,
    /// Cache miss risk in `[0, 1]`.
    pub cache_miss_risk: f64,
    /// Broker lag normalized to a research-friendly scalar.
    pub broker_lag: f64,
    /// Host-level pressure injected by colocation/resource constraints.
    pub host_pressure: f64,
}

/// Snapshot of all node runtime states used by routing policies.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeSnapshot {
    /// Per-node runtime state.
    #[serde(default)]
    pub nodes: BTreeMap<NodeId, NodeRuntimeState>,
}

impl RuntimeSnapshot {
    /// Returns runtime metrics for a node or a zero/default state.
    pub fn node_or_default(&self, node: &NodeId) -> NodeRuntimeState {
        self.nodes.get(node).cloned().unwrap_or_default()
    }
}

/// Collector extension point. Collectors subscribe to trace events and produce snapshots.
pub trait MetricCollector {
    /// Stable collector name.
    fn name(&self) -> &'static str;

    /// Observes a trace event.
    fn on_event(&mut self, event: &TraceEvent);

    /// Returns the current metrics snapshot.
    fn snapshot(&self) -> MetricSnapshot;
}
