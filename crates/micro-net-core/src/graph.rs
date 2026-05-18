//! Graph abstraction used by policies and simulation code.
//!
//! Concrete graph libraries must remain behind this trait. Domain code uses
//! stable `NodeId`/`EdgeId`, never backend-specific indexes.

use crate::domain::{Edge, Node};
use crate::ids::{EdgeId, NodeId};

/// Path between two nodes with aggregate link metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    /// Nodes traversed by the path, including source and target.
    pub nodes: Vec<NodeId>,
    /// Edge identifiers traversed by the path.
    pub edges: Vec<EdgeId>,
    /// Sum of link latencies.
    pub total_latency_ms: f64,
    /// Sum of abstract link costs.
    pub total_cost: f64,
}

/// Path aggregate metrics without allocating node/edge lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathTotals {
    pub total_latency_ms: f64,
    pub total_cost: f64,
}

/// Backend-neutral graph port.
pub trait GraphBackend: Send + Sync {
    /// Returns a node by stable domain identifier.
    fn node(&self, id: &NodeId) -> Option<&Node>;

    /// Returns an edge by stable domain identifier.
    fn edge(&self, id: &EdgeId) -> Option<&Edge>;

    /// Returns outgoing neighbor node ids.
    fn neighbors(&self, id: &NodeId) -> Vec<NodeId>;

    /// Returns outgoing edge ids.
    fn edges_from(&self, id: &NodeId) -> Vec<EdgeId>;

    /// Returns the cheapest path by latency, if it exists.
    fn shortest_path(&self, from: &NodeId, to: &NodeId) -> Option<Path>;

    /// Returns cheapest path aggregate metrics without allocating path vectors.
    fn shortest_path_totals(&self, from: &NodeId, to: &NodeId) -> Option<PathTotals> {
        self.shortest_path(from, to).map(|p| PathTotals {
            total_latency_ms: p.total_latency_ms,
            total_cost: p.total_cost,
        })
    }

    /// Returns up to `k` path candidates. Implementations may initially return
    /// only the best path and later evolve into real k-shortest path algorithms.
    fn k_paths(&self, from: &NodeId, to: &NodeId, k: usize) -> Vec<Path>;
}
