# Micro-Net-Lab: Experimental Model and Evaluation of Routing Algorithms

This document is the English counterpart of the Russian publication draft. It summarizes the simulator model, experiment protocol, routing algorithms, final analysis results, reproducibility metadata, and limitations.

## Abstract

Micro-Net-Lab is a deterministic discrete-time simulator for evaluating routing and load-balancing algorithms in typed microservice topologies with explicit downstream dependencies: databases, caches, brokers, external APIs, and service-to-service calls. The simulator models network paths, active requests, utilization-derived pressure signals, multi-hop dependency chains, incomplete observability, and controlled degradation/failure scenarios.

The main empirical result is not that a single score-based algorithm dominates everywhere. Instead, the dependency-aware score family provides a stronger multi-objective trade-off under localized degradations such as `zone-burst` and `partial-failure`, where simple local baselines cannot distinguish hidden differences in dependency paths and resource health.

## Research Questions

- RQ1. Does dependency-aware score routing improve resilience compared with `random`, `round-robin`, and `least-inflight` under asymmetric degradations?
- RQ2. Do the effects persist when the number of services and replicas increases?
- RQ3. What trade-off appears between error rate, p95 latency, and throughput, and can algorithms be ranked by a single metric?
- RQ4. Which score components matter: local concurrency, network features, downstream pressure, and host pressure?

## Model Overview

- A request arrives at a gateway and targets a logical service.
- The routing algorithm chooses one concrete service instance among the replicas of that logical service.
- Request latency consists of network latency to the selected instance, service processing, and expected downstream work.
- Each node maintains active-request state. Utilization, host pressure, and resource pressure signals are derived from this state and scenario injections.
- The routing algorithm does not observe perfect simulator internals. It sees lagged and noisy telemetry, which mitigates circular evaluation.
- Experiments use warmup, measurement, and drain phases. Results are aggregated across 50 random seeds.

## Notation

- Discrete time: \(t \in \{0,1,2,\dots\}\).
- Network graph: \(G=(V,E)\).
- Node types: gateway, service instance, database, cache, broker, external API, client.
- Each directed edge \(e=(u\to v)\) has latency \(L_e\), capacity \(C_e\), cost \(K_e\), and optionally an error probability.
- Logical services: \(S\).
- Concrete instances of service \(s\): \(I(s)\subseteq V\).
- Request \(r\) targets logical service \(s(r)\) and is routed to instance \(i(r)\in I(s(r))\).

## Topology Model

For any ordered node pair \((a,b)\), the simulator uses the latency-minimizing path:

$$
P^*(a,b)=\arg\min_{P:a\leadsto b}\sum_{e\in P} L_e.
$$

The corresponding totals are:

$$
L(a,b)=\sum_{e\in P^*(a,b)}L_e,\qquad
K(a,b)=\sum_{e\in P^*(a,b)}K_e.
$$

Path totals are precomputed by deterministic Dijkstra runs. This is a performance optimization and does not change the model semantics.

## Experiment Timeline

Each experiment has:

- total duration \(T\)
- warmup window \(T_w\)
- drain window \(T_d\)
- measurement window \(T_m=\max(0,T-T_w-T_d)\)
- offered load \(\lambda\), measured as requests per discrete time step
- a deterministic target-service rotation
- a random seed \(\sigma\)

For \(t<T-T_d\), the generator creates \(\lambda\) requests per step. For \(t\ge T-T_d\), new requests stop and the system drains active requests. Summary counters are collected only inside the measurement window.

## Request Lifecycle

At request creation time \(t_0\):

1. Candidate instances \(C=I(s(r))\) are formed.
2. A routing algorithm selects \(i(r)\in C\).
3. The request accumulates network, service, and dependency latency.
4. Completion time is:

$$
t_{\mathrm{complete}}=t_0+\left\lceil\frac{\mathrm{latency}_{\mathrm{ms}}(r)}{10}\right\rceil.
$$

The 10 ms discretization is chosen for deterministic and fast publication-scale experiments. It should be treated as a model parameter, not as a universal constant.

## Latency Model

Total request latency is:

$$
\mathrm{latency}_{\mathrm{ms}}(r)=L(g,i(r))+\ell_{\mathrm{svc}}(i(r))+\ell_{\mathrm{down}}(r,i(r)).
$$

Service processing latency is:

$$
\ell_{\mathrm{svc}}(i)=b_i.
$$

For each dependency \(d\), the model defines:

- base operation latency \(b_d\)
- usage probability \(p_d\)
- call mode \(m_d\in\{\mathrm{sync},\mathrm{async},\mathrm{fire}\}\)

with call-mode factor:

$$
\alpha(m_d)=
\begin{cases}
1.0 & \mathrm{sync}\\
0.25 & \mathrm{async}\\
0.10 & \mathrm{fire}
\end{cases}
$$

For concrete target \(x\), the dependency contribution is:

$$
\ell(i,d,x)=p_d\alpha(m_d)(b_d+L(i,x)+\beta(x,t)).
$$

The downstream contribution is:

$$
\ell_{\mathrm{down}}(r,i)=\sum_{d\in D(s(i))}\sum_{x\in T(i,d)}\ell(i,d,x)+\ell_{\mathrm{svcchain}}(r,i).
$$

Service-to-service recursion is bounded by \(D_{\max}=3\), and logical service cycles are blocked with a visited set.

## Runtime Pressure and Failures

Each node \(v\) maintains active requests \(q(v,t)\). Utilization is:

$$
u(v,t)=\frac{q(v,t)}{\mathrm{cap}(v)}.
$$

Host pressure is:

$$
\mathrm{host\_pressure}(v,t)=
\frac{\sum_{x:h(x)=h(v)}q(x,t)}{\mathrm{cap}(v)}.
$$

The simulator also tracks scalar resource pressures:

- database pressure
- cache miss risk
- broker lag
- error-rate estimate

These evolve as functions of utilization plus a deterministic wave:

$$
w(t)=\left|\sin\left(2\pi\frac{t\bmod 100}{100}\right)\right|.
$$

Dependency pressure penalty is:

$$
\beta(x,t)=10\left(u(x,t)+p_{\mathrm{db}}(x,t)+p_{\mathrm{cache}}(x,t)+p_{\mathrm{broker}}(x,t)\right).
$$

The synthetic request failure probability is:

$$
\Pr[\mathrm{fail}]=
\mathrm{clamp}\left(p_{\mathrm{err}}(i,t)+0.02\max(0,u(i,t)),0,0.95\right).
$$

## Observability Model

Routing algorithms observe lagged and noisy telemetry:

$$
\tilde{x}(t)=x(t-\Delta)+\eta,\qquad \eta\sim \mathrm{Uniform}(-\epsilon,\epsilon).
$$

The simulator uses true internal state to compute latency and failures, but algorithms see only the observed state. This avoids giving the score family direct access to the same ground-truth values used by the simulator outcome model.

## Routing Algorithms

All algorithms select one candidate \(i(r)\in I(s(r))\).

### Random

$$
i(r)\sim \mathrm{Uniform}(C).
$$

### Round-robin

$$
i(r)=C[k_{s(r)}\bmod |C|],\qquad k_{s(r)}\leftarrow k_{s(r)}+1.
$$

### Least-inflight

$$
i(r)=\arg\min_{c\in C}\tilde{q}(c,t).
$$

### Score family

Score algorithms compute:

$$
\mathrm{score}(c)=\sum_{j=1}^{J}w_j f_j(c),\qquad
i(r)=\arg\min_{c\in C}\mathrm{score}(c).
$$

The default `score-v1` weights are:

$$
\mathrm{score\text{-}v1}
=0.30f_{\mathrm{lat}}
+0.25f_{\mathrm{inflight}}
+0.15f_{\mathrm{err}}
+0.10f_{\mathrm{netcost}}
+0.15f_{\mathrm{down}}
+0.05f_{\mathrm{host}}.
$$

Weights were chosen a priori as a fixed research hypothesis and were not optimized on the final results. They are interpreted as one dependency-aware design point, not as a discovered optimum.

| Variant | \(f_{\mathrm{lat}}\) | \(f_{\mathrm{inflight}}\) | \(f_{\mathrm{err}}\) | \(f_{\mathrm{netcost}}\) | \(f_{\mathrm{down}}\) | \(f_{\mathrm{host}}\) |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `score` / `score-v1` | 0.30 | 0.25 | 0.15 | 0.10 | 0.15 | 0.05 |
| `score-local-only` | 0.00 | 0.55 | 0.35 | 0.00 | 0.00 | 0.10 |
| `score-local+network` | 0.30 | 0.35 | 0.20 | 0.10 | 0.00 | 0.05 |
| `score-local+downstream` | 0.20 | 0.30 | 0.15 | 0.00 | 0.30 | 0.05 |
| `score-no-downstream` | 0.35 | 0.30 | 0.20 | 0.10 | 0.00 | 0.05 |
| `score-no-host-pressure` | 0.30 | 0.25 | 0.15 | 0.10 | 0.20 | 0.00 |

## Metrics and Aggregation

Per run, the simulator records:

- created, completed, failed
- success rate and error rate
- average, p95, and p99 latency over completed requests
- active requests at the end
- throughput per measurement step
- utilization and pressure summaries
- replica imbalance
- drain metrics
- failure burstiness

For each experimental context, results are aggregated across seeds using mean, sample standard deviation, and 95% confidence interval:

$$
\mathrm{CI95}=1.96\frac{s}{\sqrt{n}}.
$$

The multi-objective evaluation minimizes error rate, minimizes p95 latency, and maximizes throughput ratio:

$$
\min \mathrm{error\_rate},\qquad
\min \mathrm{p95\_latency},\qquad
\max \mathrm{throughput\_ratio}.
$$

An algorithm is Pareto-optimal in a context if no other algorithm is no worse on all three objectives and strictly better on at least one objective.

## Experimental Design

### Full-factorial small-topology experiment

This experiment covers all algorithms, scenarios, loads, and topologies on a controlled small topology.

```text
topologies = star, ring, full-mesh, random-sparse
logical_services = 3
replicas_per_service = 3
algorithms = random, round-robin, least-inflight,
             score, score-local-only, score-local+network,
             score-local+downstream, score-no-downstream,
             score-no-host-pressure
scenarios = healthy, db-overloaded, cache-degraded,
            broker-lag, zone-burst, partial-failure
load_levels = 1, 5, 10, 25, 50, 100
seeds = 50
```

Total: \(4\times 9\times 6\times 6\times 50 = 64800\) experiments.

Important caveat: in this dataset, score variants are aggregated as `score-v1`, so it supports score-family comparisons, not claims about individual score weight variants.

### Scaling experiment

This experiment tests whether the main effects persist as the topology grows.

```text
topologies = star, ring, full-mesh, random-sparse
logical_services = 3, 10, 20
replicas_per_service = 3, 5
algorithms = random, round-robin, least-inflight,
             score-local+network, score-no-host-pressure
scenarios = healthy, zone-burst, partial-failure
load_levels = 10, 50, 100
seeds = 50
```

Total: \(4\times 3\times 2\times 5\times 3\times 3\times 50 = 54000\) experiments.

## Final Results

### Macro summary

| Dataset | Algorithm | Contexts | Error rate | P95 latency, ms | Throughput ratio |
| --- | --- | ---: | ---: | ---: | ---: |
| Small topology | `score-v1` family | 144 | 0.02699 | 44.92 | 0.9730 |
| Small topology | `round-robin` | 144 | 0.08314 | 46.18 | 0.9169 |
| Small topology | `random` | 144 | 0.08832 | 46.15 | 0.9117 |
| Small topology | `least-inflight` | 144 | 0.10810 | 44.35 | 0.8919 |
| Scaling | `score-local+network` | 216 | 0.02205 | 50.20 | 0.9779 |
| Scaling | `score-no-host-pressure` | 216 | 0.02387 | 49.37 | 0.9761 |
| Scaling | `round-robin` | 216 | 0.13650 | 51.59 | 0.8635 |
| Scaling | `random` | 216 | 0.15460 | 51.52 | 0.8454 |
| Scaling | `least-inflight` | 216 | 0.19260 | 50.31 | 0.8074 |

### CI95 appendix for macro summaries

| Dataset | Algorithm | Error CI95 | P95 CI95 | Throughput-ratio CI95 |
| --- | --- | ---: | ---: | ---: |
| Small topology | `score-v1` family | 0.005271 | 0.5911 | 0.005271 |
| Small topology | `round-robin` | 0.01674 | 0.4951 | 0.01674 |
| Small topology | `random` | 0.01786 | 0.4989 | 0.01786 |
| Small topology | `least-inflight` | 0.02515 | 0.4581 | 0.02515 |
| Scaling | `score-local+network` | 0.003493 | 0.7505 | 0.003493 |
| Scaling | `score-no-host-pressure` | 0.003627 | 0.7641 | 0.003627 |
| Scaling | `round-robin` | 0.01359 | 0.7890 | 0.01359 |
| Scaling | `random` | 0.01536 | 0.7905 | 0.01536 |
| Scaling | `least-inflight` | 0.02328 | 0.7922 | 0.02328 |

### Localized degradation scenarios

Best score-family variant versus `random`:

| Dataset | Scenario | Best score variant | Contexts | Error delta | Error delta, % | P95 delta, % | Throughput-ratio delta |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: |
| Small topology | `partial-failure` | `score-v1` family | 24 | -0.2195 | -85.33 | -2.908 | +0.2194 |
| Small topology | `zone-burst` | `score-v1` family | 24 | -0.1838 | -81.73 | -1.937 | +0.1838 |
| Scaling | `partial-failure` | `score-local+network` | 72 | -0.1846 | -87.84 | -3.227 | +0.1846 |
| Scaling | `zone-burst` | `score-local+network` | 72 | -0.2203 | -87.13 | -1.733 | +0.2203 |

`score-no-host-pressure` is close in error-rate reduction and often stronger on p95 latency: -4.239% in `partial-failure` and -3.445% in `zone-burst`. In contrast, `least-inflight` can reduce p95 while worsening error rate and throughput, especially in `zone-burst`.

### Pareto-front counts

| Dataset | Algorithm | Contexts | Pareto contexts | Share |
| --- | --- | ---: | ---: | ---: |
| Small topology | `score-v1` family | 144 | 144 | 1.0000 |
| Small topology | `least-inflight` | 144 | 103 | 0.7153 |
| Small topology | `random` | 144 | 80 | 0.5556 |
| Small topology | `round-robin` | 144 | 46 | 0.3194 |
| Scaling | `score-no-host-pressure` | 216 | 213 | 0.9861 |
| Scaling | `score-local+network` | 216 | 180 | 0.8333 |
| Scaling | `least-inflight` | 216 | 82 | 0.3796 |
| Scaling | `random` | 216 | 60 | 0.2778 |
| Scaling | `round-robin` | 216 | 58 | 0.2685 |

## Interpretation

The strongest claim is not universal dominance of score routing. The defensible claim is narrower and stronger:

> Dependency-aware score features improve the Pareto trade-off under localized degradation, where candidate replicas differ in dependency paths, resource pressure, and zone conditions.

`least-inflight` is an important negative result. It often has attractive p95 latency, but it can achieve this while losing more requests and reducing throughput. Therefore latency-only ranking is misleading.

## Reproducibility

Repository: <https://github.com/rusanoph/Micro-Net-Lab>

Publication revision with the final analysis script and documents:
<https://github.com/rusanoph/Micro-Net-Lab/commit/ea288fd>

Each benchmark run writes metadata including:

- git commit, when available
- full command line
- Rust compiler version
- `uname` and `lscpu`
- provider and VM type, if supplied
- parallelism
- shard index/count
- config fingerprint

This is why the exact dataset revision can be tied to a concrete repository commit and hardware/software environment.

## Threats to Validity

- This is a controlled simulation study, not production validation.
- Absolute latency values should not be interpreted as predictions for a specific production system.
- Pressure signals are synthetic low-dimensional proxies.
- The main workloads use constant per-step arrival rates; bursty and diurnal workloads remain future work.
- The network is modeled at service-graph level, not as a packet-level TCP simulation.
- Topologies are synthetic rather than extracted from production traces.
- The current comparison does not include all production-grade routing baselines such as power-of-two choices, EWMA latency, locality-aware routing, and outlier detection.
- Sensitivity analysis for pressure coefficient, wave period, observability lag, and noise remains future work.

## Conclusion

Micro-Net-Lab provides a reproducible simulation framework for studying routing algorithms in microservice topologies with explicit dependency bindings and incomplete observability. The final experiments show that dependency-aware score routing is not universally best on every metric, but it provides a stronger resilience/throughput/latency trade-off under localized degradation scenarios. The central methodological point is that routing evaluation must be multi-objective: p95 latency, error rate, and throughput must be interpreted together.
