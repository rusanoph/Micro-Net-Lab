# micro-net-lab-rs

`micro-net-lab-rs` is a Rust research framework for deterministic, tick-based experiments with routing and load-balancing policies in typed microservice topologies.

The first implementation is an **in-memory discrete-event simulator**. It is intentionally not a production service mesh, not a packet-level emulator and not a Docker/Kubernetes orchestrator. The architecture is prepared for those integrations through driver/adapter traits.

## Why this project exists

The core research direction is:

```text
Dependency-aware score-based routing for typed microservice topologies.
```

Traditional baselines usually route by local backend state:

```text
random
round-robin
least-inflight
latency / EWMA latency
```

The proposed research direction is to compare those baselines with a policy that also accounts for candidate downstream state:

```text
database pressure
cache miss risk
broker lag
network path cost
host/zone pressure
retry amplification risk
```

A key modeling decision is that all replicas of a logical service share the same logical dependency profile, but different concrete service instances can have different runtime placement, links, hosts, zones and concrete dependency bindings.

Example:

```text
LogicalService payments:
  depends on service-cache
  depends on primary-db
  publishes to events-broker

ServiceInstance payments-1:
  zone-a, host-a, cache-a, path to db-main = low latency

ServiceInstance payments-2:
  zone-b, host-b, cache-b, path to db-main = medium latency

ServiceInstance payments-3:
  zone-c, shared host, cache-c, path to db-main = high latency
```

The routing decision is still:

```text
target LogicalService = payments
candidate ServiceInstances = payments-1, payments-2, payments-3
```

But a dependency-aware score may prefer a candidate whose local metrics are slightly worse if its concrete downstream path/resources are healthier.

## Workspace layout

```text
micro-net-lab-rs/
  Cargo.toml
  README.md
  HANDOFF.md
  crates/
    micro-net-core/        # pure domain model + traits
    micro-net-petgraph/    # PetgraphBackend + topology generators
    micro-net-algorithms/  # random / round-robin / least-inflight / score-v1
    micro-net-drivers/     # in-memory tick simulation engine
    micro-net-executor/    # sequential / rayon batch helpers
    micro-net-report/      # JSON/CSV report writers
    micro-net-cli/         # CLI binary: micro-net
```

`micro-net-core` does not depend on Petgraph, Tokio, Docker, Kubernetes, WebSocket or any UI/output implementation. Concrete backends depend on `micro-net-core`.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

## Generate topology

```bash
cargo run -p micro-net-cli -- generate-topology \
  --kind ring \
  --logical-services 3 \
  --replicas-per-service 3 \
  --seed 42 \
  --out topology.json
```

Supported topology kinds:

```text
star
ring
full-mesh
random-sparse
```

The generated topology includes:

```text
logical services
service instances
zones
hosts
database/cache/broker nodes
explicit dependency bindings
edges with latency/capacity/error/cost
```

## Run one experiment

```bash
cargo run -p micro-net-cli -- run \
  --topology topology.json \
  --policy score \
  --seed 42 \
  --duration-ticks 100 \
  --requests-per-tick 5 \
  --out runs/score-001
```

Supported policies:

```text
random
round-robin
least-inflight
score
```

Output artifacts:

```text
runs/score-001/
  experiment.json
  topology.json
  trace.jsonl
  summary.json
  metrics.csv
```

## Run a small benchmark

```bash
cargo run -p micro-net-cli -- bench \
  --topologies star,ring,full-mesh,random-sparse \
  --policies random,round-robin,least-inflight,score \
  --seeds 3 \
  --parallel 4 \
  --duration-ticks 100 \
  --requests-per-tick 5 \
  --out bench-results
```

This runs:

```text
topologies × policies × seeds
```

and writes:

```text
bench-results/
  aggregate.json
  aggregate.csv
  experiments/
    ...
```

## Important concepts

### LogicalService

Abstract service identity, for example:

```text
payments
orders
analytics
```

It owns the logical dependency profile.

### ServiceInstance

Concrete replica selected by a routing policy:

```text
payments-1
payments-2
payments-3
```

Instances share logical dependencies but may have different zones, hosts, resource pressure and network paths.

### Network topology graph

Models concrete connectivity:

```text
node -> node edges
latency
capacity
error_rate
cost
```

### Service dependency profile

Models what a service logically touches:

```text
cache
database
broker
other services
external APIs
```

### DependencyBinding

Maps a concrete service instance and logical dependency to concrete target nodes:

```text
payments-1 + payments:cache -> cache-a
payments-2 + payments:cache -> cache-b
payments-3 + payments:cache -> cache-c
payments-* + payments:db    -> db-main
```

## Rustdoc

Key public types and traits have Rustdoc comments:

```text
GraphBackend
TopologySpec
ServiceIndex
LogicalServiceSpec
ServiceInstanceSpec
RoutingPolicy
Feature
RoutingContext
TraceEvent
EventSink
MetricCollector
InMemorySimulationEngine
PetgraphBackend
```

Generate docs with:

```bash
cargo doc --workspace --no-deps --open
```

## Current limitations

This is the first scaffold, not a final simulator.

Known limitations:

```text
EWMA is planned for the next iteration.
YAML config is planned for the next iteration.
Failure injectors are modeled only indirectly through synthetic runtime pressure.
k_paths currently returns only the shortest path in PetgraphBackend.
No Docker/K8s/stub driver yet.
No Web UI yet.
```

## Validation note

The project was generated in an environment where `cargo` was not installed, so the workspace could not be compiled inside that environment. The code is structured to be compilable, but the first thing to do after unpacking is:

```bash
cargo fmt
cargo test --workspace
```

and then fix any compiler-level issues reported by your local Rust toolchain.
