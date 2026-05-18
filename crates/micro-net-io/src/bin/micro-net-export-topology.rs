use anyhow::Context;
use clap::Parser;
use micro_net_core::{Edge, NodeKind, TopologySpec};
use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "micro-net-export-topology")]
#[command(about = "Export a TopologySpec JSON to Graphviz DOT")]
struct Args {
    /// Input topology JSON file.
    #[arg(long)]
    topology: PathBuf,
    /// Output DOT path. If omitted, writes to stdout.
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let file = File::open(&args.topology)
        .with_context(|| format!("failed to open topology {}", args.topology.display()))?;
    let topology: TopologySpec = serde_json::from_reader(file)?;

    let mut writer: Box<dyn Write> = if let Some(out) = args.out {
        Box::new(File::create(out)?)
    } else {
        Box::new(io::stdout().lock())
    };
    write_dot(&mut writer, &topology)?;
    Ok(())
}

fn node_shape(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Gateway => "diamond",
        NodeKind::Client => "oval",
        NodeKind::Service(_) => "box",
        NodeKind::Database(_) => "cylinder",
        NodeKind::Cache(_) => "component",
        NodeKind::Broker(_) => "folder",
        NodeKind::ExternalApi(_) => "hexagon",
    }
}

fn node_color(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Gateway => "#333333",
        NodeKind::Client => "#555555",
        NodeKind::Service(_) => "#1f4b99",
        NodeKind::Database(_) => "#8a2c2c",
        NodeKind::Cache(_) => "#2a7b45",
        NodeKind::Broker(_) => "#7a4c16",
        NodeKind::ExternalApi(_) => "#6b3fa0",
    }
}

fn edge_label(edge: &Edge) -> String {
    format!("lat={:.2}ms cap={:.0} cost={:.2}", edge.latency_ms, edge.capacity_rps, edge.cost)
}

fn write_dot(mut w: impl Write, topo: &TopologySpec) -> anyhow::Result<()> {
    writeln!(w, "digraph topology {{")?;
    writeln!(w, "  rankdir=LR;")?;
    writeln!(w, "  graph [label=\"{}\" labelloc=t fontsize=20];", escape(&topo.name))?;
    writeln!(w, "  node [fontname=\"Helvetica\" fontsize=10];")?;
    writeln!(w, "  edge [fontname=\"Helvetica\" fontsize=9];")?;

    for node in &topo.nodes {
        let shape = node_shape(&node.kind);
        let color = node_color(&node.kind);
        let zone = node.zone.as_ref().map(|z| z.as_str()).unwrap_or("-");
        let host = node.host.as_ref().map(|h| h.as_str()).unwrap_or("-");
        let label = format!("{}\\nzone={}\\nhost={}", node.id, zone, host);
        writeln!(
            w,
            "  \"{}\" [shape={} color=\"{}\" fontcolor=\"{}\" label=\"{}\"];",
            escape(node.id.as_str()),
            shape,
            color,
            color,
            escape(&label)
        )?;
    }

    for edge in &topo.edges {
        let label = edge_label(edge);
        writeln!(
            w,
            "  \"{}\" -> \"{}\" [label=\"{}\"];",
            escape(edge.from.as_str()),
            escape(edge.to.as_str()),
            escape(&label)
        )?;
    }

    writeln!(w, "}}")?;
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

