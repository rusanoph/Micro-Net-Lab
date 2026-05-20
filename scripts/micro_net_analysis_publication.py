#!/usr/bin/env python3
"""
Analyze micro-net-lab-rs benchmark artifacts.

Inputs supported:
  - raw aggregate.csv / aggregate.json produced by micro-net bench
  - grouped stats.csv produced by micro-net-report

Main outputs:
  - normalized_group_stats.csv
  - global_policy_summary.csv
  - scenario_policy_summary.csv
  - size_policy_summary.csv
  - pareto_frontier.csv
  - pareto_counts.csv
  - metric_winners.csv
  - comparisons_vs_baseline.csv
  - dominance_matrix.csv
  - scenario_focus_summary.csv
  - scenario_focus_best_score_vs_baseline.csv
  - global_policy_summary_with_ci.csv
  - scenario_policy_summary_with_ci.csv
  - ci95_appendix_contexts.csv
  - metric_winner_counts.csv
  - pareto_plot_points.csv
  - figures/pareto_*.png
  - publication_tables.md
  - analysis_report.md
  - warnings.txt

The script is intentionally conservative. It separates:
  1) numerical results;
  2) Pareto-optimality;
  3) practical significance thresholds;
  4) warnings about invalid grouping / collapsed policy variants.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Optional

np = None
pd = None


METRICS_RAW = {
    "avg_latency": "avg_latency_ms",
    "p95_latency": "p95_latency_ms",
    "p99_latency": "p99_latency_ms",
    "throughput": "throughput_per_tick",
    "error_rate": "error_rate",
}

METRICS_STATS = {
    "avg_latency": "avg_latency_mean",
    "p95_latency": "p95_latency_mean",
    "p99_latency": "p99_latency_mean",
    "throughput": "throughput_mean",
    "error_rate": "error_rate_mean",
}

STAT_METRICS = ["avg_latency", "p95_latency", "p99_latency", "throughput", "error_rate"]

# Scaling topology example: fullmesh-ls10-rep3-seed1
SCALING_RE = re.compile(
    r"^(?P<kind>fullmesh|full-mesh|randomsparse|random-sparse|star|ring)-ls(?P<ls>\d+)-rep(?P<rep>\d+)-seed(?P<seed>\d+)$"
)

# Paper topology example: fullmesh-services-3-replicas-3
PAPER_RE = re.compile(
    r"^(?P<kind>fullmesh|full-mesh|randomsparse|random-sparse|star|ring)-services-(?P<ls>\d+)-replicas-(?P<rep>\d+)$"
)


def norm_kind(kind: object) -> str:
    s = str(kind).strip().lower().replace("_", "-")
    if s in {"fullmesh", "full-mesh", "full mesh"}:
        return "full-mesh"
    if s in {"randomsparse", "random-sparse", "random sparse"}:
        return "random-sparse"
    return s


def parse_topology_name(value: object) -> dict:
    s = str(value)
    m = SCALING_RE.match(s)
    if m:
        return {
            "topology_key": f"{norm_kind(m.group('kind'))}-ls{m.group('ls')}-rep{m.group('rep')}",
            "topology_kind": norm_kind(m.group("kind")),
            "logical_services": int(m.group("ls")),
            "replicas_per_service": int(m.group("rep")),
            "topology_seed": int(m.group("seed")),
            "topology_seed_in_name": True,
        }
    m = PAPER_RE.match(s)
    if m:
        return {
            "topology_key": f"{norm_kind(m.group('kind'))}-ls{m.group('ls')}-rep{m.group('rep')}",
            "topology_kind": norm_kind(m.group("kind")),
            "logical_services": int(m.group("ls")),
            "replicas_per_service": int(m.group("rep")),
            "topology_seed": np.nan,
            "topology_seed_in_name": False,
        }
    return {
        "topology_key": s,
        "topology_kind": s,
        "logical_services": np.nan,
        "replicas_per_service": np.nan,
        "topology_seed": np.nan,
        "topology_seed_in_name": False,
    }


def ci95(std: float, n: int) -> float:
    if n <= 1 or pd.isna(std):
        return 0.0
    return 1.96 * std / math.sqrt(n)


def safe_pct(delta: float, base: float) -> float:
    if base == 0 or pd.isna(base):
        return np.nan
    return 100.0 * delta / base


def read_input(path: Path) -> pd.DataFrame:
    if path.suffix.lower() == ".json":
        with path.open("r", encoding="utf-8") as f:
            data = json.load(f)
        return pd.json_normalize(data)
    return pd.read_csv(path)


@dataclass
class NormalizeResult:
    group_stats: pd.DataFrame
    warnings: list[str]
    source_mode: str


def policy_col(df: pd.DataFrame) -> str:
    if "policy_variant" in df.columns and df["policy_variant"].notna().any():
        return "policy_variant"
    if "policy" in df.columns:
        return "policy"
    raise ValueError("No policy/policy_variant column found")


def add_topology_columns(df: pd.DataFrame) -> pd.DataFrame:
    if "topology" not in df.columns:
        raise ValueError("No topology column found")
    parsed = pd.DataFrame([parse_topology_name(v) for v in df["topology"]])
    return pd.concat([df.reset_index(drop=True), parsed.reset_index(drop=True)], axis=1)


def normalize_to_group_stats(df: pd.DataFrame, dataset_name: str) -> NormalizeResult:
    warnings: list[str] = []
    df = add_topology_columns(df.copy())
    pcol = policy_col(df)
    df["policy_norm"] = df[pcol].astype(str)

    is_raw = all(col in df.columns for col in METRICS_RAW.values())
    is_stats = all(col in df.columns for col in METRICS_STATS.values()) and "n" in df.columns

    context_cols = [
        "topology_key",
        "topology_kind",
        "logical_services",
        "replicas_per_service",
        "scenario",
        "requests_per_tick",
        "policy_norm",
    ]

    if is_raw:
        source_mode = "raw_aggregate"
        work = pd.DataFrame()
        for c in context_cols:
            work[c] = df[c]
        # Keep seed/topology_seed for diagnostics; grouping is by topology_key without seed.
        if "seed" in df.columns:
            work["seed"] = df["seed"]
        work["topology_seed"] = df["topology_seed"]
        for metric, raw_col in METRICS_RAW.items():
            work[metric] = pd.to_numeric(df[raw_col], errors="coerce")
        stats = aggregate_observations(work, context_cols)
    elif is_stats:
        # If stats.csv rows are actually per topology-seed (n=1 + seed embedded in topology name),
        # treat them as observations and re-aggregate over topology_key.
        n_unique = set(pd.to_numeric(df["n"], errors="coerce").dropna().astype(int).unique().tolist())
        seed_in_name_frac = float(df["topology_seed_in_name"].mean()) if len(df) else 0.0
        if seed_in_name_frac > 0.5 and n_unique == {1}:
            source_mode = "stats_seed_rows_reaggregated"
            warnings.append(
                "Detected stats.csv with topology seed embedded in topology name and n=1. "
                "Re-aggregating across topology_seed; original stats file was not seed-aggregated."
            )
            work = pd.DataFrame()
            for c in context_cols:
                work[c] = df[c]
            work["topology_seed"] = df["topology_seed"]
            for metric, stat_col in METRICS_STATS.items():
                work[metric] = pd.to_numeric(df[stat_col], errors="coerce")
            stats = aggregate_observations(work, context_cols)
        else:
            source_mode = "preaggregated_stats"
            warnings.append(
                "Input is already aggregated stats.csv. Pareto and summaries will use group means. "
                "Raw per-seed variance cannot be reconstructed from this file."
            )
            stats = pd.DataFrame()
            for c in context_cols:
                stats[c] = df[c]
            stats["n"] = pd.to_numeric(df["n"], errors="coerce").fillna(0).astype(int)
            for metric, stat_col in METRICS_STATS.items():
                stats[f"{metric}_mean"] = pd.to_numeric(df[stat_col], errors="coerce")
                std_col = stat_col.replace("_mean", "_stddev")
                ci_col = stat_col.replace("_mean", "_ci95")
                stats[f"{metric}_stddev"] = pd.to_numeric(df.get(std_col, 0.0), errors="coerce").fillna(0.0)
                stats[f"{metric}_ci95"] = pd.to_numeric(df.get(ci_col, 0.0), errors="coerce").fillna(0.0)
    else:
        raise ValueError(
            "Unsupported input format. Expected raw aggregate columns or grouped stats columns."
        )

    stats = stats.rename(columns={"policy_norm": "policy"})
    stats.insert(0, "dataset", dataset_name)
    stats["throughput_ratio_mean"] = stats["throughput_mean"] / stats["requests_per_tick"].replace(0, np.nan)

    # Diagnostics: collapsed score-family likely if only score-v1 exists but config had more variants,
    # or if score-v1 n is a large multiple of baseline n.
    if "score-v1" in set(stats["policy"]):
        base_n = stats.loc[stats["policy"].eq("random"), "n"].median()
        score_n = stats.loc[stats["policy"].eq("score-v1"), "n"].median()
        if pd.notna(base_n) and pd.notna(score_n) and base_n > 0 and score_n >= base_n * 2:
            warnings.append(
                f"Policy 'score-v1' has median n={score_n:g}, while random has median n={base_n:g}. "
                "This usually means multiple score variants were collapsed into one policy group. "
                "Use raw aggregate data or newer artifacts with policy_variant for ablation claims."
            )
    return NormalizeResult(stats, warnings, source_mode)


def aggregate_observations(work: pd.DataFrame, context_cols: list[str]) -> pd.DataFrame:
    rows = []
    for keys, g in work.groupby(context_cols, dropna=False, sort=True):
        if not isinstance(keys, tuple):
            keys = (keys,)
        row = dict(zip(context_cols, keys))
        row["n"] = len(g)
        for metric in STAT_METRICS:
            vals = pd.to_numeric(g[metric], errors="coerce").dropna()
            n = len(vals)
            row[f"{metric}_mean"] = vals.mean() if n else np.nan
            row[f"{metric}_stddev"] = vals.std(ddof=1) if n > 1 else 0.0
            row[f"{metric}_ci95"] = ci95(row[f"{metric}_stddev"], n)
        rows.append(row)
    return pd.DataFrame(rows)


def policy_summaries(stats: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    agg_cols = {
        "error_rate_mean": "mean",
        "p95_latency_mean": "mean",
        "avg_latency_mean": "mean",
        "p99_latency_mean": "mean",
        "throughput_ratio_mean": "mean",
        "throughput_mean": "mean",
        "n": "sum",
    }
    global_summary = (
        stats.groupby(["dataset", "policy"], dropna=False)
        .agg(agg_cols)
        .reset_index()
    )
    global_summary.columns = [
        "dataset",
        "policy",
        "error_rate_macro_mean",
        "p95_latency_macro_mean",
        "avg_latency_macro_mean",
        "p99_latency_macro_mean",
        "throughput_ratio_macro_mean",
        "throughput_macro_mean",
        "n_total_or_sum",
    ]
    global_summary = global_summary.sort_values(["dataset", "error_rate_macro_mean", "p95_latency_macro_mean"])

    scenario_summary = (
        stats.groupby(["dataset", "scenario", "policy"], dropna=False)
        .agg(agg_cols)
        .reset_index()
    )
    scenario_summary.columns = [
        "dataset",
        "scenario",
        "policy",
        "error_rate_macro_mean",
        "p95_latency_macro_mean",
        "avg_latency_macro_mean",
        "p99_latency_macro_mean",
        "throughput_ratio_macro_mean",
        "throughput_macro_mean",
        "n_total_or_sum",
    ]

    size_summary = (
        stats.groupby(["dataset", "logical_services", "replicas_per_service", "policy"], dropna=False)
        .agg(agg_cols)
        .reset_index()
    )
    size_summary.columns = [
        "dataset",
        "logical_services",
        "replicas_per_service",
        "policy",
        "error_rate_macro_mean",
        "p95_latency_macro_mean",
        "avg_latency_macro_mean",
        "p99_latency_macro_mean",
        "throughput_ratio_macro_mean",
        "throughput_macro_mean",
        "n_total_or_sum",
    ]
    return global_summary, scenario_summary, size_summary


def dominates(a: pd.Series, b: pd.Series, eps: float = 1e-12) -> bool:
    # Lower error and p95 are better; higher throughput ratio is better.
    no_worse = (
        a["error_rate_mean"] <= b["error_rate_mean"] + eps
        and a["p95_latency_mean"] <= b["p95_latency_mean"] + eps
        and a["throughput_ratio_mean"] >= b["throughput_ratio_mean"] - eps
    )
    strictly_better = (
        a["error_rate_mean"] < b["error_rate_mean"] - eps
        or a["p95_latency_mean"] < b["p95_latency_mean"] - eps
        or a["throughput_ratio_mean"] > b["throughput_ratio_mean"] + eps
    )
    return bool(no_worse and strictly_better)


def pareto_analysis(stats: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame, pd.DataFrame, pd.DataFrame]:
    context_cols = [
        "dataset",
        "topology_key",
        "topology_kind",
        "logical_services",
        "replicas_per_service",
        "scenario",
        "requests_per_tick",
    ]
    front_rows = []
    dominance_rows = []
    winner_rows = []
    for keys, g in stats.groupby(context_cols, dropna=False, sort=True):
        if not isinstance(keys, tuple):
            keys = (keys,)
        ctx = dict(zip(context_cols, keys))
        g = g.reset_index(drop=True)
        for i, row in g.iterrows():
            dominated_by = []
            dominates_list = []
            for j, other in g.iterrows():
                if i == j:
                    continue
                if dominates(other, row):
                    dominated_by.append(str(other["policy"]))
                if dominates(row, other):
                    dominates_list.append(str(other["policy"]))
            front_rows.append(
                {
                    **ctx,
                    "policy": row["policy"],
                    "is_pareto": len(dominated_by) == 0,
                    "dominated_by": ";".join(dominated_by),
                    "dominates": ";".join(dominates_list),
                    "error_rate_mean": row["error_rate_mean"],
                    "p95_latency_mean": row["p95_latency_mean"],
                    "throughput_ratio_mean": row["throughput_ratio_mean"],
                }
            )
            for target in dominates_list:
                dominance_rows.append({**ctx, "dominator": row["policy"], "dominated": target})

        # Metric winners per context. Ties are kept within numerical tolerance.
        for metric, direction in [
            ("error_rate_mean", "min"),
            ("p95_latency_mean", "min"),
            ("throughput_ratio_mean", "max"),
        ]:
            best = g[metric].min() if direction == "min" else g[metric].max()
            winners = g.loc[np.isclose(g[metric], best, rtol=1e-10, atol=1e-12), "policy"].astype(str).tolist()
            winner_rows.append({**ctx, "metric": metric, "direction": direction, "best_value": best, "winners": ";".join(winners)})

    frontier = pd.DataFrame(front_rows)
    pareto_counts = (
        frontier.groupby(["dataset", "policy"], dropna=False)
        .agg(contexts=("is_pareto", "size"), pareto_contexts=("is_pareto", "sum"))
        .reset_index()
    )
    pareto_counts["pareto_share"] = pareto_counts["pareto_contexts"] / pareto_counts["contexts"].replace(0, np.nan)
    pareto_counts = pareto_counts.sort_values(["dataset", "pareto_share", "pareto_contexts"], ascending=[True, False, False])

    dominance = pd.DataFrame(dominance_rows)
    if not dominance.empty:
        dominance_matrix = (
            dominance.groupby(["dataset", "dominator", "dominated"], dropna=False)
            .size()
            .reset_index(name="contexts_dominated")
            .sort_values(["dataset", "contexts_dominated"], ascending=[True, False])
        )
    else:
        dominance_matrix = pd.DataFrame(columns=["dataset", "dominator", "dominated", "contexts_dominated"])

    metric_winners = pd.DataFrame(winner_rows)
    return frontier, pareto_counts, metric_winners, dominance_matrix


def comparisons_vs_baseline(stats: pd.DataFrame, baseline: str) -> pd.DataFrame:
    context_cols = [
        "dataset",
        "topology_key",
        "topology_kind",
        "logical_services",
        "replicas_per_service",
        "scenario",
        "requests_per_tick",
    ]
    rows = []
    for keys, g in stats.groupby(context_cols, dropna=False, sort=True):
        if not isinstance(keys, tuple):
            keys = (keys,)
        ctx = dict(zip(context_cols, keys))
        base_rows = g[g["policy"].astype(str).eq(baseline)]
        if base_rows.empty:
            continue
        b = base_rows.iloc[0]
        for _, p in g.iterrows():
            if str(p["policy"]) == baseline:
                continue
            error_delta = p["error_rate_mean"] - b["error_rate_mean"]
            p95_delta = p["p95_latency_mean"] - b["p95_latency_mean"]
            thr_delta = p["throughput_ratio_mean"] - b["throughput_ratio_mean"]
            rows.append({
                **ctx,
                "policy": p["policy"],
                "baseline": baseline,
                "error_rate_policy": p["error_rate_mean"],
                "error_rate_baseline": b["error_rate_mean"],
                "error_rate_delta_abs": error_delta,
                "error_rate_delta_pct_of_baseline": safe_pct(error_delta, b["error_rate_mean"]),
                "p95_latency_policy": p["p95_latency_mean"],
                "p95_latency_baseline": b["p95_latency_mean"],
                "p95_latency_delta_abs": p95_delta,
                "p95_latency_delta_pct_of_baseline": safe_pct(p95_delta, b["p95_latency_mean"]),
                "throughput_ratio_policy": p["throughput_ratio_mean"],
                "throughput_ratio_baseline": b["throughput_ratio_mean"],
                "throughput_ratio_delta_abs": thr_delta,
                "throughput_ratio_delta_pct_of_baseline": safe_pct(thr_delta, b["throughput_ratio_mean"]),
            })
    return pd.DataFrame(rows)


def classify_practical_effects(comp: pd.DataFrame, error_abs: float, p95_pct: float, throughput_abs: float) -> pd.DataFrame:
    if comp.empty:
        return comp
    comp = comp.copy()
    comp["meaningful_error_improvement"] = comp["error_rate_delta_abs"] <= -error_abs
    comp["meaningful_p95_improvement"] = comp["p95_latency_delta_pct_of_baseline"] <= -(p95_pct * 100.0)
    comp["meaningful_throughput_improvement"] = comp["throughput_ratio_delta_abs"] >= throughput_abs
    comp["meaningful_any_improvement"] = (
        comp["meaningful_error_improvement"]
        | comp["meaningful_p95_improvement"]
        | comp["meaningful_throughput_improvement"]
    )
    # Vectorized all-3: lower error, lower p95 by threshold, higher throughput by threshold.
    comp["meaningful_all_3_objectives"] = (
        (comp["error_rate_delta_abs"] <= -error_abs)
        & (comp["p95_latency_delta_pct_of_baseline"] <= -(p95_pct * 100.0))
        & (comp["throughput_ratio_delta_abs"] >= throughput_abs)
    )
    return comp



def is_score_policy(policy: object) -> bool:
    """Return True for score-family policies and their ablation variants."""
    return str(policy).strip().lower().startswith("score")


def sanitize_filename_part(value: object) -> str:
    """Make a stable, portable filename fragment from dataset/scenario names."""
    s = str(value)
    s = re.sub(r"[\\/:\s]+", "_", s)
    s = re.sub(r"[^A-Za-z0-9_.+\-]+", "_", s)
    return s.strip("_") or "dataset"


def macro_summary_with_ci(stats: pd.DataFrame, group_cols: list[str]) -> pd.DataFrame:
    """
    Macro-average context-level means and estimate CI95 over contexts.

    This is intended for publication appendices: the primary per-seed/per-group CI95
    remains in normalized_group_stats.csv; this table summarizes uncertainty of
    macro-averaged tables without widening the main article tables.
    """
    metric_cols = {
        "error_rate": "error_rate_mean",
        "avg_latency": "avg_latency_mean",
        "p95_latency": "p95_latency_mean",
        "p99_latency": "p99_latency_mean",
        "throughput": "throughput_mean",
        "throughput_ratio": "throughput_ratio_mean",
    }
    rows: list[dict] = []
    for keys, g in stats.groupby(group_cols, dropna=False, sort=True):
        if not isinstance(keys, tuple):
            keys = (keys,)
        row = dict(zip(group_cols, keys))
        row["contexts"] = len(g)
        row["n_total_or_sum"] = int(pd.to_numeric(g.get("n", pd.Series(dtype=float)), errors="coerce").fillna(0).sum())
        for metric, col in metric_cols.items():
            vals = pd.to_numeric(g[col], errors="coerce").dropna() if col in g.columns else pd.Series(dtype=float)
            n = len(vals)
            std = vals.std(ddof=1) if n > 1 else 0.0
            row[f"{metric}_macro_mean"] = vals.mean() if n else np.nan
            row[f"{metric}_macro_stddev"] = std
            row[f"{metric}_macro_ci95"] = ci95(std, n)
        rows.append(row)
    return pd.DataFrame(rows)


def context_ci95_appendix(stats: pd.DataFrame) -> pd.DataFrame:
    """Keep only the columns useful for a CI95 appendix."""
    base_cols = [
        "dataset",
        "topology_key",
        "topology_kind",
        "logical_services",
        "replicas_per_service",
        "scenario",
        "requests_per_tick",
        "policy",
        "n",
    ]
    metric_cols: list[str] = []
    for metric in STAT_METRICS:
        metric_cols.extend([f"{metric}_mean", f"{metric}_stddev", f"{metric}_ci95"])
    keep = [c for c in base_cols + metric_cols + ["throughput_ratio_mean"] if c in stats.columns]
    return stats[keep].sort_values([c for c in base_cols if c in stats.columns])


def build_metric_winner_counts(metric_winners: pd.DataFrame) -> pd.DataFrame:
    if metric_winners.empty:
        return pd.DataFrame(columns=["dataset", "metric", "winners", "winner_contexts"])
    exploded = metric_winners.assign(winners=metric_winners["winners"].astype(str).str.split(";")).explode("winners")
    exploded = exploded[exploded["winners"].notna() & exploded["winners"].astype(str).ne("")]
    return (
        exploded.groupby(["dataset", "metric", "winners"], dropna=False)
        .size()
        .reset_index(name="winner_contexts")
        .sort_values(["dataset", "metric", "winner_contexts"], ascending=[True, True, False])
    )


def scenario_focus_summary(comp: pd.DataFrame, focus_scenarios: list[str]) -> pd.DataFrame:
    """
    Compact scenario × policy comparison against baseline.

    This is the table the article needs for zone-burst / partial-failure:
    delta error, delta p95, and delta throughput versus the chosen baseline.
    """
    if comp.empty:
        return pd.DataFrame()
    focus = comp[comp["scenario"].astype(str).isin(focus_scenarios)].copy()
    if focus.empty:
        return pd.DataFrame()
    bool_cols = [
        "meaningful_any_improvement",
        "meaningful_error_improvement",
        "meaningful_p95_improvement",
        "meaningful_throughput_improvement",
        "meaningful_all_3_objectives",
    ]
    agg: dict[str, tuple[str, str]] = {
        "contexts": ("policy", "size"),
        "error_rate_policy_mean": ("error_rate_policy", "mean"),
        "error_rate_baseline_mean": ("error_rate_baseline", "mean"),
        "error_rate_delta_abs_mean": ("error_rate_delta_abs", "mean"),
        "error_rate_delta_pct_mean": ("error_rate_delta_pct_of_baseline", "mean"),
        "p95_latency_policy_mean": ("p95_latency_policy", "mean"),
        "p95_latency_baseline_mean": ("p95_latency_baseline", "mean"),
        "p95_latency_delta_abs_mean": ("p95_latency_delta_abs", "mean"),
        "p95_latency_delta_pct_mean": ("p95_latency_delta_pct_of_baseline", "mean"),
        "throughput_ratio_policy_mean": ("throughput_ratio_policy", "mean"),
        "throughput_ratio_baseline_mean": ("throughput_ratio_baseline", "mean"),
        "throughput_ratio_delta_abs_mean": ("throughput_ratio_delta_abs", "mean"),
        "throughput_ratio_delta_pct_mean": ("throughput_ratio_delta_pct_of_baseline", "mean"),
    }
    for c in bool_cols:
        if c in focus.columns:
            agg[f"{c}_contexts"] = (c, "sum")
    result = (
        focus.groupby(["dataset", "scenario", "policy", "baseline"], dropna=False)
        .agg(**agg)
        .reset_index()
        .sort_values(
            ["dataset", "scenario", "error_rate_delta_abs_mean", "throughput_ratio_delta_abs_mean", "p95_latency_delta_abs_mean"],
            ascending=[True, True, True, False, True],
        )
    )
    return result


def best_score_by_focus_scenario(focus_summary: pd.DataFrame) -> pd.DataFrame:
    """Pick the strongest score-family variant per dataset/scenario using reliability-first ordering."""
    if focus_summary.empty:
        return pd.DataFrame()
    score_rows = focus_summary[focus_summary["policy"].map(is_score_policy)].copy()
    if score_rows.empty:
        return pd.DataFrame()
    selected = []
    for keys, g in score_rows.groupby(["dataset", "scenario"], dropna=False, sort=True):
        if not isinstance(keys, tuple):
            keys = (keys,)
        g = g.sort_values(
            ["error_rate_delta_abs_mean", "throughput_ratio_delta_abs_mean", "p95_latency_delta_abs_mean"],
            ascending=[True, False, True],
        )
        row = g.iloc[0].to_dict()
        row["selection_rule"] = "min Δerror vs baseline, then max Δthroughput, then min Δp95"
        selected.append(row)
    return pd.DataFrame(selected)


def pareto_plot_points(stats: pd.DataFrame, focus_scenarios: list[str]) -> pd.DataFrame:
    """Aggregate per-scenario points used for publication Pareto scatter plots."""
    focus = stats[stats["scenario"].astype(str).isin(focus_scenarios)].copy()
    if focus.empty:
        return pd.DataFrame()
    return macro_summary_with_ci(focus, ["dataset", "scenario", "policy"]).sort_values(
        ["dataset", "scenario", "error_rate_macro_mean", "p95_latency_macro_mean"]
    )


def plot_pareto_figures(plot_points: pd.DataFrame, out: Path, figure_format: str = "png") -> pd.DataFrame:
    """
    Create one Pareto scatter plot per dataset/scenario.

    X: error rate; Y: p95 latency; marker size: throughput ratio. Lower-left is better
    for error and p95; larger markers indicate higher normalized throughput.
    """
    if plot_points.empty:
        return pd.DataFrame(columns=["dataset", "scenario", "figure_path"])
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except Exception as exc:  # pragma: no cover - depends on optional runtime package
        return pd.DataFrame([{"dataset": "", "scenario": "", "figure_path": "", "error": f"matplotlib unavailable: {exc}"}])

    fig_dir = out / "figures"
    fig_dir.mkdir(parents=True, exist_ok=True)
    rows: list[dict] = []
    for (dataset, scenario), g in plot_points.groupby(["dataset", "scenario"], dropna=False, sort=True):
        g = g.dropna(subset=["error_rate_macro_mean", "p95_latency_macro_mean", "throughput_ratio_macro_mean"])
        if g.empty:
            continue
        fig, ax = plt.subplots(figsize=(8, 5))
        sizes = (g["throughput_ratio_macro_mean"].clip(lower=0) * 250.0).fillna(80.0)
        sizes = sizes.clip(lower=50.0)
        ax.scatter(g["error_rate_macro_mean"], g["p95_latency_macro_mean"], s=sizes)
        for _, row in g.iterrows():
            ax.annotate(
                str(row["policy"]),
                (row["error_rate_macro_mean"], row["p95_latency_macro_mean"]),
                xytext=(4, 4),
                textcoords="offset points",
                fontsize=8,
            )
        ax.set_xlabel("error_rate, меньше лучше")
        ax.set_ylabel("p95 latency, мс, меньше лучше")
        ax.set_title(f"Pareto view: {Path(str(dataset)).stem} / {scenario}")
        ax.grid(True, alpha=0.3)
        fig.tight_layout()
        filename = f"pareto_{sanitize_filename_part(Path(str(dataset)).stem)}_{sanitize_filename_part(scenario)}.{figure_format}"
        target = fig_dir / filename
        fig.savefig(target, dpi=180)
        plt.close(fig)
        rows.append({"dataset": dataset, "scenario": scenario, "figure_path": str(target)})
    return pd.DataFrame(rows)


def write_publication_tables(
    out: Path,
    global_ci: pd.DataFrame,
    scenario_focus: pd.DataFrame,
    best_score: pd.DataFrame,
    pareto_counts: pd.DataFrame,
    winner_counts: pd.DataFrame,
    figure_index: pd.DataFrame,
    focus_scenarios: list[str],
) -> None:
    """Write ready-to-copy Markdown fragments for the article and appendix."""
    lines: list[str] = []
    lines.append("# Publication tables for Micro-Net-Lab\n")
    lines.append("Этот файл генерируется автоматически и содержит компактные таблицы/ссылки на рисунки для вставки в статью.\n")

    lines.append("## Main table with macro CI95 moved to appendix\n")
    main_cols = [
        "dataset",
        "policy",
        "contexts",
        "n_total_or_sum",
        "error_rate_macro_mean",
        "p95_latency_macro_mean",
        "throughput_ratio_macro_mean",
    ]
    if not global_ci.empty:
        lines.append(markdown_table(global_ci[main_cols].sort_values(["dataset", "error_rate_macro_mean"]), 40))
    else:
        lines.append("_No global CI data._\n")

    lines.append("\n## Appendix A: CI95 for macro summaries\n")
    ci_cols = [
        "dataset",
        "policy",
        "contexts",
        "error_rate_macro_mean",
        "error_rate_macro_ci95",
        "p95_latency_macro_mean",
        "p95_latency_macro_ci95",
        "throughput_ratio_macro_mean",
        "throughput_ratio_macro_ci95",
    ]
    if not global_ci.empty:
        lines.append(markdown_table(global_ci[ci_cols].sort_values(["dataset", "error_rate_macro_mean"]), 80))
    else:
        lines.append("_No CI95 data._\n")

    lines.append(f"\n## Focus scenarios vs baseline: {', '.join(focus_scenarios)}\n")
    focus_cols = [
        "dataset",
        "scenario",
        "policy",
        "baseline",
        "contexts",
        "error_rate_delta_abs_mean",
        "error_rate_delta_pct_mean",
        "p95_latency_delta_pct_mean",
        "throughput_ratio_delta_abs_mean",
        "meaningful_any_improvement_contexts",
    ]
    if not scenario_focus.empty:
        existing = [c for c in focus_cols if c in scenario_focus.columns]
        lines.append(markdown_table(scenario_focus[existing], 80))
    else:
        lines.append("_No focus-scenario comparisons found. Check scenario names and baseline._\n")

    lines.append("\n## Best score-family variant per focus scenario\n")
    if not best_score.empty:
        existing = [c for c in focus_cols + ["selection_rule"] if c in best_score.columns]
        lines.append(markdown_table(best_score[existing], 30))
    else:
        lines.append("_No score-family rows found in focus scenarios._\n")

    lines.append("\n## Pareto-front counts\n")
    if not pareto_counts.empty:
        lines.append(markdown_table(pareto_counts, 80))
    else:
        lines.append("_No Pareto data._\n")

    lines.append("\n## Metric winner counts\n")
    if not winner_counts.empty:
        lines.append(markdown_table(winner_counts, 100))
    else:
        lines.append("_No metric winner counts._\n")

    lines.append("\n## Generated Pareto figures\n")
    if not figure_index.empty:
        lines.append(markdown_table(figure_index, 50))
    else:
        lines.append("_No figures generated._\n")

    lines.append("\n## Suggested article wording\n")
    lines.append(
        "Доверительные интервалы для основных макроусреднённых таблиц вынесены в приложение: "
        "в основном тексте оставлены средние значения и число контекстов, чтобы не перегружать таблицы.\n"
    )
    lines.append(
        "Для сценариев локализованной деградации дополнительно приводится сравнение с baseline "
        "по изменению доли ошибок, p95-задержки и относительной пропускной способности.\n"
    )
    (out / "publication_tables.md").write_text("\n".join(lines), encoding="utf-8")


def markdown_table(df: pd.DataFrame, max_rows: int = 20, floatfmt: str = ".4g") -> str:
    if df.empty:
        return "_No rows._\n"
    show = df.head(max_rows).copy()
    for col in show.select_dtypes(include=[np.number]).columns:
        show[col] = show[col].map(lambda x: "" if pd.isna(x) else format(float(x), floatfmt))
    return show.to_markdown(index=False) + "\n"


def write_report(
    out: Path,
    warnings: list[str],
    global_summary: pd.DataFrame,
    scenario_summary: pd.DataFrame,
    pareto_counts: pd.DataFrame,
    metric_winners: pd.DataFrame,
    comp: pd.DataFrame,
    thresholds: tuple[float, float, float],
) -> None:
    error_abs, p95_pct, throughput_abs = thresholds
    lines = []
    lines.append("# Micro-Net benchmark analysis\n")
    lines.append("## Warnings / diagnostics\n")
    if warnings:
        for w in warnings:
            lines.append(f"- {w}")
    else:
        lines.append("- No warnings.")
    lines.append("")
    lines.append("## Global policy summary (macro-average over contexts)\n")
    cols = [
        "dataset", "policy", "error_rate_macro_mean", "p95_latency_macro_mean",
        "throughput_ratio_macro_mean", "n_total_or_sum",
    ]
    lines.append(markdown_table(global_summary[cols].sort_values(["dataset", "error_rate_macro_mean"]), 30))

    lines.append("\n## Pareto counts\n")
    lines.append(
        "Pareto objective vector: minimize `error_rate_mean`, minimize `p95_latency_mean`, "
        "maximize `throughput_ratio_mean`. Counts are per context.\n"
    )
    lines.append(markdown_table(pareto_counts, 50))

    lines.append("\n## Metric winner frequencies\n")
    if not metric_winners.empty:
        exploded = metric_winners.assign(winners=metric_winners["winners"].str.split(";")).explode("winners")
        winner_counts = (
            exploded.groupby(["dataset", "metric", "winners"], dropna=False)
            .size().reset_index(name="winner_contexts")
            .sort_values(["dataset", "metric", "winner_contexts"], ascending=[True, True, False])
        )
        lines.append(markdown_table(winner_counts, 80))
    else:
        lines.append("_No winner data._\n")

    lines.append("\n## Comparisons vs baseline: practical-significance thresholds\n")
    lines.append(
        f"Thresholds: error absolute reduction ≥ {error_abs:g}; "
        f"p95 relative reduction ≥ {p95_pct*100:g}%; "
        f"throughput-ratio absolute increase ≥ {throughput_abs:g}.\n"
    )
    if not comp.empty:
        practical_counts = (
            comp.groupby(["dataset", "policy"], dropna=False)
            .agg(
                contexts=("policy", "size"),
                meaningful_any=("meaningful_any_improvement", "sum"),
                meaningful_error=("meaningful_error_improvement", "sum"),
                meaningful_p95=("meaningful_p95_improvement", "sum"),
                meaningful_throughput=("meaningful_throughput_improvement", "sum"),
                meaningful_all3=("meaningful_all_3_objectives", "sum"),
                error_delta_abs_mean=("error_rate_delta_abs", "mean"),
                p95_delta_pct_mean=("p95_latency_delta_pct_of_baseline", "mean"),
                throughput_ratio_delta_mean=("throughput_ratio_delta_abs", "mean"),
            )
            .reset_index()
            .sort_values(["dataset", "meaningful_all3", "meaningful_any"], ascending=[True, False, False])
        )
        lines.append(markdown_table(practical_counts, 80))
    else:
        lines.append("_No baseline comparisons available._\n")

    lines.append("\n## How to read this report\n")
    lines.append(
        "- `global_policy_summary` is a macro-average over contexts; it is not proof that a policy wins in every topology/scenario/load.\n"
        "- `metric_winners.csv` answers 'who is the extremum in each context'. Use it to verify claims like 'policy X is best by error rate'.\n"
        "- `pareto_frontier.csv` answers multi-objective optimality. A policy can be Pareto-optimal even if it is not the best by any single metric.\n"
        "- For scientific claims, prefer context-specific comparisons and practical-effect thresholds, not only raw mean differences.\n"
    )
    (out / "analysis_report.md").write_text("\n".join(lines), encoding="utf-8")


def main(argv: Optional[list[str]] = None) -> int:
    ap = argparse.ArgumentParser(description="Analyze micro-net-lab-rs benchmark results")
    ap.add_argument("--input", "-i", action="append", required=True, help="Input CSV/JSON. Repeatable.")
    ap.add_argument("--out-dir", "-o", required=True, help="Output directory for analysis CSVs/report")
    ap.add_argument("--baseline", default="random", help="Baseline policy for comparisons")
    ap.add_argument("--error-abs-threshold", type=float, default=0.01, help="Practical absolute error-rate threshold, e.g. 0.01 = 1 percentage point")
    ap.add_argument("--p95-rel-threshold", type=float, default=0.02, help="Practical relative p95 latency threshold, e.g. 0.02 = 2%%")
    ap.add_argument("--throughput-ratio-threshold", type=float, default=0.02, help="Practical absolute throughput-ratio threshold")
    ap.add_argument(
        "--focus-scenario",
        action="append",
        dest="focus_scenarios",
        default=None,
        help="Scenario to highlight in publication outputs. Repeatable. Defaults: zone-burst, partial-failure.",
    )
    ap.add_argument("--no-figures", action="store_true", help="Do not generate Pareto PNG figures")
    ap.add_argument("--figure-format", default="png", choices=["png", "pdf", "svg"], help="Figure file format")
    args = ap.parse_args(argv)

    global np, pd
    try:
        import numpy as _np
        import pandas as _pd
    except ModuleNotFoundError as exc:
        print(
            f"Missing Python dependency: {exc.name}. "
            "Install analysis dependencies with: python3 -m pip install numpy pandas matplotlib",
            file=sys.stderr,
        )
        return 2
    np = _np
    pd = _pd

    out = Path(args.out_dir)
    out.mkdir(parents=True, exist_ok=True)

    all_stats = []
    all_warnings: list[str] = []
    for inp in args.input:
        path = Path(inp)
        if not path.exists():
            raise FileNotFoundError(path)
        dataset_name = path
        df = read_input(path)
        norm = normalize_to_group_stats(df, dataset_name)
        all_stats.append(norm.group_stats)
        all_warnings.append(f"[{dataset_name}] source_mode={norm.source_mode}")
        for w in norm.warnings:
            all_warnings.append(f"[{dataset_name}] {w}")

    stats = pd.concat(all_stats, ignore_index=True)
    stats.to_csv(out / "normalized_group_stats.csv", index=False)

    global_summary, scenario_summary, size_summary = policy_summaries(stats)
    global_summary.to_csv(out / "global_policy_summary.csv", index=False)
    scenario_summary.to_csv(out / "scenario_policy_summary.csv", index=False)
    size_summary.to_csv(out / "size_policy_summary.csv", index=False)

    frontier, pareto_counts, metric_winners, dominance_matrix = pareto_analysis(stats)
    frontier.to_csv(out / "pareto_frontier.csv", index=False)
    pareto_counts.to_csv(out / "pareto_counts.csv", index=False)
    metric_winners.to_csv(out / "metric_winners.csv", index=False)
    dominance_matrix.to_csv(out / "dominance_matrix.csv", index=False)

    comp = comparisons_vs_baseline(stats, args.baseline)
    comp = classify_practical_effects(
        comp,
        error_abs=args.error_abs_threshold,
        p95_pct=args.p95_rel_threshold,
        throughput_abs=args.throughput_ratio_threshold,
    )
    comp.to_csv(out / "comparisons_vs_baseline.csv", index=False)

    focus_scenarios = args.focus_scenarios or ["zone-burst", "partial-failure"]

    global_ci = macro_summary_with_ci(stats, ["dataset", "policy"])
    scenario_ci = macro_summary_with_ci(stats, ["dataset", "scenario", "policy"])
    size_ci = macro_summary_with_ci(stats, ["dataset", "logical_services", "replicas_per_service", "policy"])
    ci_contexts = context_ci95_appendix(stats)
    winner_counts = build_metric_winner_counts(metric_winners)
    focus_summary = scenario_focus_summary(comp, focus_scenarios)
    best_score = best_score_by_focus_scenario(focus_summary)
    plot_points = pareto_plot_points(stats, focus_scenarios)

    global_ci.to_csv(out / "global_policy_summary_with_ci.csv", index=False)
    scenario_ci.to_csv(out / "scenario_policy_summary_with_ci.csv", index=False)
    size_ci.to_csv(out / "size_policy_summary_with_ci.csv", index=False)
    ci_contexts.to_csv(out / "ci95_appendix_contexts.csv", index=False)
    winner_counts.to_csv(out / "metric_winner_counts.csv", index=False)
    focus_summary.to_csv(out / "scenario_focus_summary.csv", index=False)
    best_score.to_csv(out / "scenario_focus_best_score_vs_baseline.csv", index=False)
    plot_points.to_csv(out / "pareto_plot_points.csv", index=False)

    if args.no_figures:
        figure_index = pd.DataFrame(columns=["dataset", "scenario", "figure_path"])
    else:
        figure_index = plot_pareto_figures(plot_points, out, args.figure_format)
    figure_index.to_csv(out / "pareto_figure_index.csv", index=False)

    write_publication_tables(
        out=out,
        global_ci=global_ci,
        scenario_focus=focus_summary,
        best_score=best_score,
        pareto_counts=pareto_counts,
        winner_counts=winner_counts,
        figure_index=figure_index,
        focus_scenarios=focus_scenarios,
    )

    (out / "warnings.txt").write_text("\n".join(all_warnings) + "\n", encoding="utf-8")
    write_report(
        out,
        all_warnings,
        global_summary,
        scenario_summary,
        pareto_counts,
        metric_winners,
        comp,
        (args.error_abs_threshold, args.p95_rel_threshold, args.throughput_ratio_threshold),
    )
    print(f"Wrote analysis to {out}")
    print(f"Open: {out / 'analysis_report.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
