//! CLI entry point for `micro-net-lab-rs`.

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use micro_net_algorithms::{LeastInflightPolicy, RandomPolicy, RoundRobinPolicy, ScorePolicyV1};
use micro_net_core::*;
use micro_net_drivers::InMemorySimulationEngine;
use micro_net_petgraph::{
    BasicTopologyGenerator, GeneratedTopologyConfig, PetgraphBackend, TopologyGenerator,
};
use micro_net_report::{
    write_aggregate_csv, write_effect_sizes_csv, write_grouped_stats_csv, write_json_pretty,
    write_summary_csv,
};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command as SysCommand;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// CLI for deterministic microservice routing experiments.
#[derive(Debug, Parser)]
#[command(name = "micro-net")]
#[command(about = "Research CLI for microservice routing/load-balancing simulation")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

/// Supported CLI commands.
#[derive(Debug, Subcommand)]
enum Command {
    /// Generate a typed topology JSON file.
    GenerateTopology(GenerateTopologyArgs),
    /// Run one deterministic experiment.
    Run(RunArgs),
    /// Run N policies × M seeds experiments and aggregate results.
    Bench(BenchArgs),
    /// Merge sharded bench outputs (aggregate.json + metadata) into one run directory.
    Merge(MergeArgs),
}

/// Topology generation arguments.
#[derive(Debug, Parser)]
struct GenerateTopologyArgs {
    /// Topology shape.
    #[arg(long, value_enum, default_value = "star")]
    kind: CliTopologyKind,
    /// Number of logical services.
    #[arg(long, default_value_t = 3)]
    logical_services: usize,
    /// Number of replicas per logical service.
    #[arg(long, default_value_t = 3)]
    replicas_per_service: usize,
    /// Deterministic seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Output topology file.
    #[arg(long)]
    out: PathBuf,
}

/// Single run arguments.
#[derive(Debug, Parser)]
struct RunArgs {
    /// Input topology JSON.
    #[arg(long)]
    topology: PathBuf,
    /// Routing policy.
    #[arg(long, value_enum, default_value = "random")]
    policy: CliPolicy,
    /// Deterministic seed.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// Simulation duration in ticks.
    #[arg(long, default_value_t = 100)]
    duration_ticks: u64,
    /// Warmup ticks before measurement.
    #[arg(long, default_value_t = 0)]
    warmup_ticks: u64,
    /// Drain ticks at the end with no new requests.
    #[arg(long, default_value_t = 0)]
    drain_ticks: u64,
    /// Requests generated on each tick.
    #[arg(long, default_value_t = 5)]
    requests_per_tick: u64,
    /// Synthetic degradation scenario.
    #[arg(long, default_value = "healthy")]
    scenario: String,
    /// Observability lag (ticks) for policy-visible runtime metrics.
    #[arg(long, default_value_t = 5)]
    observability_lag_ticks: u64,
    /// Observability noise (stddev) for policy-visible runtime metrics.
    #[arg(long, default_value_t = 0.02)]
    observability_noise: f64,
    /// Source node.
    #[arg(long, default_value = "gateway-1")]
    source: String,
    /// Optional comma-separated target logical services. Defaults to all services in topology.
    #[arg(long)]
    target_services: Option<String>,
    /// Output experiment directory.
    #[arg(long)]
    out: PathBuf,
}

/// Batch run arguments.
#[derive(Debug, Parser)]
struct BenchArgs {
    /// Optional TOML config file to populate bench parameters.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Comma-separated topology kinds.
    #[arg(long, default_value = "star,ring,full-mesh,random-sparse")]
    topologies: String,
    /// Comma-separated policies.
    #[arg(long, default_value = "random,round-robin,least-inflight,score")]
    policies: String,
    /// Number of seeds, starting from 1.
    #[arg(long, default_value_t = 3)]
    seeds: u64,
    /// Comma-separated degradation scenarios.
    #[arg(long, default_value = "healthy,db-overloaded,cache-degraded,broker-lag,zone-burst,partial-failure")]
    scenarios: String,
    /// Comma-separated request rates per tick.
    #[arg(long, default_value = "1,5,10,25,50")]
    load_levels: String,
    /// Number of logical services in generated topologies.
    #[arg(long, default_value_t = 3)]
    logical_services: usize,
    /// Number of replicas per logical service.
    #[arg(long, default_value_t = 3)]
    replicas_per_service: usize,
    /// Simulation duration in ticks.
    #[arg(long, default_value_t = 100)]
    duration_ticks: u64,
    /// Warmup ticks before measurement.
    #[arg(long, default_value_t = 100)]
    warmup_ticks: u64,
    /// Drain ticks at the end with no new requests.
    #[arg(long, default_value_t = 50)]
    drain_ticks: u64,
    /// Observability lag (ticks) for policy-visible runtime metrics.
    #[arg(long, default_value_t = 5)]
    observability_lag_ticks: u64,
    /// Observability noise (stddev) for policy-visible runtime metrics.
    #[arg(long, default_value_t = 0.02)]
    observability_noise: f64,
    /// Requests generated on each tick.
    #[arg(long, default_value_t = 5)]
    requests_per_tick: u64,
    /// Parallelism. `1` uses sequential execution; values > 1 use Rayon.
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    /// Optional sharding: only run experiments whose id hash matches this shard.
    #[arg(long)]
    shard_index: Option<u64>,
    /// Optional sharding: number of shards to split the full run into.
    #[arg(long)]
    shard_count: Option<u64>,
    /// Print shell commands for all shards (0..shard-count) and exit.
    #[arg(long, default_value_t = false)]
    print_shard_commands: bool,
    /// Output base directory used by `--print-shard-commands` (defaults to `--out`).
    #[arg(long)]
    out_base: Option<PathBuf>,
    /// Optional provider label for metadata (e.g., "aws", "gcp", "local").
    #[arg(long)]
    provider: Option<String>,
    /// Optional VM/instance type label for metadata.
    #[arg(long)]
    vm_type: Option<String>,
    /// Trace output mode. `none` is recommended for large publication runs.
    #[arg(long, value_enum, default_value = "none")]
    trace: TraceMode,
    /// Artifact output mode. `aggregate` writes only aggregate/stats CSVs.
    #[arg(long, value_enum, default_value = "aggregate")]
    artifacts: ArtifactMode,
    /// Progress reporting interval in milliseconds. `0` disables progress output.
    #[arg(long, default_value_t = 1000)]
    progress_ms: u64,
    /// Output batch directory.
    #[arg(long)]
    out: Option<PathBuf>,
}

/// Merge sharded outputs into one run directory.
#[derive(Debug, Parser)]
struct MergeArgs {
    /// One or more shard output directories that contain `aggregate.json` and `run_metadata.json`.
    #[arg(long, required = true, num_args = 1..)]
    inputs: Vec<PathBuf>,
    /// Output directory for the merged run.
    #[arg(long)]
    out: PathBuf,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
enum TraceMode {
    None,
    Jsonl,
}

#[derive(Debug, Clone, Copy, ValueEnum, serde::Serialize, serde::Deserialize)]
enum ArtifactMode {
    Aggregate,
    Experiments,
}

/// CLI topology kind enum.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliTopologyKind {
    Star,
    Ring,
    FullMesh,
    RandomSparse,
}

impl From<CliTopologyKind> for TopologyKind {
    fn from(value: CliTopologyKind) -> Self {
        match value {
            CliTopologyKind::Star => TopologyKind::Star,
            CliTopologyKind::Ring => TopologyKind::Ring,
            CliTopologyKind::FullMesh => TopologyKind::FullMesh,
            CliTopologyKind::RandomSparse => TopologyKind::RandomSparse,
        }
    }
}

/// Built-in policies available from CLI.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliPolicy {
    Random,
    RoundRobin,
    LeastInflight,
    Score,
    ScoreLocalOnly,
    ScoreLocalNetwork,
    ScoreLocalDownstream,
    ScoreNoDownstream,
    ScoreNoHostPressure,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenerateTopology(args) => generate_topology(args),
        Command::Run(args) => run_once(args),
        Command::Bench(args) => bench(args),
        Command::Merge(args) => merge(args),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RunMetadata {
    schema_version: String,
    run_fingerprint_fnv64: String,
    created_at_utc: String,
    git_commit: Option<String>,
    cli_commandline: Vec<String>,
    provider: Option<String>,
    vm_type: Option<String>,
    parallel: usize,
    shard_index: Option<u64>,
    shard_count: Option<u64>,
    seed_start: u64,
    seed_end: u64,
    seeds: u64,
    topologies: String,
    policies: String,
    scenarios: String,
    load_levels: String,
    logical_services: usize,
    replicas_per_service: usize,
    duration_ticks: u64,
    warmup_ticks: u64,
    drain_ticks: u64,
    observability_lag_ticks: u64,
    observability_noise: f64,
    trace: TraceMode,
    artifacts: ArtifactMode,
    rustc_vv: Option<String>,
    uname_a: Option<String>,
    lscpu: Option<String>,
    planned_experiments_total: u64,
    selected_experiments: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
struct BenchFingerprint<'a> {
    // Everything that defines "the same run" for sharding/merge compatibility.
    topologies: &'a str,
    policies: &'a str,
    seeds: u64,
    scenarios: &'a str,
    load_levels: &'a str,
    logical_services: usize,
    replicas_per_service: usize,
    duration_ticks: u64,
    warmup_ticks: u64,
    drain_ticks: u64,
    observability_lag_ticks: u64,
    observability_noise: f64,
    trace: TraceMode,
    artifacts: ArtifactMode,
}

fn stable_fnv1a_64(bytes: &[u8]) -> u64 {
    // FNV-1a 64-bit.
    let mut hash: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn run_fingerprint(args: &BenchArgs) -> anyhow::Result<u64> {
    let fp = BenchFingerprint {
        topologies: args.topologies.as_str(),
        policies: args.policies.as_str(),
        seeds: args.seeds,
        scenarios: args.scenarios.as_str(),
        load_levels: args.load_levels.as_str(),
        logical_services: args.logical_services,
        replicas_per_service: args.replicas_per_service,
        duration_ticks: args.duration_ticks,
        warmup_ticks: args.warmup_ticks,
        drain_ticks: args.drain_ticks,
        observability_lag_ticks: args.observability_lag_ticks,
        observability_noise: args.observability_noise,
        trace: args.trace,
        artifacts: args.artifacts,
    };
    let json = serde_json::to_string(&fp)?;
    Ok(stable_fnv1a_64(json.as_bytes()))
}

fn now_utc_rfc3339() -> String {
    // Prefer system `date` for a human-readable RFC3339 timestamp; fall back to epoch seconds.
    if let Some(s) = capture_cmd_stdout("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]) {
        return s;
    }
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("unix:{secs}")
}

fn capture_cmd_stdout(cmd: &str, args: &[&str]) -> Option<String> {
    let out = SysCommand::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn capture_cmd_stdout_stderr(cmd: &str, args: &[&str]) -> Option<String> {
    let out = SysCommand::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let mut s = String::new();
    s.push_str(String::from_utf8_lossy(&out.stdout).as_ref());
    if !out.stderr.is_empty() {
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s.push_str(String::from_utf8_lossy(&out.stderr).as_ref());
    }
    let s = s.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct BenchTomlConfig {
    /// Optional config schema version for forward compatibility.
    #[serde(default)]
    schema_version: Option<String>,
    topologies: Vec<String>,
    policies: Vec<String>,
    scenarios: Vec<String>,
    load_levels: Vec<u64>,
    seeds: u64,
    #[serde(default)]
    logical_services: Option<usize>,
    #[serde(default)]
    replicas_per_service: Option<usize>,
    duration_ticks: u64,
    warmup_ticks: u64,
    drain_ticks: u64,
    observability_lag_ticks: u64,
    observability_noise: f64,
    #[serde(default)]
    requests_per_tick: Option<u64>,
    #[serde(default)]
    parallel: Option<usize>,
    trace: String,
    artifacts: String,
    progress_ms: u64,
    #[serde(default)]
    out: Option<PathBuf>,
}

fn apply_bench_config(mut args: BenchArgs, path: &Path) -> anyhow::Result<BenchArgs> {
    let preserved = (
        args.shard_index,
        args.shard_count,
        args.provider.clone(),
        args.vm_type.clone(),
        args.out.clone(),
        args.out_base.clone(),
        args.print_shard_commands,
    );

    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read config {}", path.display()))?;
    let cfg: BenchTomlConfig =
        toml::from_str(&raw).with_context(|| format!("failed to parse TOML {}", path.display()))?;

    let trace = match cfg.trace.as_str() {
        "none" => TraceMode::None,
        "jsonl" => TraceMode::Jsonl,
        other => anyhow::bail!("unknown trace mode in config: {other}"),
    };
    let artifacts = match cfg.artifacts.as_str() {
        "aggregate" => ArtifactMode::Aggregate,
        "experiments" => ArtifactMode::Experiments,
        other => anyhow::bail!("unknown artifacts mode in config: {other}"),
    };

    args.topologies = cfg.topologies.join(",");
    args.policies = cfg.policies.join(",");
    args.scenarios = cfg.scenarios.join(",");
    args.load_levels = cfg
        .load_levels
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",");
    args.seeds = cfg.seeds;
    if let Some(v) = cfg.logical_services {
        args.logical_services = v;
    }
    if let Some(v) = cfg.replicas_per_service {
        args.replicas_per_service = v;
    }
    args.duration_ticks = cfg.duration_ticks;
    args.warmup_ticks = cfg.warmup_ticks;
    args.drain_ticks = cfg.drain_ticks;
    args.observability_lag_ticks = cfg.observability_lag_ticks;
    args.observability_noise = cfg.observability_noise;
    args.requests_per_tick = cfg.requests_per_tick.unwrap_or(args.requests_per_tick);
    args.parallel = cfg.parallel.unwrap_or(args.parallel);
    args.trace = trace;
    args.artifacts = artifacts;
    args.progress_ms = cfg.progress_ms;
    if args.out.is_none() {
        args.out = cfg.out;
    }

    // Restore non-config controls / overrides.
    args.shard_index = preserved.0;
    args.shard_count = preserved.1;
    args.provider = preserved.2;
    args.vm_type = preserved.3;
    if preserved.4.is_some() {
        args.out = preserved.4;
    }
    args.out_base = preserved.5;
    args.print_shard_commands = preserved.6;

    Ok(args)
}

fn generate_topology(args: GenerateTopologyArgs) -> anyhow::Result<()> {
    let generator = BasicTopologyGenerator;
    let topology = generator.generate(&GeneratedTopologyConfig {
        kind: args.kind.into(),
        logical_services: args.logical_services,
        replicas_per_service: args.replicas_per_service,
        seed: args.seed,
    });
    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent)?;
    }
    write_json_pretty(&args.out, &topology)?;
    println!("wrote topology to {}", args.out.display());
    Ok(())
}

fn run_once(args: RunArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.out)?;
    let topology = read_topology(&args.topology)?;
    let targets = parse_targets(args.target_services, &topology);
    let experiment = ExperimentSpec {
        schema_version: "0.1".into(),
        id: ExperimentId::new("experiment-001"),
        seed: args.seed,
        duration_ticks: args.duration_ticks,
        warmup_ticks: args.warmup_ticks,
        drain_ticks: args.drain_ticks,
        policy: policy_name(args.policy).into(),
        scenario: args.scenario,
        observability_lag_ticks: args.observability_lag_ticks,
        observability_noise: args.observability_noise,
        requests_per_tick: args.requests_per_tick,
        source: NodeId::new(args.source),
        targets,
    };

    let summary = execute_experiment(
        &topology,
        &experiment,
        args.policy,
        &args.out,
        TraceMode::Jsonl,
        ArtifactMode::Experiments,
    )?;
    println!(
        "completed: policy={} created={} completed={} failed={} p95={:.2}ms",
        summary.policy, summary.created, summary.completed, summary.failed, summary.p95_latency_ms
    );
    Ok(())
}

fn bench(args: BenchArgs) -> anyhow::Result<()> {
    // Load TOML config if provided. CLI flags (sharding/provider/vm/out) can override it.
    let args = if let Some(cfg_path) = args.config.clone() {
        apply_bench_config(args, &cfg_path)?
    } else {
        args
    };

    if args.print_shard_commands {
        let Some(cnt) = args.shard_count else {
            anyhow::bail!("--print-shard-commands requires --shard-count");
        };
        if cnt == 0 {
            anyhow::bail!("--shard-count must be > 0");
        }
        let base = args
            .out_base
            .clone()
            .or_else(|| args.out.clone())
            .ok_or_else(|| anyhow::anyhow!("missing output base: pass --out-base or --out (or set out in config)"))?;
        let trace = match args.trace {
            TraceMode::None => "none",
            TraceMode::Jsonl => "jsonl",
        };
        let artifacts = match args.artifacts {
            ArtifactMode::Aggregate => "aggregate",
            ArtifactMode::Experiments => "experiments",
        };
        for idx in 0..cnt {
            println!(
                "cargo run -p micro-net-cli -- bench \\\n  --topologies {topologies} \\\n  --policies {policies} \\\n  --scenarios {scenarios} \\\n  --load-levels {load_levels} \\\n  --seeds {seeds} \\\n  --logical-services {logical_services} --replicas-per-service {replicas_per_service} \\\n  --duration-ticks {duration_ticks} --warmup-ticks {warmup_ticks} --drain-ticks {drain_ticks} \\\n  --observability-lag-ticks {observability_lag_ticks} --observability-noise {observability_noise} \\\n  --parallel {parallel} --trace {trace} --artifacts {artifacts} --progress-ms {progress_ms} \\\n  {provider}{vm_type}--shard-index {idx} --shard-count {cnt} \\\n  --out {base}/shard-{idx}\n",
                topologies = args.topologies,
                policies = args.policies,
                scenarios = args.scenarios,
                load_levels = args.load_levels,
                seeds = args.seeds,
                logical_services = args.logical_services,
                replicas_per_service = args.replicas_per_service,
                duration_ticks = args.duration_ticks,
                warmup_ticks = args.warmup_ticks,
                drain_ticks = args.drain_ticks,
                observability_lag_ticks = args.observability_lag_ticks,
                observability_noise = args.observability_noise,
                parallel = args.parallel,
                progress_ms = args.progress_ms,
                provider = args
                    .provider
                    .as_ref()
                    .map(|p| format!("--provider {p} \\\n  "))
                    .unwrap_or_default(),
                vm_type = args
                    .vm_type
                    .as_ref()
                    .map(|t| format!("--vm-type {t} \\\n  "))
                    .unwrap_or_default(),
                base = base.display(),
            );
        }
        return Ok(());
    }

    let out_dir: PathBuf = args
        .out
        .clone()
        .ok_or_else(|| anyhow::anyhow!("missing --out (or set out in --config)"))?;
    fs::create_dir_all(&out_dir)?;

    // Sharding validation.
    match (args.shard_index, args.shard_count) {
        (None, None) => {}
        (Some(_), None) | (None, Some(_)) => {
            anyhow::bail!("sharding requires both --shard-index and --shard-count");
        }
        (Some(idx), Some(cnt)) => {
            if cnt == 0 {
                anyhow::bail!("--shard-count must be > 0");
            }
            if idx >= cnt {
                anyhow::bail!("--shard-index must be in [0, shard_count)");
            }
        }
    }

    let topology_kinds = parse_topologies(&args.topologies)?;
    let policies = parse_policies(&args.policies)?;
    let scenarios = parse_scenarios(&args.scenarios)?;
    let load_levels = parse_load_levels(&args.load_levels)?;
    let mut jobs = Vec::new();
    let mut planned_total: u64 = 0;
    for kind in topology_kinds {
        for seed in 1..=args.seeds {
            let topology = BasicTopologyGenerator.generate(&GeneratedTopologyConfig {
                kind,
                logical_services: args.logical_services,
                replicas_per_service: args.replicas_per_service,
                seed,
            });
            for policy in &policies {
                for scenario in &scenarios {
                    for requests_per_tick in &load_levels {
                        planned_total += 1;
                        let service_targets = topology
                            .logical_services
                            .iter()
                            .map(|s| s.id.clone())
                            .collect::<Vec<_>>();
                        let topo_key = format!(
                            "{:?}:ls{}:rep{}:seed{}",
                            kind, args.logical_services, args.replicas_per_service, seed
                        );
                        let exp_key = format!(
                            "{topo_key}|{}|{}|load={}|seed={}",
                            policy_name(*policy),
                            scenario,
                            requests_per_tick,
                            seed
                        );
                        let exp_hash = stable_fnv1a_64(exp_key.as_bytes());
                        if let (Some(idx), Some(cnt)) = (args.shard_index, args.shard_count) {
                            if exp_hash % cnt != idx {
                                continue;
                            }
                        }
                        // Include the stable hash prefix so that sharding and de-duplication are robust.
                        let experiment_id = format!(
                            "exp-{exp_hash:016x}-{:?}-{}-{}-load-{requests_per_tick}-seed-{seed}",
                            kind,
                            policy_name(*policy),
                            scenario
                        )
                        .to_lowercase()
                        .replace(' ', "-");
                        let experiment = ExperimentSpec {
                            schema_version: "0.1".into(),
                            id: ExperimentId::new(experiment_id),
                            seed,
                            duration_ticks: args.duration_ticks,
                            warmup_ticks: args.warmup_ticks,
                            drain_ticks: args.drain_ticks,
                            policy: policy_name(*policy).into(),
                            scenario: scenario.clone(),
                            observability_lag_ticks: args.observability_lag_ticks,
                            observability_noise: args.observability_noise,
                            requests_per_tick: *requests_per_tick,
                            source: NodeId::new("gateway-1"),
                            targets: service_targets,
                        };
                        jobs.push((topology.clone(), experiment, *policy));
                    }
                }
            }
        }
    }

    let experiments_dir = out_dir.join("experiments");
    if matches!(args.artifacts, ArtifactMode::Experiments) {
        fs::create_dir_all(&experiments_dir)?;
    }
    // Write run metadata early so shards are self-describing.
    let started_hms = capture_cmd_stdout("date", &["-u", "+%H:%M:%SZ"]).unwrap_or_else(|| "?".to_string());
    let fingerprint = run_fingerprint(&args)?;
    let meta = RunMetadata {
        schema_version: "0.1".to_string(),
        run_fingerprint_fnv64: format!("{fingerprint:016x}"),
        created_at_utc: now_utc_rfc3339(),
        git_commit: capture_cmd_stdout("git", &["rev-parse", "HEAD"]),
        cli_commandline: std::env::args().collect(),
        provider: args.provider.clone(),
        vm_type: args.vm_type.clone(),
        parallel: args.parallel,
        shard_index: args.shard_index,
        shard_count: args.shard_count,
        seed_start: 1,
        seed_end: args.seeds,
        seeds: args.seeds,
        topologies: args.topologies.clone(),
        policies: args.policies.clone(),
        scenarios: args.scenarios.clone(),
        load_levels: args.load_levels.clone(),
        logical_services: args.logical_services,
        replicas_per_service: args.replicas_per_service,
        duration_ticks: args.duration_ticks,
        warmup_ticks: args.warmup_ticks,
        drain_ticks: args.drain_ticks,
        observability_lag_ticks: args.observability_lag_ticks,
        observability_noise: args.observability_noise,
        trace: args.trace,
        artifacts: args.artifacts,
        rustc_vv: capture_cmd_stdout_stderr("rustc", &["-Vv"]),
        uname_a: capture_cmd_stdout("uname", &["-a"]),
        lscpu: capture_cmd_stdout("lscpu", &[]),
        planned_experiments_total: planned_total,
        selected_experiments: jobs.len() as u64,
    };
    write_json_pretty(out_dir.join("run_metadata.json"), &meta)?;

    let total_jobs = jobs.len() as u64;
    let done = AtomicU64::new(0);
    let started = Instant::now();
    let last_report = std::sync::Mutex::new(Instant::now());
    let report_every = Duration::from_millis(args.progress_ms);

    let maybe_report = || {
        if args.progress_ms == 0 {
            return;
        }
        let mut guard = match last_report.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let now = Instant::now();
        let last = *guard;
        if now.duration_since(last) < report_every {
            return;
        }
        *guard = now;
        let done_now = done.load(Ordering::Relaxed);
        let elapsed = started.elapsed().as_secs_f64().max(0.001);
        let rate = done_now as f64 / elapsed;
        let remaining = (total_jobs.saturating_sub(done_now)) as f64;
        let eta_s = if rate > 0.0 { remaining / rate } else { f64::INFINITY };
        let now_utc = capture_cmd_stdout("date", &["-u", "+%H:%M:%SZ"]).unwrap_or_else(|| "?".to_string());
        eprintln!(
            "progress: {done_now}/{total_jobs} ({:.1}%) elapsed={:.1}m start={started_hms} now={now_utc} rate={:.2} exp/s eta={:.1}m",
            (done_now as f64 / total_jobs.max(1) as f64) * 100.0,
            elapsed / 60.0,
            rate,
            eta_s / 60.0
        );
    };

    let summaries = if args.parallel <= 1 {
        micro_net_executor::run_sequential(jobs, |(topology, experiment, policy)| {
            let out = experiments_dir.join(experiment.id.as_str());
            let result = execute_experiment(
                &topology,
                &experiment,
                policy,
                &out,
                args.trace,
                args.artifacts,
            );
            done.fetch_add(1, Ordering::Relaxed);
            maybe_report();
            result
        })?
    } else {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.parallel)
            .build()?;
        pool.install(|| {
            micro_net_executor::run_rayon(jobs, |(topology, experiment, policy)| {
                let out = experiments_dir.join(experiment.id.as_str());
                let result = execute_experiment(
                    &topology,
                    &experiment,
                    policy,
                    &out,
                    args.trace,
                    args.artifacts,
                );
                done.fetch_add(1, Ordering::Relaxed);
                maybe_report();
                result
            })
        })?
    };

    write_json_pretty(out_dir.join("aggregate.json"), &summaries)?;
    write_aggregate_csv(out_dir.join("aggregate.csv"), &summaries)?;
    write_grouped_stats_csv(out_dir.join("stats.csv"), &summaries)?;
    write_effect_sizes_csv(out_dir.join("effect_sizes.csv"), &summaries, "random")?;
    println!("bench completed: {} experiments", summaries.len());
    Ok(())
}

fn merge(args: MergeArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.out)?;
    let mut all_summaries: Vec<SimulationSummary> = Vec::new();
    let mut expected_fingerprint: Option<String> = None;
    let mut expected_git: Option<String> = None;
    let mut expected_planned: Option<u64> = None;
    let mut expected_shard_count: Option<u64> = None;

    let mut seen: std::collections::BTreeMap<String, SimulationSummary> = std::collections::BTreeMap::new();
    for input in &args.inputs {
        let meta_path = input.join("run_metadata.json");
        let agg_path = input.join("aggregate.json");
        let meta_file = File::open(&meta_path)
            .with_context(|| format!("missing run_metadata.json in {}", input.display()))?;
        let meta: RunMetadata = serde_json::from_reader(meta_file)
            .with_context(|| format!("failed to parse {}", meta_path.display()))?;

        if let Some(fp) = &expected_fingerprint {
            if fp != &meta.run_fingerprint_fnv64 {
                anyhow::bail!(
                    "shard fingerprint mismatch: expected {} but got {} from {}",
                    fp,
                    meta.run_fingerprint_fnv64,
                    input.display()
                );
            }
        } else {
            expected_fingerprint = Some(meta.run_fingerprint_fnv64.clone());
        }

        match (&expected_git, &meta.git_commit) {
            (Some(exp), Some(got)) if exp != got => {
                anyhow::bail!(
                    "git commit mismatch across shards: expected {} but got {} from {}",
                    exp,
                    got,
                    input.display()
                );
            }
            (None, Some(got)) => expected_git = Some(got.clone()),
            _ => {}
        }

        if let Some(p) = expected_planned {
            if meta.planned_experiments_total != p {
                anyhow::bail!(
                    "planned experiment count mismatch across shards: expected {} but got {} from {}",
                    p,
                    meta.planned_experiments_total,
                    input.display()
                );
            }
        } else {
            expected_planned = Some(meta.planned_experiments_total);
        }

        if let Some(cnt) = meta.shard_count {
            if let Some(exp_cnt) = expected_shard_count {
                if cnt != exp_cnt {
                    anyhow::bail!(
                        "shard_count mismatch across shards: expected {} but got {} from {}",
                        exp_cnt,
                        cnt,
                        input.display()
                    );
                }
            } else {
                expected_shard_count = Some(cnt);
            }
        }

        let agg_file =
            File::open(&agg_path).with_context(|| format!("missing {}", agg_path.display()))?;
        let shard_summaries: Vec<SimulationSummary> = serde_json::from_reader(agg_file)
            .with_context(|| format!("failed to parse {}", agg_path.display()))?;
        for s in shard_summaries {
            // De-duplicate by experiment_id to make merges idempotent.
            seen.entry(s.experiment_id.clone()).or_insert(s);
        }
    }

    all_summaries.extend(seen.into_values());
    all_summaries.sort_by(|a, b| a.experiment_id.cmp(&b.experiment_id));

    write_json_pretty(args.out.join("aggregate.json"), &all_summaries)?;
    write_aggregate_csv(args.out.join("aggregate.csv"), &all_summaries)?;
    write_grouped_stats_csv(args.out.join("stats.csv"), &all_summaries)?;
    write_effect_sizes_csv(args.out.join("effect_sizes.csv"), &all_summaries, "random")?;

    // Write merge metadata.
    #[derive(Debug, serde::Serialize)]
    struct MergeMetadata {
        schema_version: String,
        merged_at_utc: String,
        inputs: Vec<String>,
        run_fingerprint_fnv64: Option<String>,
        git_commit: Option<String>,
        merged_experiments: u64,
        planned_experiments_total: Option<u64>,
        shard_count: Option<u64>,
    }
    let merge_meta = MergeMetadata {
        schema_version: "0.1".to_string(),
        merged_at_utc: now_utc_rfc3339(),
        inputs: args.inputs.iter().map(|p| p.display().to_string()).collect(),
        run_fingerprint_fnv64: expected_fingerprint,
        git_commit: expected_git,
        merged_experiments: all_summaries.len() as u64,
        planned_experiments_total: expected_planned,
        shard_count: expected_shard_count,
    };
    write_json_pretty(args.out.join("merge_metadata.json"), &merge_meta)?;

    if let Some(planned) = expected_planned {
        if (all_summaries.len() as u64) != planned {
            eprintln!(
                "warning: merged experiments ({}) != planned_experiments_total ({}) (missing shards or filtered inputs?)",
                all_summaries.len(),
                planned
            );
        }
    }

    println!(
        "merge completed: {} experiments written to {}",
        all_summaries.len(),
        args.out.display()
    );
    Ok(())
}

fn execute_experiment(
    topology: &TopologySpec,
    experiment: &ExperimentSpec,
    policy_kind: CliPolicy,
    out: &Path,
    trace: TraceMode,
    artifacts: ArtifactMode,
) -> anyhow::Result<SimulationSummary> {
    let graph = PetgraphBackend::from_topology(topology.clone())?;
    let engine = InMemorySimulationEngine::new(topology.clone(), graph);
    let mut trace_file: Option<File> = None;
    if matches!(artifacts, ArtifactMode::Experiments) {
        fs::create_dir_all(out)?;
        write_json_pretty(out.join("topology.json"), topology)?;
        write_json_pretty(out.join("experiment.json"), experiment)?;
        if matches!(trace, TraceMode::Jsonl) {
            trace_file = Some(File::create(out.join("trace.jsonl"))?);
        }
    }

    let mut policy = build_policy(policy_kind, experiment.seed);
    let mut workload = ConstantWorkloadGenerator::new(ConstantWorkloadConfig {
        requests_per_tick: experiment.requests_per_tick,
        source: experiment.source.clone(),
        targets: experiment.targets.clone(),
        class: RequestClassId::new("default"),
    });
    let summary = if let Some(file) = trace_file {
        let mut sink = JsonlTraceSink::new(file);
        engine.run(experiment, policy.as_mut(), &mut workload, &mut sink)?
    } else {
        let mut sink = NoopEventSink::default();
        engine.run(experiment, policy.as_mut(), &mut workload, &mut sink)?
    };

    if matches!(artifacts, ArtifactMode::Experiments) {
        write_json_pretty(out.join("summary.json"), &summary)?;
        write_summary_csv(out.join("metrics.csv"), &summary)?;
    }
    Ok(summary)
}

fn build_policy(kind: CliPolicy, seed: u64) -> Box<dyn RoutingPolicy> {
    match kind {
        CliPolicy::Random => Box::new(RandomPolicy::new(seed)),
        CliPolicy::RoundRobin => Box::new(RoundRobinPolicy::new()),
        CliPolicy::LeastInflight => Box::new(LeastInflightPolicy::new()),
        CliPolicy::Score => Box::new(ScorePolicyV1::dependency_aware_default()),
        CliPolicy::ScoreLocalOnly => Box::new(ScorePolicyV1::local_only()),
        CliPolicy::ScoreLocalNetwork => Box::new(ScorePolicyV1::local_plus_network()),
        CliPolicy::ScoreLocalDownstream => Box::new(ScorePolicyV1::local_plus_downstream()),
        CliPolicy::ScoreNoDownstream => Box::new(ScorePolicyV1::without_downstream()),
        CliPolicy::ScoreNoHostPressure => Box::new(ScorePolicyV1::without_host_pressure()),
    }
}

fn policy_name(kind: CliPolicy) -> &'static str {
    match kind {
        CliPolicy::Random => "random",
        CliPolicy::RoundRobin => "round-robin",
        CliPolicy::LeastInflight => "least-inflight",
        CliPolicy::Score => "score-v1",
        CliPolicy::ScoreLocalOnly => "score-local-only",
        CliPolicy::ScoreLocalNetwork => "score-local+network",
        CliPolicy::ScoreLocalDownstream => "score-local+downstream",
        CliPolicy::ScoreNoDownstream => "score-no-downstream",
        CliPolicy::ScoreNoHostPressure => "score-no-host-pressure",
    }
}

fn read_topology(path: &Path) -> anyhow::Result<TopologySpec> {
    let file =
        File::open(path).with_context(|| format!("failed to open topology {}", path.display()))?;
    Ok(serde_json::from_reader(file)?)
}

fn parse_targets(value: Option<String>, topology: &TopologySpec) -> Vec<LogicalServiceId> {
    match value {
        Some(raw) => raw
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| LogicalServiceId::new(s.trim()))
            .collect(),
        None => topology
            .logical_services
            .iter()
            .map(|svc| svc.id.clone())
            .collect(),
    }
}

fn parse_topologies(raw: &str) -> anyhow::Result<Vec<TopologyKind>> {
    raw.split(',')
        .map(|v| match v.trim() {
            "star" => Ok(TopologyKind::Star),
            "ring" => Ok(TopologyKind::Ring),
            "full-mesh" | "full_mesh" => Ok(TopologyKind::FullMesh),
            "random-sparse" | "random_sparse" => Ok(TopologyKind::RandomSparse),
            other => anyhow::bail!("unknown topology kind: {other}"),
        })
        .collect()
}

fn parse_policies(raw: &str) -> anyhow::Result<Vec<CliPolicy>> {
    raw.split(',')
        .map(|v| match v.trim() {
            "random" => Ok(CliPolicy::Random),
            "round-robin" | "round_robin" => Ok(CliPolicy::RoundRobin),
            "least-inflight" | "least_inflight" => Ok(CliPolicy::LeastInflight),
            "score" | "score-v1" | "score_v1" => Ok(CliPolicy::Score),
            "score-local-only" | "score_local_only" => Ok(CliPolicy::ScoreLocalOnly),
            "score-local+network" | "score_local_network" => Ok(CliPolicy::ScoreLocalNetwork),
            "score-local+downstream" | "score_local_downstream" => Ok(CliPolicy::ScoreLocalDownstream),
            "score-no-downstream" | "score_no_downstream" => Ok(CliPolicy::ScoreNoDownstream),
            "score-no-host-pressure" | "score_no_host_pressure" => Ok(CliPolicy::ScoreNoHostPressure),
            other => anyhow::bail!("unknown policy: {other}"),
        })
        .collect()
}

fn parse_scenarios(raw: &str) -> anyhow::Result<Vec<String>> {
    raw.split(',')
        .map(|v| {
            let value = v.trim();
            if value.is_empty() {
                anyhow::bail!("empty scenario value");
            }
            Ok(value.to_string())
        })
        .collect()
}

fn parse_load_levels(raw: &str) -> anyhow::Result<Vec<u64>> {
    raw.split(',')
        .map(|v| {
            let value = v.trim();
            if value.is_empty() {
                anyhow::bail!("empty load level value");
            }
            Ok(value.parse::<u64>()?)
        })
        .collect()
}
