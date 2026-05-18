# HANDOFF: micro-net-lab-rs

This document captures the architectural decisions and implementation context needed to continue the project in later chats or local development sessions.

## Project goal

Build a Rust research framework for deterministic, tick-based simulation of routing/load-balancing policies in typed microservice topologies.

The framework should support:

```text
typed graph topology
topology generators
workload generation
failure/pressure modeling
routing policies
score features
metrics
JSONL trace
CSV/JSON reports
N policies × M seeds batch experiments
future Docker/K8s/Java stub drivers
future external proxy/router drivers
future visualization/replay
```

## Core modeling decisions

### 1. Request abstraction

A request is a logical workload unit, not a TCP packet.

A request may:

```text
arrive at client/gateway
target a LogicalService
be routed to a concrete ServiceInstance
touch downstream DB/cache/broker/external resources
complete/fail/timeout
produce retry behavior in later iterations
```

### 2. LogicalService vs ServiceInstance

`LogicalService` is the abstract service name:

```text
payments
orders
analytics
```

`ServiceInstance` is a concrete replica:

```text
payments-1
payments-2
payments-3
```

Routing policies choose concrete `ServiceInstance` candidates for a target `LogicalService`.

If a `LogicalService` has one instance, routing degenerates into a trivial choice. Meaningful routing/load-balancing experiments require multiple replicas for at least some services.

### 3. Logical dependencies vs concrete bindings

This was a key clarification.

All replicas of the same logical service share the same logical dependency profile.

Example:

```text
payments:
  depends on service-cache
  depends on primary-db
  publishes to events-broker
```

But concrete replicas can have different physical/simulation bindings:

```text
payments-1 -> cache-a -> db-main through low-latency zone-a path
payments-2 -> cache-b -> db-main through medium-latency zone-b path
payments-3 -> cache-c -> db-main through high-latency zone-c path
```

They can also have different host-level pressure:

```text
payments-1 on host-a
payments-2 on host-b
payments-3 colocated with another replica on shared-host
```

So dependency-aware routing does **not** mean different business logic per replica. It means that candidates implementing the same logical service can have different concrete runtime environments, paths, resource bindings and pressure.

### 4. Network topology graph vs service dependency graph

There are two conceptual layers:

```text
Network topology graph:
  concrete nodes and links, latency/capacity/error/cost.

Service dependency graph/profile:
  what logical resources/services each service uses.
```

In the first code, both are represented inside `TopologySpec`, but the concepts are separated through:

```text
LogicalServiceSpec
LogicalDependency
DependencyTarget
DependencyBinding
```

### 5. Dependency-aware score

The score policy should compare candidates using both local and downstream features:

```text
latency
inflight
error_rate
network_cost
host_pressure
downstream_pressure
```

The first implementation has `ScorePolicyV1` with explainable `CandidateScoreExplanation` and `FeatureContribution` payloads in `RouteChosen` events.

## Workspace crates

```text
micro-net-core
```

Pure domain model and traits. Must not depend on Petgraph/Tokio/Docker/K8s/WebSocket/UI.

```text
micro-net-petgraph
```

First graph backend implementation and topology generators. Keeps `NodeIndex`/`EdgeIndex` internal.

```text
micro-net-algorithms
```

Built-in routing policies and features:

```text
RandomPolicy
RoundRobinPolicy
LeastInflightPolicy
ScorePolicyV1
LatencyFeature
InflightFeature
ErrorRateFeature
NetworkCostFeature
DownstreamPressureFeature
HostPressureFeature
```

```text
micro-net-drivers
```

`InMemorySimulationEngine` for deterministic tick-based experiments.

```text
micro-net-executor
```

Sequential/Rayon batch helpers.

```text
micro-net-report
```

JSON/CSV report writers.

```text
micro-net-cli
```

CLI binary `micro-net`.

## Current CLI

Generate topology:

```bash
cargo run -p micro-net-cli -- generate-topology \
  --kind ring \
  --logical-services 3 \
  --replicas-per-service 3 \
  --seed 42 \
  --out topology.json
```

Run one experiment:

```bash
cargo run -p micro-net-cli -- run \
  --topology topology.json \
  --policy score \
  --seed 42 \
  --duration-ticks 100 \
  --requests-per-tick 5 \
  --out runs/score-001
```

Run batch:

```bash
cargo run -p micro-net-cli -- bench \
  --topologies star,ring,full-mesh,random-sparse \
  --policies random,round-robin,least-inflight,score \
  --seeds 3 \
  --parallel 4 \
  --out bench-results
```

## Artifacts

Single run:

```text
experiment.json
topology.json
trace.jsonl
summary.json
metrics.csv
```

Batch run:

```text
aggregate.json
aggregate.csv
experiments/<experiment-id>/...
```

Every JSON artifact has `schema_version = "0.1"` where applicable.

## Extension points

Add new topology:

```text
implement TopologyGenerator
```

Add new policy:

```text
implement RoutingPolicy
```

Add new score feature:

```text
implement Feature
```

Add new metric:

```text
implement MetricCollector
```

Add new graph backend:

```text
implement GraphBackend
```

Add Docker/K8s/stub execution:

```text
add new SimulationDriver-style crate/adapter without changing core domain model
```

## Next implementation steps

1. Run locally:

```bash
cargo fmt
cargo test --workspace
```

2. Fix compiler issues if any. The generation environment did not have `cargo`, so tests could not be run there.

3. Add EWMA policy.

4. Add YAML config:

```bash
micro-net run --config experiment.yaml
```

5. Add explicit failure injectors:

```text
BackendSlowdown
BackendPartialFailure
DbPressure
CacheDegradation
BrokerLag
NetworkDegradation
RetryStorm
```

6. Add real metric collectors:

```text
node_timeseries.csv
edge_timeseries.csv
route_decisions.csv
```

7. Improve graph algorithms:

```text
real k-shortest paths
edge utilization
path-aware link capacity
```

8. Add property/golden tests:

```text
same seed => same result
inflight never below zero
completed + failed + active_at_end == created, except explicitly dropped requests
generated graph shape invariants
score explainability invariants
```

9. Add Docker/K8s/Java stub driver later, keeping the in-memory engine as the fast research baseline.

## Research guardrails

Do not drift into packet-level networking in the MVP. The first version is an abstract discrete-event simulator where latency/error/pressure/capacity are modeled by parameters and synthetic scenarios.

The priority is:

```text
correct boundaries
reproducibility
traceability
pluggability
explainable score decisions
comparable experiment outputs
```

not production-grade networking fidelity.
