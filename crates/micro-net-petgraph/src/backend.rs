//! `GraphBackend` implementation backed by `petgraph`.

use micro_net_core::{Edge, EdgeId, GraphBackend, Node, NodeId, Path, TopologySpec};
use petgraph::algo::astar;
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableDiGraph;
use petgraph::visit::EdgeRef;
use std::collections::BTreeMap;

/// Concrete graph backend backed by `petgraph::StableDiGraph`.
///
/// Backend-specific `NodeIndex`/`EdgeIndex` values never escape this type.
/// Public APIs use stable domain ids from `micro-net-core`.
pub struct PetgraphBackend {
    topology: TopologySpec,
    graph: StableDiGraph<NodeId, EdgeId>,
    node_to_index: BTreeMap<NodeId, NodeIndex>,
    edge_to_index: BTreeMap<EdgeId, EdgeIndex>,
}

impl PetgraphBackend {
    /// Builds a backend from a serializable topology specification.
    pub fn from_topology(topology: TopologySpec) -> anyhow::Result<Self> {
        let mut graph = StableDiGraph::<NodeId, EdgeId>::new();
        let mut node_to_index = BTreeMap::new();
        for node in &topology.nodes {
            let idx = graph.add_node(node.id.clone());
            node_to_index.insert(node.id.clone(), idx);
        }

        let mut edge_to_index = BTreeMap::new();
        for edge in &topology.edges {
            let Some(from) = node_to_index.get(&edge.from).copied() else {
                anyhow::bail!(
                    "edge {} references missing source node {}",
                    edge.id,
                    edge.from
                );
            };
            let Some(to) = node_to_index.get(&edge.to).copied() else {
                anyhow::bail!(
                    "edge {} references missing target node {}",
                    edge.id,
                    edge.to
                );
            };
            let idx = graph.add_edge(from, to, edge.id.clone());
            edge_to_index.insert(edge.id.clone(), idx);
        }

        Ok(Self {
            topology,
            graph,
            node_to_index,
            edge_to_index,
        })
    }

    /// Returns the underlying serializable topology.
    pub fn topology(&self) -> &TopologySpec {
        &self.topology
    }

    fn edge_by_nodes(&self, from: NodeIndex, to: NodeIndex) -> Option<&Edge> {
        self.graph
            .edges(from)
            .find(|e| e.target() == to)
            .and_then(|e| self.topology.edge(e.weight()))
    }
}

impl GraphBackend for PetgraphBackend {
    fn node(&self, id: &NodeId) -> Option<&Node> {
        self.topology.node(id)
    }

    fn edge(&self, id: &EdgeId) -> Option<&Edge> {
        self.topology.edge(id)
    }

    fn neighbors(&self, id: &NodeId) -> Vec<NodeId> {
        let Some(idx) = self.node_to_index.get(id).copied() else {
            return Vec::new();
        };
        self.graph
            .neighbors(idx)
            .filter_map(|n| self.graph.node_weight(n).cloned())
            .collect()
    }

    fn edges_from(&self, id: &NodeId) -> Vec<EdgeId> {
        let Some(idx) = self.node_to_index.get(id).copied() else {
            return Vec::new();
        };
        self.graph.edges(idx).map(|e| e.weight().clone()).collect()
    }

    fn shortest_path(&self, from: &NodeId, to: &NodeId) -> Option<Path> {
        let start = self.node_to_index.get(from).copied()?;
        let goal = self.node_to_index.get(to).copied()?;
        let (_cost, nodes) = astar(
            &self.graph,
            start,
            |finish| finish == goal,
            |edge| {
                self.topology
                    .edge(edge.weight())
                    .map(|e| e.latency_ms)
                    .unwrap_or(f64::INFINITY)
            },
            |_| 0.0,
        )?;

        let mut edge_ids = Vec::new();
        let mut latency = 0.0;
        let mut cost = 0.0;
        for pair in nodes.windows(2) {
            if let Some(edge) = self.edge_by_nodes(pair[0], pair[1]) {
                edge_ids.push(edge.id.clone());
                latency += edge.latency_ms;
                cost += edge.cost;
            }
        }
        let node_ids = nodes
            .into_iter()
            .filter_map(|idx| self.graph.node_weight(idx).cloned())
            .collect();
        Some(Path {
            nodes: node_ids,
            edges: edge_ids,
            total_latency_ms: latency,
            total_cost: cost,
        })
    }

    fn k_paths(&self, from: &NodeId, to: &NodeId, k: usize) -> Vec<Path> {
        if k == 0 {
            return Vec::new();
        }
        self.shortest_path(from, to).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use micro_net_core::{Edge, Node, NodeKind, TopologySpec};
    use std::collections::BTreeMap;

    #[test]
    fn shortest_path_uses_stable_ids() {
        let topology = TopologySpec {
            schema_version: "0.1".into(),
            name: "test".into(),
            logical_services: vec![],
            nodes: vec![
                Node {
                    id: "a".into(),
                    label: "a".into(),
                    zone: None,
                    host: None,
                    kind: NodeKind::Gateway,
                    metadata: BTreeMap::new(),
                },
                Node {
                    id: "b".into(),
                    label: "b".into(),
                    zone: None,
                    host: None,
                    kind: NodeKind::Gateway,
                    metadata: BTreeMap::new(),
                },
            ],
            edges: vec![Edge::new("a-b", "a", "b", 3.0)],
            dependency_bindings: vec![],
        };
        let backend = PetgraphBackend::from_topology(topology).unwrap();
        let path = backend.shortest_path(&"a".into(), &"b".into()).unwrap();
        assert_eq!(path.total_latency_ms, 3.0);
    }
}
