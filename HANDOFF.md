# HANDOFF: Micro-Net-Lab

This file summarizes the current project state for future development sessions.

## Current Goal

Micro-Net-Lab is now a publication-oriented Rust simulation framework for evaluating routing algorithms in microservice topologies with explicit dependency bindings and controlled degradation scenarios.

The current paper claim is intentionally narrow:

```text
Dependency-aware score routing improves the multi-objective trade-off
under localized degradation, but it is not universally best on every metric.
```

## Important Documents

```text
EXPERIMENT_MODEL_RU_publication_draft.md
  Main Russian publication draft with final tables and interpretation.

EXPERIMENT_MODEL.md
  English model/results summary mirroring the Russian draft.

README.md
  Repository overview and publication workflow.

HELP.md
  Command cheat sheet.

out/paper/EXPERIMENT_MODEL_RU_review.md
  Review notes that guided the final paper edits.
```

## Key Experiment Configs

```text
configs/sanity-001.toml
  Lightweight end-to-end check.

configs/paper-001.toml
  Full small-topology experiment:
  4 topologies x 9 algorithms x 6 scenarios x 6 loads x 50 seeds.

configs/scaling-001.toml
  One-shot scaling experiment:
  topologies x services=[3,10,20] x replicas=[3,5]
  x selected algorithms x focus scenarios x loads x 50 seeds.
```

## Final Analysis Script

```text
scripts/micro_net_analysis_publication.py
```

Expected invocation:

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

## Current Final Results

Small-topology macro summary:

```text
score-v1 family: error=0.02699, p95=44.92 ms, throughput_ratio=0.9730
round-robin:      error=0.08314, p95=46.18 ms, throughput_ratio=0.9169
random:           error=0.08832, p95=46.15 ms, throughput_ratio=0.9117
least-inflight:   error=0.10810, p95=44.35 ms, throughput_ratio=0.8919
```

Scaling macro summary:

```text
score-local+network:    error=0.02205, p95=50.20 ms, throughput_ratio=0.9779
score-no-host-pressure: error=0.02387, p95=49.37 ms, throughput_ratio=0.9761
round-robin:            error=0.13650, p95=51.59 ms, throughput_ratio=0.8635
random:                 error=0.15460, p95=51.52 ms, throughput_ratio=0.8454
least-inflight:         error=0.19260, p95=50.31 ms, throughput_ratio=0.8074
```

Localized degradation, best score variant vs `random`:

```text
small partial-failure: score-v1 family, error delta=-85.33%, p95 delta=-2.908%
small zone-burst:      score-v1 family, error delta=-81.73%, p95 delta=-1.937%
scaling partial-failure: score-local+network, error delta=-87.84%, p95 delta=-3.227%
scaling zone-burst:      score-local+network, error delta=-87.13%, p95 delta=-1.733%
```

Pareto counts:

```text
small:   score-v1 family on Pareto front in 144/144 contexts
scaling: score-no-host-pressure in 213/216, score-local+network in 180/216
```

## Interpretation Rules

- Do not claim that score routing is universally best.
- Treat `paper-001` score rows as score-family results because old artifacts collapsed score variants into `score-v1`.
- Use `scaling-001` for claims about concrete score variants.
- Do not interpret p95 without error rate and throughput ratio.
- `least-inflight` is a useful negative result: low p95 can coincide with worse reliability and throughput.

## Reproducibility

The CLI writes run metadata with:

```text
git commit
command line
rustc -Vv
uname
lscpu
provider/vm type
parallelism
shard index/count
config fingerprint
```

If `git` is missing, `git_commit` is null and simulation correctness is unaffected, but reproducibility metadata is weaker.

## Commit / Repository Link

Repository: <https://github.com/rusanoph/Micro-Net-Lab>

Publication revision with the final analysis script and documents:
<https://github.com/rusanoph/Micro-Net-Lab/commit/ea288fd>

A commit cannot contain a reliable link to its own hash because the hash depends on file contents. For publication, cite either:

```text
the exact commit printed by git after the final commit
```

or use the run metadata `git_commit` captured by the CLI for the dataset-producing run.

## Next Work

- Add production-grade baselines: power-of-two choices, EWMA latency, locality-aware routing, outlier detection.
- Add sensitivity analysis for pressure coefficient, wave period, observability lag, and noise.
- Add bursty/diurnal workload profiles.
- Optionally validate a small scenario against containerized stub services.
