//! `GraphBackend` implementation backed by `petgraph`.

use micro_net_core::{Edge, EdgeId, GraphBackend, Node, NodeId, Path, PathTotals, TopologySpec};
use petgraph::algo::astar;
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::stable_graph::StableDiGraph;
use petgraph::visit::EdgeRef;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::sync::{Mutex, OnceLock};

/// Concrete graph backend backed by `petgraph::StableDiGraph`.
///
/// Backend-specific `NodeIndex`/`EdgeIndex` values never escape this type.
/// Public APIs use stable domain ids from `micro-net-core`.
pub struct PetgraphBackend {
    topology: TopologySpec,
    graph: StableDiGraph<NodeId, EdgeId>,
    node_to_index: BTreeMap<NodeId, NodeIndex>,
    edge_to_index: BTreeMap<EdgeId, EdgeIndex>,
    pos_by_node_id: BTreeMap<NodeId, usize>,
    totals_precomputed: Vec<PathTotals>,
    path_cache: OnceLock<Mutex<BTreeMap<(NodeId, NodeId), Path>>>,
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

        let totals_precomputed =
            precompute_shortest_totals(&topology, &graph, &node_to_index);
        let mut pos_by_node_id = BTreeMap::new();
        for (pos, node) in topology.nodes.iter().enumerate() {
            pos_by_node_id.insert(node.id.clone(), pos);
        }

        Ok(Self {
            topology,
            graph,
            node_to_index,
            edge_to_index,
            pos_by_node_id,
            totals_precomputed,
            path_cache: OnceLock::new(),
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

    fn path_cache(&self) -> &Mutex<BTreeMap<(NodeId, NodeId), Path>> {
        self.path_cache.get_or_init(|| Mutex::new(BTreeMap::new()))
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
        let key = (from.clone(), to.clone());
        if let Ok(cache) = self.path_cache().lock() {
            if let Some(found) = cache.get(&key) {
                return Some(found.clone());
            }
        }

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
        let path = Path {
            nodes: node_ids,
            edges: edge_ids,
            total_latency_ms: latency,
            total_cost: cost,
        };
        if let Ok(mut cache) = self.path_cache().lock() {
            cache.insert(key.clone(), path.clone());
        }
        Some(path)
    }

    fn shortest_path_totals(&self, from: &NodeId, to: &NodeId) -> Option<PathTotals> {
        let from_pos = *self.pos_by_node_id.get(from)?;
        let to_pos = *self.pos_by_node_id.get(to)?;
        let n = self.topology.nodes.len();
        let idx = from_pos * n + to_pos;
        let totals = *self.totals_precomputed.get(idx)?;
        if totals.total_latency_ms.is_finite() {
            Some(totals)
        } else {
            None
        }
    }

    fn k_paths(&self, from: &NodeId, to: &NodeId, k: usize) -> Vec<Path> {
        if k == 0 {
            return Vec::new();
        }
        self.shortest_path(from, to).into_iter().collect()
    }
}

fn precompute_shortest_totals(
    topology: &TopologySpec,
    graph: &StableDiGraph<NodeId, EdgeId>,
    node_to_index: &BTreeMap<NodeId, NodeIndex>,
) -> Vec<PathTotals> {
    let indices: Vec<(NodeId, NodeIndex)> = topology
        .nodes
        .iter()
        .map(|n| {
            let idx = *node_to_index.get(&n.id).expect("node index must exist");
            (n.id.clone(), idx)
        })
        .collect();
    let pos_by_index: HashMap<NodeIndex, usize> =
        indices.iter().enumerate().map(|(i, (_, idx))| (*idx, i)).collect();
    let n = indices.len();
    if n == 0 {
        return Vec::new();
    }

    let mut out = vec![
        PathTotals {
            total_latency_ms: f64::INFINITY,
            total_cost: f64::INFINITY,
        };
        n * n
    ];

    for (source_id, source_idx) in &indices {
        let mut dist: Vec<f64> = vec![f64::INFINITY; n];
        let mut prev_edge: Vec<Option<EdgeId>> = vec![None; n];
        let mut prev_node: Vec<Option<usize>> = vec![None; n];
        let mut heap: BinaryHeap<(Reverse<F64Ord>, NodeId, usize)> = BinaryHeap::new();

        let source_pos = pos_by_index.get(source_idx).copied().unwrap_or(0);
        dist[source_pos] = 0.0;
        heap.push((Reverse(F64Ord(0.0)), source_id.clone(), source_pos));

        while let Some((Reverse(F64Ord(d)), _u_id, u_pos)) = heap.pop() {
            if d > dist[u_pos] + 1e-12 {
                continue;
            }
            let u_idx = indices[u_pos].1;
            for edge in graph.edges(u_idx) {
                let edge_id = edge.weight();
                let Some(e) = topology.edge(edge_id) else {
                    continue;
                };
                let v_idx = edge.target();
                let Some(v_pos) = pos_by_index.get(&v_idx).copied() else {
                    continue;
                };
                let nd = d + e.latency_ms;
                if nd < dist[v_pos] - 1e-12 {
                    dist[v_pos] = nd;
                    prev_node[v_pos] = Some(u_pos);
                    prev_edge[v_pos] = Some(e.id.clone());
                    heap.push((Reverse(F64Ord(nd)), indices[v_pos].0.clone(), v_pos));
                } else if (nd - dist[v_pos]).abs() <= 1e-12 {
                    // Deterministic tie-break: prefer predecessor with smaller NodeId.
                    let current_prev = prev_node[v_pos]
                        .and_then(|p| indices.get(p).map(|(id, _)| id.clone()));
                    if current_prev.as_ref().map(|id| id > &indices[u_pos].0).unwrap_or(true) {
                        prev_node[v_pos] = Some(u_pos);
                        prev_edge[v_pos] = Some(e.id.clone());
                    }
                }
            }
        }

        // Compute cost along the chosen predecessor tree with memoization.
        let mut cost_memo: Vec<Option<f64>> = vec![None; n];
        cost_memo[source_pos] = Some(0.0);
        for target_pos in 0..n {
            if !dist[target_pos].is_finite() {
                continue;
            }
            let cost = compute_cost(topology, &prev_node, &prev_edge, &mut cost_memo, target_pos);
            if let Some(total_cost) = cost {
                let idx = source_pos * n + target_pos;
                out[idx] = PathTotals {
                    total_latency_ms: dist[target_pos],
                    total_cost: total_cost,
                };
            }
        }
    }

    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct F64Ord(f64);

impl Eq for F64Ord {}

impl PartialOrd for F64Ord {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for F64Ord {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

fn compute_cost(
    topology: &TopologySpec,
    prev_node: &[Option<usize>],
    prev_edge: &[Option<EdgeId>],
    memo: &mut [Option<f64>],
    pos: usize,
) -> Option<f64> {
    if let Some(v) = memo[pos] {
        return Some(v);
    }
    let p = prev_node[pos]?;
    let edge_id = prev_edge[pos].as_ref()?;
    let edge = topology.edge(edge_id)?;
    let base = compute_cost(topology, prev_node, prev_edge, memo, p)?;
    let v = base + edge.cost;
    memo[pos] = Some(v);
    Some(v)
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
