//! Logical dependency profiles and concrete dependency bindings.
//!
//! A logical service declares what it needs (for example `payments -> payments-db`).
//! Concrete service instances may reach that logical dependency through different
//! links, zones, hosts or concrete resource nodes.

use crate::ids::*;
use serde::{Deserialize, Serialize};

/// Logical behavior of a service, shared by all of its service instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalServiceSpec {
    /// Logical service identifier.
    pub id: LogicalServiceId,
    /// Dependencies used by this service during request processing.
    #[serde(default)]
    pub dependencies: Vec<LogicalDependency>,
}

/// One logical dependency used by a service profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogicalDependency {
    /// Stable dependency identifier, unique inside an experiment.
    pub id: LogicalDependencyId,
    /// Target dependency at logical/resource level.
    pub target: DependencyTarget,
    /// Dependency role in the service behavior.
    pub kind: DependencyKind,
    /// Whether the call is synchronous, asynchronous or fire-and-forget.
    pub call_mode: CallMode,
    /// Probability that a request of a matching class touches this dependency.
    pub probability: f64,
    /// Baseline logical operation latency before network and target runtime pressure.
    pub base_operation_latency_ms: f64,
}

/// Logical dependency target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum DependencyTarget {
    /// Another logical service.
    LogicalService(LogicalServiceId),
    /// Logical database resource.
    Database(LogicalResourceId),
    /// Logical cache resource.
    Cache(LogicalResourceId),
    /// Logical broker resource.
    Broker(LogicalResourceId),
    /// Logical external API resource.
    ExternalApi(LogicalResourceId),
}

/// Dependency behavior kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    /// Cache lookup or mutation.
    CacheLookup,
    /// Database read.
    DatabaseRead,
    /// Database write.
    DatabaseWrite,
    /// Broker publish.
    BrokerPublish,
    /// Broker consume.
    BrokerConsume,
    /// Synchronous service-to-service call.
    ServiceCall,
    /// External API call.
    ExternalCall,
}

/// Logical call mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallMode {
    /// Caller waits for the dependency result.
    Synchronous,
    /// Caller emits work and does not include full latency in critical path.
    Asynchronous,
    /// Best-effort operation.
    FireAndForget,
}

/// Concrete binding from one service instance and logical dependency to physical nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyBinding {
    /// Caller node, usually a concrete service instance.
    pub caller: NodeId,
    /// Logical dependency being resolved.
    pub dependency: LogicalDependencyId,
    /// Concrete target nodes reachable from the caller for this dependency.
    pub targets: Vec<NodeId>,
}
