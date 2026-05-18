use anyhow::Context;
use clap::Parser;
use plotters::prelude::*;
use plotters::series::LineSeries;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "micro-net-plot")]
#[command(about = "Plot micro-net bench stats.csv into SVG charts")]
struct Args {
    /// Input stats CSV produced by `micro-net bench` (stats.csv).
    #[arg(long)]
    stats_csv: PathBuf,
    /// Output directory for SVG charts.
    #[arg(long)]
    out: PathBuf,
    /// Comma-separated metrics to plot.
    #[arg(long, default_value = "avg_latency_mean,p95_latency_mean,p99_latency_mean,throughput_mean,error_rate_mean")]
    metrics: String,
}

#[derive(Debug, Deserialize)]
struct StatsRow {
    topology: String,
    scenario: String,
    policy: String,
    requests_per_tick: u64,
    n: u64,
    avg_latency_mean: f64,
    avg_latency_stddev: f64,
    avg_latency_ci95: f64,
    p95_latency_mean: f64,
    p95_latency_stddev: f64,
    p95_latency_ci95: f64,
    p99_latency_mean: f64,
    p99_latency_stddev: f64,
    p99_latency_ci95: f64,
    throughput_mean: f64,
    throughput_stddev: f64,
    throughput_ci95: f64,
    error_rate_mean: f64,
    error_rate_stddev: f64,
    error_rate_ci95: f64,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    fs::create_dir_all(&args.out)?;
    let metrics: Vec<String> = args
        .metrics
        .split(',')
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    let rows = read_stats(&args.stats_csv)?;
    for metric in metrics {
        plot_metric(&rows, &metric, &args.out)?;
    }
    Ok(())
}

fn read_stats(path: &Path) -> anyhow::Result<Vec<StatsRow>> {
    let mut reader = csv::Reader::from_path(path)
        .with_context(|| format!("failed to open stats csv {}", path.display()))?;
    let mut out = Vec::new();
    for row in reader.deserialize() {
        let row: StatsRow = row?;
        if row.n == 0 {
            continue;
        }
        out.push(row);
    }
    Ok(out)
}

fn metric_value(row: &StatsRow, metric: &str) -> Option<f64> {
    match metric {
        "avg_latency_mean" => Some(row.avg_latency_mean),
        "p95_latency_mean" => Some(row.p95_latency_mean),
        "p99_latency_mean" => Some(row.p99_latency_mean),
        "throughput_mean" => Some(row.throughput_mean),
        "error_rate_mean" => Some(row.error_rate_mean),
        _ => None,
    }
}

fn plot_metric(rows: &[StatsRow], metric: &str, out_dir: &Path) -> anyhow::Result<()> {
    let mut by_group: BTreeMap<(String, String), Vec<&StatsRow>> = BTreeMap::new();
    for row in rows {
        if metric_value(row, metric).is_none() {
            continue;
        }
        by_group
            .entry((row.topology.clone(), row.scenario.clone()))
            .or_default()
            .push(row);
    }

    for ((topology, scenario), group_rows) in by_group {
        let mut loads = BTreeSet::new();
        let mut policies = BTreeSet::new();
        for row in &group_rows {
            loads.insert(row.requests_per_tick);
            policies.insert(row.policy.clone());
        }
        if loads.len() < 2 {
            continue;
        }

        let load_vals: Vec<u64> = loads.into_iter().collect();
        let min_load = *load_vals.first().unwrap();
        let max_load = *load_vals.last().unwrap();

        let mut y_min = f64::INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        for row in &group_rows {
            let y = metric_value(row, metric).unwrap();
            y_min = y_min.min(y);
            y_max = y_max.max(y);
        }
        if !y_min.is_finite() || !y_max.is_finite() {
            continue;
        }
        if (y_max - y_min).abs() < 1e-9 {
            y_max = y_min + 1.0;
        }

        let filename = format!(
            "{}__{}__{}.svg",
            sanitize(&topology),
            sanitize(&scenario),
            sanitize(metric)
        );
        let path = out_dir.join(filename);

        let root = SVGBackend::new(&path, (1100, 700)).into_drawing_area();
        root.fill(&WHITE)?;
        let title = format!("{metric} | {topology} | {scenario}");
        let mut chart = ChartBuilder::on(&root)
            .margin(20)
            .caption(title, ("sans-serif", 24))
            .set_label_area_size(LabelAreaPosition::Left, 60)
            .set_label_area_size(LabelAreaPosition::Bottom, 50)
            .build_cartesian_2d(min_load..max_load, y_min..y_max)?;

        chart
            .configure_mesh()
            .x_desc("requests_per_tick")
            .y_desc(metric)
            .label_style(("sans-serif", 14))
            .draw()?;

        for (idx, policy) in policies.into_iter().enumerate() {
            let series_color = Palette99::pick(idx).mix(0.9);
            let mut points: Vec<(u64, f64)> = group_rows
                .iter()
                .filter(|r| r.policy == policy)
                .filter_map(|r| Some((r.requests_per_tick, metric_value(r, metric)?)))
                .collect();
            points.sort_by_key(|(x, _)| *x);
            if points.len() < 2 {
                continue;
            }
            chart
                .draw_series(LineSeries::new(points.clone(), &series_color))?
                .label(policy)
                .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 18, y)], &series_color));
            chart.draw_series(points.into_iter().map(|(x, y)| {
                Circle::new((x, y), 3, series_color.filled())
            }))?;
        }

        chart
            .configure_series_labels()
            .border_style(&BLACK)
            .label_font(("sans-serif", 14))
            .position(SeriesLabelPosition::UpperRight)
            .draw()?;
        root.present()?;
    }

    Ok(())
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => c,
            _ => '_',
        })
        .collect()
}

