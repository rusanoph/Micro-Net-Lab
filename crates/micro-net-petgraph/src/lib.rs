//! Petgraph adapter and topology generators.
//!
//! `micro-net-core` intentionally knows only about the `GraphBackend` trait.
//! This crate is the first concrete implementation of that port.

pub mod backend;
pub mod generators;

pub use backend::*;
pub use generators::*;
