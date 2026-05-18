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

        let routers = [
            NodeId::new("router-zone-a"),
            NodeId::new("router-zone-b"),
            NodeId::new("router-zone-c"),
        ];
        for (idx, router) in routers.iter().enumerate() {
            nodes.push(Node {
                id: router.clone(),
                label: router.to_string(),
                zone: Some(zones[idx].clone()),
                host: Some(HostId::new(format!("host-router-{idx}"))),
                kind: NodeKind::Gateway,
                metadata: BTreeMap::from([("role".into(), "router".into())]),
            });
        }

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
            let next_service = services
                .iter()
                .position(|name| name == service_name)
                .and_then(|idx| services.get(idx + 1))
                .map(|name| LogicalServiceId::new(name.clone()));
            let mut dependencies = Vec::new();
            if let Some(target) = next_service.clone() {
                dependencies.push(LogicalDependency {
                    id: LogicalDependencyId::new(format!("{service_name}:next-service")),
                    target: DependencyTarget::LogicalService(target),
                    kind: DependencyKind::ServiceCall,
                    call_mode: CallMode::Synchronous,
                    probability: 0.40,
                    base_operation_latency_ms: 2.0,
                });
            }
            dependencies.push(LogicalDependency {
                id: LogicalDependencyId::new(format!("{service_name}:cache")),
                target: DependencyTarget::Cache(LogicalResourceId::new("service-cache")),
                kind: DependencyKind::CacheLookup,
                call_mode: CallMode::Synchronous,
                probability: 0.85,
                base_operation_latency_ms: 1.0,
            });
            dependencies.push(LogicalDependency {
                id: LogicalDependencyId::new(format!("{service_name}:db")),
                target: DependencyTarget::Database(LogicalResourceId::new("primary-db")),
                kind: DependencyKind::DatabaseRead,
                call_mode: CallMode::Synchronous,
                probability: 0.55,
                base_operation_latency_ms: 5.0,
            });
            dependencies.push(LogicalDependency {
                id: LogicalDependencyId::new(format!("{service_name}:broker")),
                target: DependencyTarget::Broker(LogicalResourceId::new("events-broker")),
                kind: DependencyKind::BrokerPublish,
                call_mode: CallMode::Asynchronous,
                probability: 0.20,
                base_operation_latency_ms: 2.0,
            });
            logical_services.push(LogicalServiceSpec {
                id: service_id.clone(),
                dependencies,
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

                let router = match zone.as_str() {
                    "zone-a" => routers[0].clone(),
                    "zone-b" => routers[1].clone(),
                    _ => routers[2].clone(),
                };
                bindings.push(DependencyBinding {
                    caller: node_id.clone(),
                    dependency: LogicalDependencyId::new(format!("{service_name}:next-service")),
                    targets: next_service
                        .as_ref()
                        .map(|target| {
                            topology_service_candidates(
                                &services,
                                target,
                                config.replicas_per_service.max(1),
                            )
                        })
                        .unwrap_or_default(),
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

                add_edge(
                    &mut edges,
                    &node_id,
                    &router,
                    1.5 + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
                add_edge(
                    &mut edges,
                    &router,
                    &node_id,
                    1.5 + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
            }
        }

        let (gateway_anchor, router_anchor) = match config.kind {
            TopologyKind::Star => (routers[0].clone(), routers[0].clone()),
            TopologyKind::Ring => (routers[0].clone(), routers[1].clone()),
            TopologyKind::FullMesh => (routers[0].clone(), routers[2].clone()),
            TopologyKind::RandomSparse => (routers[0].clone(), routers[1].clone()),
        };
        add_edge(
            &mut edges,
            &gateway,
            &gateway_anchor,
            1.0 + rng.gen_range(0.0..1.0),
            10_000.0,
        );
        add_edge(
            &mut edges,
            &gateway_anchor,
            &gateway,
            1.0 + rng.gen_range(0.0..1.0),
            10_000.0,
        );

        connect_router_shape(&mut edges, &routers, config.kind, &mut rng, &router_anchor);
        connect_resources(
            &mut edges,
            &routers,
            &router_anchor,
            &db_main,
            [&cache_a, &cache_b, &cache_c],
            &broker,
            config.kind,
            &mut rng,
        );

        for service in &service_nodes {
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
            let router = match resolve_zone(&nodes, service).as_ref().map(|z| z.as_str()) {
                Some("zone-a") => &routers[0],
                Some("zone-b") => &routers[1],
                _ => &routers[2],
            };
            add_edge(
                &mut edges,
                service,
                router,
                1.2 + rng.gen_range(0.0..0.8),
                8_000.0,
            );
            add_edge(
                &mut edges,
                router,
                service,
                1.2 + rng.gen_range(0.0..0.8),
                8_000.0,
            );

            let db_route_penalty = match config.kind {
                TopologyKind::Star => 4.5,
                TopologyKind::Ring => 6.5,
                TopologyKind::FullMesh => 3.0,
                TopologyKind::RandomSparse => 7.0,
            };
            let cache_route_penalty = match config.kind {
                TopologyKind::Star => 2.2,
                TopologyKind::Ring => 3.5,
                TopologyKind::FullMesh => 1.6,
                TopologyKind::RandomSparse => 4.0,
            };
            let broker_route_penalty = match config.kind {
                TopologyKind::Star => 3.5,
                TopologyKind::Ring => 5.5,
                TopologyKind::FullMesh => 2.5,
                TopologyKind::RandomSparse => 6.0,
            };
            add_edge(
                &mut edges,
                router,
                &db_main,
                db_route_penalty * zone_factor + rng.gen_range(0.0..2.0),
                5_000.0,
            );
            add_edge(
                &mut edges,
                &db_main,
                router,
                db_route_penalty * zone_factor + rng.gen_range(0.0..2.0),
                5_000.0,
            );

            for cache in [&cache_a, &cache_b, &cache_c] {
                let same_zone_bonus =
                    if resolve_zone(&nodes, router) == resolve_zone(&nodes, cache) {
                        0.9
                    } else {
                        cache_route_penalty
                    };
                add_edge(
                    &mut edges,
                    router,
                    cache,
                    same_zone_bonus + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
                add_edge(
                    &mut edges,
                    cache,
                    router,
                    same_zone_bonus + rng.gen_range(0.0..1.0),
                    8_000.0,
                );
            }
            add_edge(
                &mut edges,
                router,
                &broker,
                broker_route_penalty + rng.gen_range(0.0..3.0),
                6_000.0,
            );
            add_edge(
                &mut edges,
                &broker,
                router,
                broker_route_penalty + rng.gen_range(0.0..3.0),
                6_000.0,
            );
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

fn topology_service_candidates(
    services: &[String],
    logical_service: &LogicalServiceId,
    replicas_per_service: usize,
) -> Vec<NodeId> {
    services
        .iter()
        .find(|name| LogicalServiceId::new((*name).clone()) == *logical_service)
        .map(|name| {
            (1..=replicas_per_service.max(1))
                .map(|replica| NodeId::new(format!("{name}-{replica}")))
                .collect()
        })
        .unwrap_or_default()
}

fn connect_router_shape(
    edges: &mut Vec<Edge>,
    routers: &[NodeId; 3],
    kind: TopologyKind,
    rng: &mut ChaCha8Rng,
    router_anchor: &NodeId,
) {
    match kind {
        TopologyKind::Star => {
            for router in routers {
                if router != router_anchor {
                    push_weighted_edge(edges, router_anchor, router, 1.5, 8_000.0, rng);
                    push_weighted_edge(edges, router, router_anchor, 1.5, 8_000.0, rng);
                }
            }
        }
        TopologyKind::Ring => {
            for pair in routers.windows(2) {
                push_weighted_edge(edges, &pair[0], &pair[1], 2.2, 4_000.0, rng);
                push_weighted_edge(edges, &pair[1], &pair[0], 2.2, 4_000.0, rng);
            }
            push_weighted_edge(edges, &routers[2], &routers[0], 2.2, 4_000.0, rng);
            push_weighted_edge(edges, &routers[0], &routers[2], 2.2, 4_000.0, rng);
        }
        TopologyKind::FullMesh => {
            for from in routers {
                for to in routers {
                    if from != to {
                        push_weighted_edge(edges, from, to, 1.4, 6_000.0, rng);
                    }
                }
            }
        }
        TopologyKind::RandomSparse => {
            for from in routers {
                for to in routers {
                    if from != to && rng.gen_bool(0.55) {
                        push_weighted_edge(edges, from, to, 2.5, 3_500.0, rng);
                    }
                }
            }
            push_weighted_edge(edges, &routers[0], &routers[1], 2.1, 3_500.0, rng);
            push_weighted_edge(edges, &routers[1], &routers[2], 2.1, 3_500.0, rng);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn connect_resources(
    edges: &mut Vec<Edge>,
    routers: &[NodeId; 3],
    router_anchor: &NodeId,
    db_main: &NodeId,
    caches: [&NodeId; 3],
    broker: &NodeId,
    kind: TopologyKind,
    rng: &mut ChaCha8Rng,
) {
    let resource_router = match kind {
        TopologyKind::Star => router_anchor.clone(),
        TopologyKind::Ring => routers[2].clone(),
        TopologyKind::FullMesh => routers[1].clone(),
        TopologyKind::RandomSparse => routers[0].clone(),
    };

    push_weighted_edge(edges, router_anchor, db_main, 3.5, 5_000.0, rng);
    push_weighted_edge(edges, db_main, router_anchor, 3.5, 5_000.0, rng);

    for cache in caches {
        push_weighted_edge(edges, &resource_router, cache, 1.2, 8_000.0, rng);
        push_weighted_edge(edges, cache, &resource_router, 1.2, 8_000.0, rng);
    }

    push_weighted_edge(edges, &resource_router, broker, 2.8, 6_000.0, rng);
    push_weighted_edge(edges, broker, &resource_router, 2.8, 6_000.0, rng);
}

fn push_weighted_edge(
    edges: &mut Vec<Edge>,
    from: &NodeId,
    to: &NodeId,
    base: f64,
    capacity: f64,
    rng: &mut ChaCha8Rng,
) {
    let mut edge = Edge::new(
        format!("e-{}-{}-{}", edges.len(), from, to),
        from.clone(),
        to.clone(),
        base + rng.gen_range(0.0..1.2),
    );
    edge.capacity_rps = capacity;
    edge.cost = edge.latency_ms;
    edges.push(edge);
}
