//! Shared configuration enums used by CLI and generators.

use serde::{Deserialize, Serialize};

/// Topology generator kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TopologyKind {
    /// Gateway connected to all service instances and resources.
    Star,
    /// Service instances form a ring in addition to gateway/resource links.
    Ring,
    /// Dense directed mesh.
    FullMesh,
    /// Sparse random directed graph.
    RandomSparse,
}
