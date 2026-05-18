//! CLI entry point for `micro-net-lab-rs`.

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use micro_net_algorithms::{LeastInflightPolicy, RandomPolicy, RoundRobinPolicy, ScorePolicyV1};
use micro_net_core::*;
use micro_net_drivers::InMemorySimulationEngine;
use micro_net_petgraph::{
    BasicTopologyGenerator, GeneratedTopologyConfig, PetgraphBackend, TopologyGenerator,
};
use micro_net_report::{write_aggregate_csv, write_json_pretty, write_summary_csv};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

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
    /// Requests generated on each tick.
    #[arg(long, default_value_t = 5)]
    requests_per_tick: u64,
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
    /// Comma-separated topology kinds.
    #[arg(long, default_value = "star,ring,full-mesh,random-sparse")]
    topologies: String,
    /// Comma-separated policies.
    #[arg(long, default_value = "random,round-robin,least-inflight,score")]
    policies: String,
    /// Number of seeds, starting from 1.
    #[arg(long, default_value_t = 3)]
    seeds: u64,
    /// Number of logical services in generated topologies.
    #[arg(long, default_value_t = 3)]
    logical_services: usize,
    /// Number of replicas per logical service.
    #[arg(long, default_value_t = 3)]
    replicas_per_service: usize,
    /// Simulation duration in ticks.
    #[arg(long, default_value_t = 100)]
    duration_ticks: u64,
    /// Requests generated on each tick.
    #[arg(long, default_value_t = 5)]
    requests_per_tick: u64,
    /// Parallelism. `1` uses sequential execution; values > 1 use Rayon.
    #[arg(long, default_value_t = 1)]
    parallel: usize,
    /// Output batch directory.
    #[arg(long)]
    out: PathBuf,
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::GenerateTopology(args) => generate_topology(args),
        Command::Run(args) => run_once(args),
        Command::Bench(args) => bench(args),
    }
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
        policy: policy_name(args.policy).into(),
        requests_per_tick: args.requests_per_tick,
        source: NodeId::new(args.source),
        targets,
    };

    let summary = execute_experiment(&topology, &experiment, args.policy, &args.out)?;
    println!(
        "completed: policy={} created={} completed={} failed={} p95={:.2}ms",
        summary.policy, summary.created, summary.completed, summary.failed, summary.p95_latency_ms
    );
    Ok(())
}

fn bench(args: BenchArgs) -> anyhow::Result<()> {
    fs::create_dir_all(&args.out)?;
    let topology_kinds = parse_topologies(&args.topologies)?;
    let policies = parse_policies(&args.policies)?;
    let mut jobs = Vec::new();
    for kind in topology_kinds {
        for seed in 1..=args.seeds {
            let topology = BasicTopologyGenerator.generate(&GeneratedTopologyConfig {
                kind,
                logical_services: args.logical_services,
                replicas_per_service: args.replicas_per_service,
                seed,
            });
            for policy in &policies {
                let service_targets = topology
                    .logical_services
                    .iter()
                    .map(|s| s.id.clone())
                    .collect::<Vec<_>>();
                let experiment_id = format!("{:?}-{}-seed-{seed}", kind, policy_name(*policy))
                    .to_lowercase()
                    .replace(' ', "-");
                let experiment = ExperimentSpec {
                    schema_version: "0.1".into(),
                    id: ExperimentId::new(experiment_id),
                    seed,
                    duration_ticks: args.duration_ticks,
                    policy: policy_name(*policy).into(),
                    requests_per_tick: args.requests_per_tick,
                    source: NodeId::new("gateway-1"),
                    targets: service_targets,
                };
                jobs.push((topology.clone(), experiment, *policy));
            }
        }
    }

    let experiments_dir = args.out.join("experiments");
    fs::create_dir_all(&experiments_dir)?;
    let summaries = if args.parallel <= 1 {
        micro_net_executor::run_sequential(jobs, |(topology, experiment, policy)| {
            let out = experiments_dir.join(experiment.id.as_str());
            execute_experiment(&topology, &experiment, policy, &out)
        })?
    } else {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(args.parallel)
            .build()?;
        pool.install(|| {
            micro_net_executor::run_rayon(jobs, |(topology, experiment, policy)| {
                let out = experiments_dir.join(experiment.id.as_str());
                execute_experiment(&topology, &experiment, policy, &out)
            })
        })?
    };

    write_json_pretty(args.out.join("aggregate.json"), &summaries)?;
    write_aggregate_csv(args.out.join("aggregate.csv"), &summaries)?;
    println!("bench completed: {} experiments", summaries.len());
    Ok(())
}

fn execute_experiment(
    topology: &TopologySpec,
    experiment: &ExperimentSpec,
    policy_kind: CliPolicy,
    out: &Path,
) -> anyhow::Result<SimulationSummary> {
    fs::create_dir_all(out)?;
    write_json_pretty(out.join("topology.json"), topology)?;
    write_json_pretty(out.join("experiment.json"), experiment)?;
    let graph = PetgraphBackend::from_topology(topology.clone())?;
    let engine = InMemorySimulationEngine::new(topology.clone(), graph);
    let trace_file = File::create(out.join("trace.jsonl"))?;
    let mut sink = JsonlTraceSink::new(trace_file);
    let mut policy = build_policy(policy_kind, experiment.seed);
    let mut workload = ConstantWorkloadGenerator::new(ConstantWorkloadConfig {
        requests_per_tick: experiment.requests_per_tick,
        source: experiment.source.clone(),
        targets: experiment.targets.clone(),
        class: RequestClassId::new("default"),
    });
    let summary = engine.run(experiment, policy.as_mut(), &mut workload, &mut sink)?;
    write_json_pretty(out.join("summary.json"), &summary)?;
    write_summary_csv(out.join("metrics.csv"), &summary)?;
    Ok(summary)
}

fn build_policy(kind: CliPolicy, seed: u64) -> Box<dyn RoutingPolicy> {
    match kind {
        CliPolicy::Random => Box::new(RandomPolicy::new(seed)),
        CliPolicy::RoundRobin => Box::new(RoundRobinPolicy::new()),
        CliPolicy::LeastInflight => Box::new(LeastInflightPolicy::new()),
        CliPolicy::Score => Box::new(ScorePolicyV1::dependency_aware_default()),
    }
}

fn policy_name(kind: CliPolicy) -> &'static str {
    match kind {
        CliPolicy::Random => "random",
        CliPolicy::RoundRobin => "round-robin",
        CliPolicy::LeastInflight => "least-inflight",
        CliPolicy::Score => "score-v1",
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
            other => anyhow::bail!("unknown policy: {other}"),
        })
        .collect()
}
