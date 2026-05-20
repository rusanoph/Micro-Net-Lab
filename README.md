# Micro-Net-Lab

Micro-Net-Lab is a Rust research workspace for deterministic simulation of routing and load-balancing algorithms in typed microservice topologies.

The project focuses on one research question:

```text
Can dependency-aware routing improve the error/latency/throughput trade-off
under localized degradation in microservice dependency graphs?
```

The simulator is intentionally not a production service mesh, not a packet-level network emulator, and not a Kubernetes/Docker orchestrator. It is a controlled simulation framework for reproducible comparative experiments.

## Repository

GitHub: <https://github.com/rusanoph/Micro-Net-Lab>

Large benchmark outputs are intentionally not tracked. The CLI records reproducibility metadata for every benchmark run, including the git commit, command line, Rust compiler version, machine information, provider/VM labels, sharding parameters, and config fingerprint.

## Workspace Layout

```text
crates/
  micro-net-core/        domain model and traits
  micro-net-petgraph/    graph backend and topology generators
  micro-net-algorithms/  random, round-robin, least-inflight, score variants
  micro-net-drivers/     in-memory deterministic simulation engine
  micro-net-executor/    sequential/rayon batch helpers
  micro-net-report/      JSON/CSV report writers
  micro-net-cli/         CLI binary: micro-net

configs/
  sanity-001.toml        lightweight end-to-end check
  paper-001.toml         full small-topology publication experiment
  scaling-001.toml       one-shot scaling experiment

scripts/
  micro_net_analysis_publication.py

EXPERIMENT_MODEL_RU_publication_draft.md  Russian publication draft
EXPERIMENT_MODEL.md                       English model/results summary
```

## Build

```bash
cargo build --workspace
cargo test --workspace
```

Release build:

```bash
export RUSTFLAGS="-C target-cpu=native"
cargo build --release -p micro-net-cli
```

Release binary:

```text
./target/release/micro-net
```

## Sanity Check

Use this before any long benchmark:

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/sanity-001.toml \
  --parallel 4 \
  --out ./bench-results/sanity-001
```

## Publication Benchmarks

Full small-topology experiment:

```bash
PARALLEL="$(nproc)"

./target/release/micro-net bench \
  --config ./configs/paper-001.toml \
  --parallel "$PARALLEL" \
  --provider selectel \
  --vm-type standard \
  --out ./bench-results/vm/micro-net-paper-001
```

Scaling experiment:

```bash
PARALLEL="$(nproc)"

./target/release/micro-net bench \
  --config ./configs/scaling-001.toml \
  --parallel "$PARALLEL" \
  --provider selectel \
  --vm-type standard \
  --out ./bench-results/vm/scaling-001
```

## Sharding

For large runs, split the benchmark across machines:

```bash
./target/release/micro-net bench \
  --config ./configs/paper-001.toml \
  --shard-index 0 \
  --shard-count 4 \
  --parallel "$(nproc)" \
  --provider ycloud \
  --vm-type standard-v3-32-64 \
  --out ./bench-results/micro-net-paper-001/shard-0
```

Merge shard outputs:

```bash
cargo run -p micro-net-cli -- merge \
  --inputs \
    ./bench-results/micro-net-paper-001/shard-0 \
    ./bench-results/micro-net-paper-001/shard-1 \
    ./bench-results/micro-net-paper-001/shard-2 \
    ./bench-results/micro-net-paper-001/shard-3 \
  --out ./bench-results/micro-net-paper-001-merged
```

## Publication Analysis

The final article tables are generated with:

```bash
python3 scripts/micro_net_analysis_publication.py \
  --input ./bench-results/vm/micro-net-paper-001/aggregate.csv \
  --input ./bench-results/vm/scaling-001/aggregate.csv \
  --out-dir ./out/publication
```

Important outputs:

```text
out/publication/publication_tables.md
out/publication/global_policy_summary_with_ci.csv
out/publication/scenario_focus_best_score_vs_baseline.csv
out/publication/pareto_counts.csv
out/publication/figures/
```

## Main Results

The final analysis supports the following cautious claim:

```text
Dependency-aware score routing does not dominate every metric in every regime,
but improves the Pareto trade-off under localized degradation, especially
partial-failure and zone-burst.
```

The important negative result is that `least-inflight` can look good by p95 latency while worsening error rate and throughput. Therefore routing quality must be evaluated as a multi-objective problem.
