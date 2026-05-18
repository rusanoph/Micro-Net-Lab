//! Core domain model and extension ports for `micro-net-lab-rs`.
//!
//! This crate deliberately contains no dependency on Petgraph, Tokio, Docker,
//! Kubernetes, WebSocket, or any concrete output backend. Concrete adapters live
//! in sibling crates and depend on this crate, not the other way around.

pub mod config;
pub mod dependency;
pub mod domain;
pub mod graph;
pub mod ids;
pub mod metrics;
pub mod routing;
pub mod simulation;
pub mod topology;
pub mod trace;
pub mod workload;

pub use config::*;
pub use dependency::*;
pub use domain::*;
pub use graph::*;
pub use ids::*;
pub use metrics::*;
pub use routing::*;
pub use simulation::*;
pub use topology::*;
pub use trace::*;
pub use workload::*;
