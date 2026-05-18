//! JSON and CSV report writers.

use micro_net_core::SimulationSummary;
use serde::Serialize;
use std::fs::File;
use std::path::Path;

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
    writer.flush()?;
    Ok(())
}

/// Writes aggregate summaries to CSV.
pub fn write_aggregate_csv(
    path: impl AsRef<Path>,
    summaries: &[SimulationSummary],
) -> anyhow::Result<()> {
    let mut writer = csv::Writer::from_path(path)?;
    writer.write_record([
        "experiment_id",
        "policy",
        "seed",
        "created",
        "completed",
        "failed",
        "success_rate",
        "error_rate",
        "avg_latency_ms",
        "p95_latency_ms",
        "p99_latency_ms",
        "throughput_per_tick",
    ])?;
    for s in summaries {
        writer.write_record([
            s.experiment_id.clone(),
            s.policy.clone(),
            s.seed.to_string(),
            s.created.to_string(),
            s.completed.to_string(),
            s.failed.to_string(),
            s.success_rate.to_string(),
            s.error_rate.to_string(),
            s.avg_latency_ms.to_string(),
            s.p95_latency_ms.to_string(),
            s.p99_latency_ms.to_string(),
            s.throughput_per_tick.to_string(),
        ])?;
    }
    writer.flush()?;
    Ok(())
}
