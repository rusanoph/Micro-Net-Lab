# Command Cheat Sheet

Run commands from the repository root.

## Build

```bash
cargo build --workspace
cargo test --workspace
```

Release build for CPU-heavy benchmarks:

```bash
export RUSTFLAGS="-C target-cpu=native"
cargo build --release -p micro-net-cli
```

## Lightweight Sanity Check

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/sanity-001.toml \
  --parallel 4 \
  --out ./bench-results/sanity-001
```

## Full Publication Run

```bash
PARALLEL="$(nproc)"

./target/release/micro-net bench \
  --config ./configs/paper-001.toml \
  --parallel "$PARALLEL" \
  --provider selectel \
  --vm-type standard \
  --out ./bench-results/vm/micro-net-paper-001
```

## Scaling Run

```bash
PARALLEL="$(nproc)"

./target/release/micro-net bench \
  --config ./configs/scaling-001.toml \
  --parallel "$PARALLEL" \
  --provider selectel \
  --vm-type standard \
  --out ./bench-results/vm/scaling-001
```

## Sharded Run

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

Print commands for all shards:

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/paper-001.toml \
  --shard-count 8 \
  --print-shard-commands \
  --out-base ./bench-results/micro-net-paper-001
```

Merge shards:

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

```bash
python3 scripts/micro_net_analysis_publication.py \
  --input ./bench-results/vm/micro-net-paper-001/aggregate.csv \
  --input ./bench-results/vm/scaling-001/aggregate.csv \
  --out-dir ./out/publication
```

## Notes

- `trace = "none"` and `artifacts = "aggregate"` are recommended for publication-scale runs.
- If `git` is unavailable, `run_metadata.json` stores `git_commit = null`; the simulation still works, but reproducibility metadata is weaker.
- `BENCH_VM_TYPE` / `--vm-type` is an experiment-tracking label, not necessarily a cloud API machine type.
