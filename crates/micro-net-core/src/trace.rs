//! Trace-first observability model.

use crate::ids::*;
use crate::metrics::MetricSnapshot;
use crate::routing::CandidateScoreExplanation;
use crate::simulation::FailureReason;
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Event emitted by the simulation engine. JSONL traces are built from this enum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TraceEvent {
    /// Experiment started.
    SimulationStarted {
        schema_version: String,
        experiment_id: String,
        seed: u64,
        policy: String,
    },
    /// Tick started.
    TickStarted { tick: Tick },
    /// Logical request created.
    RequestCreated {
        tick: Tick,
        request_id: RequestId,
        source: NodeId,
        target: LogicalServiceId,
    },
    /// Concrete backend selected for a logical service request.
    RouteChosen {
        tick: Tick,
        request_id: RequestId,
        algorithm: String,
        candidates: Vec<NodeId>,
        chosen: NodeId,
        score: Option<f64>,
        explanations: Vec<CandidateScoreExplanation>,
    },
    /// Link was traversed by a request.
    EdgeTraversed {
        tick: Tick,
        request_id: RequestId,
        edge_id: EdgeId,
        from: NodeId,
        to: NodeId,
        latency_ms: f64,
    },
    /// Concrete downstream dependency was touched.
    DependencyTouched {
        tick: Tick,
        request_id: RequestId,
        caller: NodeId,
        dependency: LogicalDependencyId,
        target: NodeId,
        latency_ms: f64,
    },
    /// Runtime state of one node changed.
    NodeStateChanged {
        tick: Tick,
        node_id: NodeId,
        metrics: MetricSnapshot,
    },
    /// Request completed successfully.
    RequestCompleted {
        tick: Tick,
        request_id: RequestId,
        chosen: NodeId,
        latency_ms: f64,
    },
    /// Request failed.
    RequestFailed {
        tick: Tick,
        request_id: RequestId,
        reason: FailureReason,
    },
    /// Tick completed.
    TickCompleted { tick: Tick },
    /// Experiment completed.
    SimulationCompleted {
        experiment_id: String,
        created: u64,
        completed: u64,
        failed: u64,
    },
}

/// Trace verbosity level requested by an event sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceLevel {
    /// No tracing. Engine should avoid trace-only work in hot paths.
    None,
    /// Full tracing (default).
    Full,
}

/// Side-effect boundary for trace events.
pub trait EventSink {
    /// Requested trace verbosity. Defaults to full tracing.
    fn trace_level(&self) -> TraceLevel {
        TraceLevel::Full
    }

    /// Handles one event.
    fn on_event(&mut self, event: &TraceEvent) -> anyhow::Result<()>;
}

/// Event sink that discards all events.
#[derive(Debug, Default)]
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn trace_level(&self) -> TraceLevel {
        TraceLevel::None
    }

    fn on_event(&mut self, _event: &TraceEvent) -> anyhow::Result<()> {
        Ok(())
    }
}

/// In-memory event sink useful for tests and replay fixtures.
#[derive(Debug, Default)]
pub struct InMemoryTraceSink {
    /// Captured events.
    pub events: Vec<TraceEvent>,
}

impl EventSink for InMemoryTraceSink {
    fn trace_level(&self) -> TraceLevel {
        TraceLevel::Full
    }

    fn on_event(&mut self, event: &TraceEvent) -> anyhow::Result<()> {
        self.events.push(event.clone());
        Ok(())
    }
}

/// JSONL trace sink.
pub struct JsonlTraceSink<W: Write> {
    writer: W,
}

impl<W: Write> JsonlTraceSink<W> {
    /// Creates a new JSONL sink over an arbitrary writer.
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> EventSink for JsonlTraceSink<W> {
    fn trace_level(&self) -> TraceLevel {
        TraceLevel::Full
    }

    fn on_event(&mut self, event: &TraceEvent) -> anyhow::Result<()> {
        serde_json::to_writer(&mut self.writer, event)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }
}
