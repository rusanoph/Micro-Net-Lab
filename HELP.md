# Commands

## Sanity Check (Lightweight)

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/sanity-001.toml \
  --parallel 4 \
  --out ./bench-results/sanity-001
```

## Publication Run (TOML Config + Sharding Example)

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/paper-001.toml \
  --shard-index 0 --shard-count 4 \
  --provider ycloud --vm-type standard-v3-32-64 \
  --parallel 12 \
  --out ./bench-results/micro-net-paper-001/shard-0
```

## Scaling Experiment (Medium Topology)

This is a reduced cross-product intended to test whether the main effects persist as the graph grows.

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/scaling-001.toml \
  --parallel 12 \
  --out ./bench-results/scaling-001-svc10
```

## Scaling Experiment (Final, One-Shot)

Runs multiple topology sizes and replica counts in one `bench` invocation.

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/scaling-final-001.toml \
  --parallel 12 \
  --out ./bench-results/scaling-final-001
```

## Publication Run (No Config, Single Directory)

```bash
cargo run -p micro-net-cli -- bench \
  --topologies star,ring,full-mesh,random-sparse \
  --policies random,round-robin,least-inflight,score,score-local-only,score-local+network,score-local+downstream,score-no-downstream,score-no-host-pressure \
  --scenarios healthy,db-overloaded,cache-degraded,broker-lag,zone-burst,partial-failure \
  --load-levels 1,5,10,25,50,100 \
  --seeds 50 \
  --duration-ticks 12000 --warmup-ticks 2000 --drain-ticks 1000 \
  --observability-lag-ticks 10 --observability-noise 0.03 \
  --parallel 12 --trace none --artifacts aggregate --progress-ms 1000 \
  --out ./bench-results/micro-net-paper-001
```

## Sharding Helper (Print Commands For All Shards)

```bash
cargo run -p micro-net-cli -- bench \
  --config ./configs/paper-001.toml \
  --shard-count 8 --print-shard-commands \
  --out-base ./bench-results/micro-net-paper-001
```

## Merge Shards

```bash
cargo run -p micro-net-cli -- merge \
  --inputs \
    ./bench-results/micro-net-paper-001/shard-0 \
    ./bench-results/micro-net-paper-001/shard-1 \
    ./bench-results/micro-net-paper-001/shard-2 \
    ./bench-results/micro-net-paper-001/shard-3 \
    ./bench-results/micro-net-paper-001/shard-4 \
    ./bench-results/micro-net-paper-001/shard-5 \
    ./bench-results/micro-net-paper-001/shard-6 \
    ./bench-results/micro-net-paper-001/shard-7 \
  --out ./bench-results/micro-net-paper-001-merged
```

## Release Build

```bash
export RUSTFLAGS="-C target-cpu=native"
cargo build --release -p micro-net-cli
```

```bash
./target/release/micro-net bench --config ./configs/paper-001.toml --parallel 12 --out ./bench-results/micro-net-paper-001
```

## Notes

- If `git` is not available on the system, `run_metadata.json` will have `git_commit = null`. This does not affect simulation correctness; it only reduces reproducibility metadata (and makes merge validation less strict).
