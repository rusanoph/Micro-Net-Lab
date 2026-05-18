//! JSON and CSV report writers.

use micro_net_core::SimulationSummary;
use serde::Serialize;
use std::fs::File;
use std::path::Path;
use std::collections::BTreeMap;

/// Writes a pretty JSON artifact.
pub fn write_json_pretty<T: Serialize>(path: impl AsRef<Path>, value: &T) -> anyhow::Result<()> {
    let file = File::create(path)?;
    serde_json::to_writer_pretty(file, value)?;
    Ok(())
}

/// Writes a single-run summary CSV with key scalar metrics.
pub fn write_summary_csv(
    path: impl AsRef<Path>,
    summary: &SimulationSummary,
) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record(["metric", "value"])?;
    let rows = [
        ("created", summary.created.to_string()),
        ("completed", summary.completed.to_string()),
        ("failed", summary.failed.to_string()),
        ("active_at_end", summary.active_at_end.to_string()),
        ("topology", summary.topology.clone()),
        ("scenario", summary.scenario.clone()),
        ("warmup_ticks", summary.warmup_ticks.to_string()),
        ("drain_ticks", summary.drain_ticks.to_string()),
        ("requests_per_tick", summary.requests_per_tick.to_string()),
        ("success_rate", summary.success_rate.to_string()),
        ("error_rate", summary.error_rate.to_string()),
        ("avg_latency_ms", summary.avg_latency_ms.to_string()),
        ("p95_latency_ms", summary.p95_latency_ms.to_string()),
        ("p99_latency_ms", summary.p99_latency_ms.to_string()),
        (
            "throughput_per_tick",
            summary.throughput_per_tick.to_string(),
        ),
    ];
    for (metric, value) in rows {
        writer.write_record([metric.to_string(), value])?;
    }
    // Append `extra.*` fields for run-level diagnostic metrics.
    for (k, v) in &summary.extra {
        writer.write_record([format!("extra.{k}"), v.to_string()])?;
    }
    writer.flush()?;
    Ok(())
}

/// Writes aggregate summaries to CSV.
pub fn write_aggregate_csv(
    path: impl AsRef<Path>,
    summaries: &[SimulationSummary],
) -> anyhow::Result<()> {
    let mut extra_keys: Vec<String> = summaries
        .iter()
        .flat_map(|s| s.extra.keys().cloned())
        .collect();
    extra_keys.sort();
    extra_keys.dedup();

    let mut writer = csv::Writer::from_path(path)?;
    let mut header: Vec<String> = vec![
        "experiment_id",
        "policy",
        "seed",
        "created",
        "completed",
        "failed",
        "topology",
        "scenario",
        "warmup_ticks",
        "drain_ticks",
        "requests_per_tick",
        "success_rate",
        "error_rate",
        "avg_latency_ms",
        "p95_latency_ms",
        "p99_latency_ms",
        "throughput_per_tick",
    ]
    .into_iter()
    .map(|s| s.to_string())
    .collect();
    for k in &extra_keys {
        header.push(format!("extra.{k}"));
    }
    writer.write_record(header)?;
    for s in summaries {
        let mut row: Vec<String> = vec![
            s.experiment_id.clone(),
            s.policy.clone(),
            s.seed.to_string(),
            s.created.to_string(),
            s.completed.to_string(),
            s.failed.to_string(),
            s.topology.clone(),
            s.scenario.clone(),
            s.warmup_ticks.to_string(),
            s.drain_ticks.to_string(),
            s.requests_per_tick.to_string(),
            s.success_rate.to_string(),
            s.error_rate.to_string(),
            s.avg_latency_ms.to_string(),
            s.p95_latency_ms.to_string(),
            s.p99_latency_ms.to_string(),
            s.throughput_per_tick.to_string(),
        ];
        for k in &extra_keys {
            let v = s.extra.get(k).copied().unwrap_or(0.0);
            row.push(v.to_string());
        }
        writer.write_record(row)?;
    }
    writer.flush()?;
    Ok(())
}

/// Writes grouped statistics for publication-style analysis.
pub fn write_grouped_stats_csv(
    path: impl AsRef<Path>,
    summaries: &[SimulationSummary],
) -> anyhow::Result<()> {
    let mut groups: BTreeMap<(String, String, String, u64), Vec<&SimulationSummary>> =
        BTreeMap::new();
    for summary in summaries {
        groups
            .entry((
                summary.topology.clone(),
                summary.scenario.clone(),
                summary.policy.clone(),
                summary.requests_per_tick,
            ))
            .or_default()
            .push(summary);
    }

    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "topology",
        "scenario",
        "policy",
        "requests_per_tick",
        "n",
        "avg_latency_mean",
        "avg_latency_stddev",
        "avg_latency_ci95",
        "p95_latency_mean",
        "p95_latency_stddev",
        "p95_latency_ci95",
        "p99_latency_mean",
        "p99_latency_stddev",
        "p99_latency_ci95",
        "throughput_mean",
        "throughput_stddev",
        "throughput_ci95",
        "error_rate_mean",
        "error_rate_stddev",
        "error_rate_ci95",
    ])?;

    for ((topology, scenario, policy, load), rows) in groups {
        writer.write_record([
            topology,
            scenario,
            policy,
            load.to_string(),
            rows.len().to_string(),
            stat_mean(rows.iter().map(|s| s.avg_latency_ms)),
            stat_stddev(rows.iter().map(|s| s.avg_latency_ms)),
            stat_ci95(rows.iter().map(|s| s.avg_latency_ms)),
            stat_mean(rows.iter().map(|s| s.p95_latency_ms)),
            stat_stddev(rows.iter().map(|s| s.p95_latency_ms)),
            stat_ci95(rows.iter().map(|s| s.p95_latency_ms)),
            stat_mean(rows.iter().map(|s| s.p99_latency_ms)),
            stat_stddev(rows.iter().map(|s| s.p99_latency_ms)),
            stat_ci95(rows.iter().map(|s| s.p99_latency_ms)),
            stat_mean(rows.iter().map(|s| s.throughput_per_tick)),
            stat_stddev(rows.iter().map(|s| s.throughput_per_tick)),
            stat_ci95(rows.iter().map(|s| s.throughput_per_tick)),
            stat_mean(rows.iter().map(|s| s.error_rate)),
            stat_stddev(rows.iter().map(|s| s.error_rate)),
            stat_ci95(rows.iter().map(|s| s.error_rate)),
        ])?;
    }

    writer.flush()?;
    Ok(())
}

/// Writes policy-vs-baseline effect sizes for each topology/scenario/load group.
pub fn write_effect_sizes_csv(
    path: impl AsRef<Path>,
    summaries: &[SimulationSummary],
    baseline_policy: &str,
) -> anyhow::Result<()> {
    let mut groups: BTreeMap<(String, String, u64), Vec<&SimulationSummary>> = BTreeMap::new();
    for summary in summaries {
        groups
            .entry((
                summary.topology.clone(),
                summary.scenario.clone(),
                summary.requests_per_tick,
            ))
            .or_default()
            .push(summary);
    }

    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "topology",
        "scenario",
        "requests_per_tick",
        "policy",
        "baseline_policy",
        "n_policy",
        "n_baseline",
        "avg_latency_delta_pct",
        "avg_latency_cohens_d",
        "p95_latency_delta_pct",
        "p95_latency_cohens_d",
    ])?;

    for ((topology, scenario, load), rows) in groups {
        let mut by_policy: BTreeMap<String, Vec<&SimulationSummary>> = BTreeMap::new();
        for summary in rows {
            by_policy.entry(summary.policy.clone()).or_default().push(summary);
        }

        let Some(baseline_ref) = by_policy.get(baseline_policy) else {
            continue;
        };
        let baseline = baseline_ref.clone();
        let baseline_avg: Vec<f64> = baseline.iter().map(|s| s.avg_latency_ms).collect();
        let baseline_p95: Vec<f64> = baseline.iter().map(|s| s.p95_latency_ms).collect();
        let baseline_avg_mean = baseline_avg.iter().sum::<f64>() / baseline_avg.len() as f64;
        let baseline_p95_mean = baseline_p95.iter().sum::<f64>() / baseline_p95.len() as f64;

        for (policy, samples) in by_policy {
            if policy == baseline_policy {
                continue;
            }
            let policy_avg: Vec<f64> = samples.iter().map(|s| s.avg_latency_ms).collect();
            let policy_p95: Vec<f64> = samples.iter().map(|s| s.p95_latency_ms).collect();
            writer.write_record([
                topology.clone(),
                scenario.clone(),
                load.to_string(),
                policy,
                baseline_policy.to_string(),
                policy_avg.len().to_string(),
                baseline.len().to_string(),
                relative_delta_pct(
                    policy_avg.iter().sum::<f64>() / policy_avg.len() as f64,
                    baseline_avg_mean,
                ),
                cohens_d(&policy_avg, &baseline_avg),
                relative_delta_pct(
                    policy_p95.iter().sum::<f64>() / policy_p95.len() as f64,
                    baseline_p95_mean,
                ),
                cohens_d(&policy_p95, &baseline_p95),
            ])?;
        }
    }

    writer.flush()?;
    Ok(())
}

fn stat_mean<I>(values: I) -> String
where
    I: IntoIterator<Item = f64>,
{
    let xs: Vec<f64> = values.into_iter().collect();
    if xs.is_empty() {
        return "0".to_string();
    }
    (xs.iter().sum::<f64>() / xs.len() as f64).to_string()
}

fn stat_stddev<I>(values: I) -> String
where
    I: IntoIterator<Item = f64>,
{
    let xs: Vec<f64> = values.into_iter().collect();
    if xs.len() < 2 {
        return "0".to_string();
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0);
    var.sqrt().to_string()
}

fn stat_ci95<I>(values: I) -> String
where
    I: IntoIterator<Item = f64>,
{
    let xs: Vec<f64> = values.into_iter().collect();
    if xs.len() < 2 {
        return "0".to_string();
    }
    let stddev = {
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        let var = xs.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (xs.len() as f64 - 1.0);
        var.sqrt()
    };
    (1.96 * stddev / (xs.len() as f64).sqrt()).to_string()
}

fn relative_delta_pct(value: f64, baseline_mean: f64) -> String {
    if baseline_mean == 0.0 {
        return "0".to_string();
    }
    (((value - baseline_mean) / baseline_mean) * 100.0).to_string()
}

fn cohens_d(sample_a: &[f64], sample_b: &[f64]) -> String {
    if sample_a.is_empty() || sample_b.is_empty() {
        return "0".to_string();
    }
    let mean_a = sample_a.iter().sum::<f64>() / sample_a.len() as f64;
    let mean_b = sample_b.iter().sum::<f64>() / sample_b.len() as f64;
    let var_a = if sample_a.len() > 1 {
        sample_a.iter().map(|v| (v - mean_a).powi(2)).sum::<f64>() / (sample_a.len() as f64 - 1.0)
    } else {
        0.0
    };
    let var_b = if sample_b.len() > 1 {
        sample_b.iter().map(|v| (v - mean_b).powi(2)).sum::<f64>() / (sample_b.len() as f64 - 1.0)
    } else {
        0.0
    };
    let denom = (((sample_a.len() - 1) as f64 * var_a + ((sample_b.len() - 1) as f64 * var_b))
        / ((sample_a.len() + sample_b.len()).saturating_sub(2) as f64))
        .sqrt();
    if denom == 0.0 {
        "0".to_string()
    } else {
        ((mean_a - mean_b) / denom).to_string()
    }
}
