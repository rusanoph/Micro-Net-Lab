//! Serializable topology specification and service/dependency indexes.

use crate::dependency::{
    DependencyBinding, DependencyTarget, LogicalDependency, LogicalServiceSpec,
};
use crate::domain::{Edge, Node, NodeKind};
use crate::ids::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Serializable topology artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologySpec {
    /// Schema version used for forward-compatible artifacts.
    pub schema_version: String,
    /// Topology identifier or human-readable name.
    pub name: String,
    /// Logical service definitions and their dependency profiles.
    #[serde(default)]
    pub logical_services: Vec<LogicalServiceSpec>,
    /// Typed topology nodes.
    pub nodes: Vec<Node>,
    /// Directed topology edges.
    pub edges: Vec<Edge>,
    /// Optional explicit dependency bindings from service instances to resources.
    #[serde(default)]
    pub dependency_bindings: Vec<DependencyBinding>,
}

impl TopologySpec {
    /// Returns a node by id.
    pub fn node(&self, id: &NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// Returns an edge by id.
    pub fn edge(&self, id: &EdgeId) -> Option<&Edge> {
        self.edges.iter().find(|e| &e.id == id)
    }

    /// Builds an index from logical services to concrete service instance nodes.
    pub fn service_index(&self) -> ServiceIndex {
        ServiceIndex::from_topology(self)
    }

    /// Finds a logical service spec.
    pub fn logical_service(&self, id: &LogicalServiceId) -> Option<&LogicalServiceSpec> {
        self.logical_services.iter().find(|s| &s.id == id)
    }

    /// Returns the logical service implemented by a concrete service node.
    pub fn service_of_node(&self, node: &NodeId) -> Option<&LogicalServiceId> {
        match &self.node(node)?.kind {
            NodeKind::Service(spec) => Some(&spec.logical_service),
            _ => None,
        }
    }

    /// Returns logical dependencies of a concrete service instance.
    pub fn dependencies_for_instance(&self, node: &NodeId) -> Vec<&LogicalDependency> {
        let Some(service_id) = self.service_of_node(node) else {
            return Vec::new();
        };
        self.logical_service(service_id)
            .map(|svc| svc.dependencies.iter().collect())
            .unwrap_or_default()
    }

    /// Resolves a logical dependency from a concrete caller to concrete candidate target nodes.
    /// Explicit bindings win; otherwise the method falls back to resource/service matching.
    pub fn resolve_dependency(
        &self,
        caller: &NodeId,
        dependency: &LogicalDependency,
    ) -> Vec<NodeId> {
        if let Some(binding) = self
            .dependency_bindings
            .iter()
            .find(|b| &b.caller == caller && b.dependency == dependency.id)
        {
            return binding.targets.clone();
        }

        match &dependency.target {
            DependencyTarget::LogicalService(service) => self.service_index().candidates(service),
            DependencyTarget::Database(resource) => self
                .nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    NodeKind::Database(spec) if &spec.logical_resource == resource => {
                        Some(n.id.clone())
                    }
                    _ => None,
                })
                .collect(),
            DependencyTarget::Cache(resource) => self
                .nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    NodeKind::Cache(spec) if &spec.logical_resource == resource => {
                        Some(n.id.clone())
                    }
                    _ => None,
                })
                .collect(),
            DependencyTarget::Broker(resource) => self
                .nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    NodeKind::Broker(spec) if &spec.logical_resource == resource => {
                        Some(n.id.clone())
                    }
                    _ => None,
                })
                .collect(),
            DependencyTarget::ExternalApi(resource) => self
                .nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    NodeKind::ExternalApi(spec) if &spec.logical_resource == resource => {
                        Some(n.id.clone())
                    }
                    _ => None,
                })
                .collect(),
        }
    }
}

/// Index used by routing policies to find candidates for a target logical service.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceIndex {
    /// Mapping from logical service to concrete service instance node ids.
    #[serde(default)]
    pub by_logical_service: BTreeMap<LogicalServiceId, Vec<NodeId>>,
    /// Mapping from concrete instance id to concrete node id.
    #[serde(default)]
    pub by_instance_id: BTreeMap<ServiceInstanceId, NodeId>,
}

impl ServiceIndex {
    /// Builds a service index from a topology.
    pub fn from_topology(topology: &TopologySpec) -> Self {
        let mut index = Self::default();
        for node in &topology.nodes {
            if let NodeKind::Service(spec) = &node.kind {
                index
                    .by_logical_service
                    .entry(spec.logical_service.clone())
                    .or_default()
                    .push(node.id.clone());
                index
                    .by_instance_id
                    .insert(spec.instance_id.clone(), node.id.clone());
            }
        }
        for nodes in index.by_logical_service.values_mut() {
            nodes.sort();
        }
        index
    }

    /// Returns concrete service instance candidates for a logical service.
    pub fn candidates(&self, service: &LogicalServiceId) -> Vec<NodeId> {
        self.by_logical_service
            .get(service)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns a reference to the candidate list for a logical service.
    pub fn candidates_ref(&self, service: &LogicalServiceId) -> &[NodeId] {
        self.by_logical_service
            .get(service)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}
