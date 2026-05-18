//! Deterministic topology generators used by the first MVP.
//!
//! Generated topologies intentionally include zones, hosts, shared resources and
//! explicit dependency bindings. This makes it possible to study cases where
//! replicas of the same logical service have different concrete paths to the
//! same database/cache/broker or share a saturated host.

use micro_net_core::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

/// Configuration for generated typed microservice topologies.
#[derive(Debug, Clone)]
pub struct GeneratedTopologyConfig {
    /// Topology shape.
    pub kind: TopologyKind,
    /// Number of logical services. Names are selected from common examples first.
    pub logical_services: usize,
    /// Number of replicas for each logical service.
    pub replicas_per_service: usize,
    /// Random seed used by random sparse topology and jittered latencies.
    pub seed: u64,
}

impl Default for GeneratedTopologyConfig {
    fn default() -> Self {
        Self {
            kind: TopologyKind::Star,
            logical_services: 3,
            replicas_per_service: 3,
            seed: 42,
        }
    }
}

/// Topology generator extension point.
pub trait TopologyGenerator {
    /// Stable generator name.
    fn name(&self) -> &'static str;

    /// Generates a serializable topology specification.
    fn generate(&self, config: &GeneratedTopologyConfig) -> TopologySpec;
}

/// Default deterministic generator for the first workspace slice.
pub struct BasicTopologyGenerator;

impl BasicTopologyGenerator {
    fn service_names(n: usize) -> Vec<String> {
        let base = [
            "payments",
            "orders",
            "analytics",
            "search",
            "profile",
            "billing",
            "notifications",
            "catalog",
        ];
        (0..n)
            .map(|i| {
                base.get(i)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("service-{i}"))
            })
            .collect()
    }
}

impl TopologyGenerator for BasicTopologyGenerator {
    fn name(&self) -> &'static str {
        "basic"
    }

    fn generate(&self, config: &GeneratedTopologyConfig) -> TopologySpec {
        let mut rng = ChaCha8Rng::seed_from_u64(config.seed);
        let services = Self::service_names(config.logical_services.max(1));
        let zones = [
            ZoneId::new("zone-a"),
            ZoneId::new("zone-b"),
            ZoneId::new("zone-c"),
        ];
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut logical_services = Vec::new();
        let mut bindings = Vec::new();

        let gateway = NodeId::new("gateway-1");
        nodes.push(Node {
            id: gateway.clone(),
            label: "gateway-1".into(),
            zone: Some(zones[0].clone()),
            host: Some(HostId::new("host-gateway")),
            kind: NodeKind::Gateway,
            metadata: BTreeMap::new(),
        });

        let db_main = NodeId::new("db-main");
        let cache_a = NodeId::new("cache-a");
        let cache_b = NodeId::new("cache-b");
        let cache_c = NodeId::new("cache-c");
        let broker = NodeId::new("broker-main");

        nodes.push(Node {
            id: db_main.clone(),
            label: "db-main".into(),
            zone: Some(ZoneId::new("zone-a")),
            host: Some(HostId::new("host-db")),
            kind: NodeKind::Database(DatabaseSpec {
                logical_resource: LogicalResourceId::new("primary-db"),
                engine: DatabaseEngine::Postgres,
                max_connections: 500,
                base_query_latency_ms: 8.0,
            }),
            metadata: BTreeMap::new(),
        });
        for (id, zone, host, hit) in [
            (cache_a.clone(), "zone-a", "host-cache-a", 0.94),
            (cache_b.clone(), "zone-b", "host-cache-b", 0.90),
            (cache_c.clone(), "zone-c", "host-cache-c", 0.86),
        ] {
            nodes.push(Node {
                id,
                label: zone.replace("zone", "cache"),
                zone: Some(ZoneId::new(zone)),
                host: Some(HostId::new(host)),
                kind: NodeKind::Cache(CacheSpec {
                    logical_resource: LogicalResourceId::new("service-cache"),
                    max_memory_mb: 1024,
                    base_hit_rate: hit,
                    base_latency_ms: 1.5,
                }),
                metadata: BTreeMap::new(),
            });
        }
        nodes.push(Node {
            id: broker.clone(),
            label: "broker-main".into(),
            zone: Some(ZoneId::new("zone-b")),
            host: Some(HostId::new("host-broker")),
            kind: NodeKind::Broker(BrokerSpec {
                logical_resource: LogicalResourceId::new("events-broker"),
                partitions: 12,
                base_lag_ms: 5.0,
            }),
            metadata: BTreeMap::new(),
        });

        let mut service_nodes = Vec::new();
        for service_name in &services {
            let service_id = LogicalServiceId::new(service_name.clone());
            logical_services.push(LogicalServiceSpec {
                id: service_id.clone(),
                dependencies: vec![
                    LogicalDependency {
                        id: LogicalDependencyId::new(format!("{service_name}:cache")),
                        target: DependencyTarget::Cache(LogicalResourceId::new("service-cache")),
                        kind: DependencyKind::CacheLookup,
                        call_mode: CallMode::Synchronous,
                        probability: 0.85,
                        base_operation_latency_ms: 1.0,
                    },
                    LogicalDependency {
                        id: LogicalDependencyId::new(format!("{service_name}:db")),
                        target: DependencyTarget::Database(LogicalResourceId::new("primary-db")),
                        kind: DependencyKind::DatabaseRead,
                        call_mode: CallMode::Synchronous,
                        probability: 0.55,
                        base_operation_latency_ms: 5.0,
                    },
                    LogicalDependency {
                        id: LogicalDependencyId::new(format!("{service_name}:broker")),
                        target: DependencyTarget::Broker(LogicalResourceId::new("events-broker")),
                        kind: DependencyKind::BrokerPublish,
                        call_mode: CallMode::Asynchronous,
                        probability: 0.20,
                        base_operation_latency_ms: 2.0,
                    },
                ],
            });

            for replica in 1..=config.replicas_per_service.max(1) {
                let zone = zones[(replica - 1) % zones.len()].clone();
                // Intentionally colocate every third replica pair on the same host to model resource sharing.
                let host = if replica % 3 == 0 {
                    HostId::new(format!("shared-host-{service_name}"))
                } else {
                    HostId::new(format!("host-{service_name}-{replica}"))
                };
                let node_id = NodeId::new(format!("{service_name}-{replica}"));
                service_nodes.push(node_id.clone());
                nodes.push(Node {
                    id: node_id.clone(),
                    label: node_id.to_string(),
                    zone: Some(zone.clone()),
                    host: Some(host),
                    kind: NodeKind::Service(ServiceInstanceSpec {
                        logical_service: service_id.clone(),
                        instance_id: ServiceInstanceId::new(format!("{service_name}-{replica}")),
                        replica_id: replica,
                        base_capacity_rps: 100.0 + replica as f64 * 10.0,
                        base_processing_latency_ms: 4.0 + replica as f64,
                    }),
                    metadata: BTreeMap::new(),
                });

                let cache_target = match zone.as_str() {
                    "zone-a" => cache_a.clone(),
                    "zone-b" => cache_b.clone(),
                    _ => cache_c.clone(),
                };
                bindings.push(DependencyBinding {
                    caller: node_id.clone(),
                    dependency: LogicalDependencyId::new(format!("{service_name}:cache")),
                    targets: vec![cache_target],
                });
                bindings.push(DependencyBinding {
                    caller: node_id.clone(),
                    dependency: LogicalDependencyId::new(format!("{service_name}:db")),
                    targets: vec![db_main.clone()],
                });
                bindings.push(DependencyBinding {
                    caller: node_id.clone(),
                    dependency: LogicalDependencyId::new(format!("{service_name}:broker")),
                    targets: vec![broker.clone()],
                });
            }
        }

        let mut edge_seq = 0usize;
        let mut add_edge =
            |edges: &mut Vec<Edge>, from: &NodeId, to: &NodeId, latency: f64, capacity: f64| {
                edge_seq += 1;
                let mut edge =
                    Edge::new(format!("e-{edge_seq}"), from.clone(), to.clone(), latency);
                edge.capacity_rps = capacity;
                edge.cost = latency;
                edges.push(edge);
            };

        for service in &service_nodes {
            let jitter = rng.gen_range(0.0..2.0);
            add_edge(&mut edges, &gateway, service, 2.0 + jitter, 10_000.0);
            add_edge(&mut edges, service, &gateway, 2.0 + jitter, 10_000.0);

            let zone_factor = match nodes
                .iter()
                .find(|n| &n.id == service)
                .and_then(|n| n.zone.as_ref())
                .map(|z| z.as_str())
            {
                Some("zone-a") => 1.0,
                Some("zone-b") => 1.35,
                Some("zone-c") => 1.8,
                _ => 1.5,
            };
            add_edge(
                &mut edges,
                service,
                &db_main,
                6.0 * zone_factor + rng.gen_range(0.0..3.0),
                5_000.0,
            );
            add_edge(
                &mut edges,
                &db_main,
                service,
                6.0 * zone_factor + rng.gen_range(0.0..3.0),
                5_000.0,
            );

            for cache in [&cache_a, &cache_b, &cache_c] {
                let same_zone_bonus =
                    if resolve_zone(&nodes, service) == resolve_zone(&nodes, cache) {
                        1.0
                    } else {
                        2.4
                    };
                add_edge(
                    &mut edges,
                    service,
                    cache,
                    same_zone_bonus + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
                add_edge(
                    &mut edges,
                    cache,
                    service,
                    same_zone_bonus + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
            }
            add_edge(
                &mut edges,
                service,
                &broker,
                4.0 + rng.gen_range(0.0..4.0),
                6_000.0,
            );
            add_edge(
                &mut edges,
                &broker,
                service,
                4.0 + rng.gen_range(0.0..4.0),
                6_000.0,
            );
        }

        match config.kind {
            TopologyKind::Star => {}
            TopologyKind::Ring => {
                for pair in service_nodes.windows(2) {
                    add_edge(
                        &mut edges,
                        &pair[0],
                        &pair[1],
                        3.0 + rng.gen_range(0.0..2.0),
                        4_000.0,
                    );
                    add_edge(
                        &mut edges,
                        &pair[1],
                        &pair[0],
                        3.0 + rng.gen_range(0.0..2.0),
                        4_000.0,
                    );
                }
                if service_nodes.len() > 2 {
                    let first = service_nodes.first().unwrap();
                    let last = service_nodes.last().unwrap();
                    add_edge(
                        &mut edges,
                        last,
                        first,
                        3.0 + rng.gen_range(0.0..2.0),
                        4_000.0,
                    );
                    add_edge(
                        &mut edges,
                        first,
                        last,
                        3.0 + rng.gen_range(0.0..2.0),
                        4_000.0,
                    );
                }
            }
            TopologyKind::FullMesh => {
                for from in &service_nodes {
                    for to in &service_nodes {
                        if from != to {
                            add_edge(&mut edges, from, to, 4.0 + rng.gen_range(0.0..3.0), 3_000.0);
                        }
                    }
                }
            }
            TopologyKind::RandomSparse => {
                for from in &service_nodes {
                    for to in &service_nodes {
                        if from != to && rng.gen_bool(0.22) {
                            add_edge(&mut edges, from, to, 4.0 + rng.gen_range(0.0..8.0), 2_500.0);
                        }
                    }
                }
            }
        }

        TopologySpec {
            schema_version: "0.1".into(),
            name: format!(
                "{:?}-services-{}-replicas-{}",
                config.kind, config.logical_services, config.replicas_per_service
            )
            .to_lowercase(),
            logical_services,
            nodes,
            edges,
            dependency_bindings: bindings,
        }
    }
}

fn resolve_zone(nodes: &[Node], id: &NodeId) -> Option<ZoneId> {
    nodes
        .iter()
        .find(|n| &n.id == id)
        .and_then(|n| n.zone.clone())
}
