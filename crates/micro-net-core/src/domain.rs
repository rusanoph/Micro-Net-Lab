//! Typed node and edge domain model.

use crate::ids::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Domain node in the typed microservice topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    /// Stable node identifier used in configs, traces and reports.
    pub id: NodeId,
    /// Human-readable label.
    pub label: String,
    /// Optional placement zone. Different zones usually imply different network paths.
    pub zone: Option<ZoneId>,
    /// Optional physical host. Multiple service instances can share one host.
    pub host: Option<HostId>,
    /// Typed node payload.
    pub kind: NodeKind,
    /// Free-form metadata preserved in JSON artifacts.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl Node {
    /// Returns `true` if this node is a service instance of the given logical service.
    pub fn is_instance_of(&self, service: &LogicalServiceId) -> bool {
        match &self.kind {
            NodeKind::Service(spec) => &spec.logical_service == service,
            _ => false,
        }
    }
}

/// Typed node kind. Service instances, databases, caches and brokers expose
/// different parameters and runtime metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "spec", rename_all = "snake_case")]
pub enum NodeKind {
    /// Synthetic workload source.
    Client,
    /// Entry point that routes requests to logical services.
    Gateway,
    /// Concrete replica of a logical service.
    Service(ServiceInstanceSpec),
    /// Database node or database cluster abstraction.
    Database(DatabaseSpec),
    /// Cache node or cache cluster abstraction.
    Cache(CacheSpec),
    /// Broker node or broker cluster abstraction.
    Broker(BrokerSpec),
    /// External dependency outside of the simulated ownership boundary.
    ExternalApi(ExternalApiSpec),
}

/// Parameters of a concrete service replica.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInstanceSpec {
    /// Logical service implemented by this replica.
    pub logical_service: LogicalServiceId,
    /// Concrete instance identifier.
    pub instance_id: ServiceInstanceId,
    /// Replica number inside the logical service.
    pub replica_id: usize,
    /// Approximate capacity used by features and utilization metrics.
    pub base_capacity_rps: f64,
    /// Baseline service processing latency without network and downstream calls.
    pub base_processing_latency_ms: f64,
}

/// Database engine family.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseEngine {
    /// PostgreSQL-like transactional database.
    Postgres,
    /// ClickHouse-like analytical database.
    ClickHouse,
    /// Generic SQL or NoSQL database when the precise engine is not important.
    Generic,
}

/// Parameters of a database dependency node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseSpec {
    /// Logical resource name referenced by service dependency profiles.
    pub logical_resource: LogicalResourceId,
    /// Database engine type.
    pub engine: DatabaseEngine,
    /// Connection budget used by pressure features.
    pub max_connections: usize,
    /// Baseline query latency.
    pub base_query_latency_ms: f64,
}

/// Parameters of a cache dependency node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSpec {
    /// Logical cache resource name referenced by service dependency profiles.
    pub logical_resource: LogicalResourceId,
    /// Memory budget used by pressure features.
    pub max_memory_mb: usize,
    /// Baseline hit rate in `[0, 1]`.
    pub base_hit_rate: f64,
    /// Baseline cache operation latency.
    pub base_latency_ms: f64,
}

/// Parameters of a broker dependency node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerSpec {
    /// Logical broker resource name referenced by service dependency profiles.
    pub logical_resource: LogicalResourceId,
    /// Partition count used by hotspot features.
    pub partitions: usize,
    /// Baseline publish/consume lag.
    pub base_lag_ms: f64,
}

/// Parameters of an external API dependency node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalApiSpec {
    /// Logical external resource name referenced by service dependency profiles.
    pub logical_resource: LogicalResourceId,
    /// Baseline remote call latency.
    pub base_latency_ms: f64,
    /// Baseline remote error probability in `[0, 1]`.
    pub base_error_rate: f64,
}

/// Directed edge/link between two topology nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    /// Stable edge identifier.
    pub id: EdgeId,
    /// Source node.
    pub from: NodeId,
    /// Destination node.
    pub to: NodeId,
    /// Baseline link latency.
    pub latency_ms: f64,
    /// Link capacity in logical requests per second.
    pub capacity_rps: f64,
    /// Baseline link error probability in `[0, 1]`.
    pub error_rate: f64,
    /// Abstract routing cost. It may differ from latency.
    pub cost: f64,
    /// Optional free-form metadata.
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl Edge {
    /// Creates a directed edge with common defaults.
    pub fn new(
        id: impl Into<EdgeId>,
        from: impl Into<NodeId>,
        to: impl Into<NodeId>,
        latency_ms: f64,
    ) -> Self {
        Self {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            latency_ms,
            capacity_rps: 10_000.0,
            error_rate: 0.0,
            cost: latency_ms,
            metadata: BTreeMap::new(),
        }
    }
}
