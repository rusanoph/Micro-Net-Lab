# Micro-Net-Lab: Experimental Model and Methodology

This Markdown contains LaTeX math intended for renderers that support MathJax/KaTeX.

This document specifies the mathematical model implemented by this repository and the experimental protocol used to evaluate routing/load-balancing policies in typed microservice topologies.

The text is written to be directly reusable as the "Methods" and "Experimental Setup" sections of a scientific paper.

## Abstract

We present a deterministic, tick-based simulator for evaluating microservice routing and load-balancing policies in typed service topologies with explicit downstream dependency bindings. The simulator models network paths, per-node capacity/pressure, multi-hop dependency chains, and controlled degradation/failure scenarios. Policies are compared under a reproducible protocol with warmup/measurement/drain phases and statistical aggregation across seeds.

## Why This Matters

Routing decisions in microservice systems affect not only the chosen backend instance but also the concrete downstream resources and network paths that instance will use. Traditional baselines (random, round-robin, least-inflight) are often local-state driven and may fail under asymmetric downstream degradation. A dependency-aware policy can, in principle, avoid selecting instances whose downstream path/resources are currently degraded, reducing tail latency and improving stability under stress. This repository provides a research framework to test such hypotheses under controlled, reproducible conditions.

## Plain-Language Overview (Without Formulas)

- A request arrives at a gateway, targets a logical service, and the policy chooses one concrete service instance among replicas.
- Request latency is modeled as: network to the chosen instance + instance processing + expected downstream work (DB/cache/broker calls and possibly service-to-service calls).
- Each node maintains inflight concurrency; utilization/pressure signals are derived from inflight and scenario injections (overload, degradation, partial failure, zonal burst).
- Policies observe delayed/noisy telemetry (to avoid circular evaluation) and make routing decisions using either simple rules (random/round-robin/least-inflight) or a weighted score over multiple features.
- Experiments run in discrete ticks with warmup and drain; results are aggregated across many seeds and summarized as mean/p95/p99/throughput/error rate plus effect sizes vs baselines.

## Where The Formulas Come From (Conceptual Lineage)

The model combines standard, well-motivated building blocks rather than attempting to reproduce a specific vendor system:
- Graph model + shortest paths: network delay is modeled as a weighted directed graph and a latency-minimizing path, which is the common abstraction used when reasoning at the service level (as opposed to packet-level simulation).
- Latency decomposition: end-to-end latency is written as network + service processing + downstream work, mirroring typical service tracing decompositions.
- Concurrency as a proxy for queuing: inflight and utilization approximate saturation and queueing delay; this follows classic queueing intuition (higher concurrency relative to capacity increases waiting and tail latency).
- Low-dimensional pressure signals: downstream health is represented by bounded scalar signals analogous to production telemetry (queue depth/lag/error rate/hit rate). These are proxies; they are designed for controlled comparative evaluation, not absolute latency prediction.
- Anti-circular observability: lag/noise on signals follows standard experimental practice to prevent policies from “seeing the simulator’s answer” directly.

## Results Overview (Paper Section Template)

This section is a write-up template intended to be filled once the current publication-scale run finishes. It assumes the run outputs `stats.csv`, `effect_sizes.csv`, and a set of plots generated from aggregated artifacts.

### Key Findings (1 paragraph)

- Main effect on latency (mean/p95/p99) vs the baseline.
- Where the gains concentrate (which scenarios and load regimes).
- Any tradeoffs on throughput and error rate.

### Tables

- Table A: grouped metrics by policy × scenario (fixed load).
- Table B: grouped metrics by policy × load (fixed scenario).
- Table C: effect sizes (e.g., Cohen’s \(d\)) and relative deltas for primary metrics.

### Plots (Minimal Set)

- Plot 1: mean latency vs load level per policy.
- Plot 2: p95/p99 latency vs load level per policy.
- Plot 3: throughput vs load level.
- Plot 4: error rate vs load level (failure scenarios).
- Plot 5: utilization/pressure time series (to illustrate regime changes and the wave).

### Limitations / Threats to Validity

- The pressure signals are synthetic low-dimensional proxies; they aim at comparative policy evaluation rather than absolute latency prediction.
- Conclusions are sensitive to observability (lag/noise) and workload distributions; sensitivity analysis is required.
- Topology and dependency generators define the studied class of systems; generalization beyond it should be argued carefully.

## 1. Notation

- Discrete time is indexed by ticks \(t \in \{0,1,2,\dots\}\).
- The directed network is a graph \(G=(V,E)\) with node set \(V\) and directed edge set \(E\).
- Each node \(v \in V\) has a type: gateway, service instance, database, cache, broker, external API, client.
- Each directed edge \(e=(u\to v)\in E\) has parameters:
  - link latency \(L_e > 0\) in milliseconds
  - link capacity \(C_e > 0\) in requests per second (RPS)
  - abstract cost \(K_e \ge 0\) (typically \(K_e=L_e\))
  - optional error rate (currently not used in path selection)

- Each logical service \(s \in S\) has a finite set of concrete service instances \(I(s) \subset V\).
- Each request \(r\) targets one logical service \(s(r) \in S\) and is routed to one concrete instance \(i(r)\in I(s(r))\).

## 2. Topology Model

### 2.1 Typed Nodes and Zones/Hosts

Each node \(v\in V\) may have:
- zone label \(z(v)\) (e.g. availability zone)
- host label \(h(v)\) (to model colocation pressure)

These labels do not directly affect path selection. Their effects are introduced through:
- generated edge latencies/costs between zones
- host-level pressure metrics computed from inflight concurrency

### 2.2 Shortest Path and Path Totals

For any ordered pair \((a,b)\in V\times V\), the simulator uses the latency-optimal path:

$$
P^\*(a,b)=\arg\min_{P:a\leadsto b}\ \sum_{e\in P} L_e.
$$

The corresponding path totals are:

$$
L(a,b)=\sum_{e\in P^\*(a,b)} L_e,\qquad
K(a,b)=\sum_{e\in P^\*(a,b)} K_e.
$$

Implementation detail (performance, not semantics): \(L(a,b)\) and \(K(a,b)\) are precomputed at backend construction time by running a deterministic Dijkstra from each source node.

#### Rationale

We represent connectivity as a directed weighted graph and use shortest-path-by-latency as the baseline routing substrate. This matches common service-mesh abstractions where a request’s network delay is dominated by one “best” path under static link parameters. The simulator intentionally avoids packet-level congestion control modeling; instead, it keeps the research problem at the service-graph level, where policies compete based on relative differences in placement, paths, and dependency health.

We keep both latency \(L\) and an abstract cost \(K\) to support multi-objective routing studies. In the current configuration \(K_e\approx L_e\), but allowing \(K\neq L\) is important for ablation and for future extensions (risk penalties, policy-imposed costs, non-latency objectives).

## 3. Workload and Experiment Timeline

Each experiment is parameterized by:

- total ticks \(T\)
- warmup ticks \(T_w\)
- drain ticks \(T_d\)
- measurement ticks \(T_m = \max(0, T - T_d - T_w)\)
- requests per tick \(\lambda\)
- a fixed request source node \(g\) (usually a gateway)
- a fixed list of target logical services \(\{s_1,\dots,s_M\}\), rotated deterministically
- random seed \(\sigma\)

### 3.1 Request Generation

For ticks \(t < T - T_d\), the generator emits \(\lambda\) requests per tick. For \(t \ge T - T_d\), no new requests are created (drain phase).

### 3.2 Measurement Window

Counters and latency samples used in the final summary are collected only for completions/failures that occur during the measurement interval:

$$
t\in[T_w,\ T-T_d).
$$

Warmup counters are reset at \(t=T_w\).

#### Rationale

Warmup/measurement/drain is standard in systems evaluation: warmup reduces bias from transient initialization, and drain reduces truncation artifacts when the simulation stops. Without drain, the reported throughput and tail latencies become sensitive to the arbitrary stop time \(T\), because long-running requests remain in-flight and are excluded from the latency sample.

The constant-per-tick workload is chosen as the simplest reproducible arrival process that isolates routing decisions. More realistic arrival processes (bursty, diurnal) can be added as separate workloads, but a constant baseline is valuable for controlled comparisons.

## 4. Request Lifecycle

At creation tick \(t_0\), each request \(r\) is routed:

1. Candidate set is obtained: \(I(s(r))\).
2. A policy selects an instance \(i(r)\in I(s(r))\).
3. The request accrues latency from:
   - network path \(g \to i(r)\)
   - service processing at \(i(r)\)
   - downstream dependency calls issued by \(i(r)\) (and recursively, by downstream services)
4. The request completes at tick:

$$
t_{\mathrm{complete}}=t_0+\left\lceil\frac{\mathrm{latency}_{\mathrm{ms}}(r)}{10}\right\rceil,
$$

where the simulator uses a 10 ms-to-1 tick discretization.

The instance inflight counter is incremented at dispatch and decremented at completion time.

#### Rationale

The simulator’s primary state variable is inflight concurrency, because many practical routing/load-balancing baselines (least-inflight, queue-aware, EWMA-latency heuristics) are driven by concurrency/queueing signals rather than by instantaneous CPU utilization. In classical queueing intuition, higher concurrency correlates with increased queueing delay; modeling inflight explicitly keeps the model simple and interpretable while still enabling meaningful policy comparisons.

The 10 ms-to-1 tick discretization is an explicit approximation chosen to keep experiments fast while preserving relative latency differences. The simulator is intended for comparative routing evaluation, not for real-time emulation.

##### "Magic Number" Notes

- **Tick scale (10 ms)**: chosen to keep runtime manageable while providing enough resolution for microservice-scale latencies. A finer resolution increases compute cost and event volume; a coarser resolution can quantize away meaningful differences.
- **Completion mapping \(\lceil \mathrm{latency}_{\mathrm{ms}}/10\rceil\)**: ensures every request takes at least 1 tick and preserves ordering for close latencies.

## 5. Latency Model

Total request latency (in ms) is:

$$
\mathrm{latency}_{\mathrm{ms}}(r)=L(g,i(r))+\ell_{\mathrm{svc}}(i(r))+\ell_{\mathrm{down}}(r,i(r)).
$$

### 5.1 Service Processing Latency

Each service instance \(i\) has a configured base processing latency \(b_i\) (ms). The simulator currently uses:

$$
\ell_{\mathrm{svc}}(i)=b_i.
$$

#### Rationale

We separate service processing time from network and downstream time to support ablation and causal attribution. In real systems, processing latency depends on CPU scheduling, GC, cache locality, and co-tenancy. Here it is modeled as a per-instance baseline \(b_i\) so the routing problem remains focused on placement/path/dependency effects without requiring a full CPU micro-architecture model.

### 5.2 Downstream Dependency Latency

Each logical service defines a logical dependency profile, a set of dependencies \(D(s)\). For a concrete instance \(i\in I(s)\), each dependency \(d\in D(s)\) is resolved to one or more concrete target nodes \(T(i,d)\subseteq V\) via explicit dependency bindings.

For each dependency \(d\), we define:
- operation base latency \(b_d\) (ms)
- usage probability \(p_d \in [0,1]\)
- call mode \(m_d \in \{\text{sync},\text{async},\text{fire}\}\)

Call-mode factor:

$$
\alpha(m_d)=
\begin{cases}
1.0 & \mathrm{sync}\\
0.25 & \mathrm{async}\\
0.10 & \mathrm{fire}
\end{cases}
$$

Each concrete target node \(x \in T(i,d)\) contributes:

$$
\ell(i,d,x)=p_d\cdot\alpha(m_d)\cdot\left(b_d+L(i,x)+\beta(x,t)\right),
$$

where \(\beta(x,t)\) is a pressure penalty derived from runtime utilization and resource-specific pressures (Section 6).

The downstream latency is the sum over dependencies and resolved targets:

$$
\ell_{\mathrm{down}}(r,i)=\sum_{d\in D(s(i))}\ \sum_{x\in T(i,d)} \ell(i,d,x)\ +\ \ell_{\mathrm{svcchain}}(r,i).
$$

#### Rationale

This form is an “expected-cost” approximation widely used in synthetic microservice evaluation: each dependency contributes proportionally to its probability \(p_d\). The call-mode factor \(\alpha(m_d)\) encodes the empirical idea that asynchronous work contributes less to the end-to-end latency observed by the caller than synchronous critical-path calls, while still consuming downstream capacity and influencing future congestion.

Including the network term \(L(i,x)\) in each dependency is essential for topology sensitivity: two replicas of the same logical service can have different paths to the same logical resources, and dependency-aware routing should be able to exploit that.

### 5.3 Service-to-Service Chains (Recursive)

If a dependency targets another logical service, the simulator adds a recursive downstream term to model multi-hop request graphs. Recursion is bounded to depth \(D_{\max}=3\), and logical service cycles are prevented via a visited-set per request.

In effect, for dependencies with target type "logical service", \(\ell(i,d,x)\) includes the downstream latency of \(x\) as another service instance:

$$
\ell_{\mathrm{svcchain}}(r,i)=\sum_{(d,x):\, d\ \mathrm{targets\ service}} p_d\,\alpha(m_d)\cdot \ell_{\mathrm{down}}(r,x).
$$

#### Rationale

Single-hop request graphs (gateway \(\to\) service \(\to\) resources) are often too simple to reveal meaningful differences between routing strategies. Real requests frequently traverse service-to-service chains, where an early routing decision can amplify or mitigate downstream congestion. Bounded recursion \(D_{\max}\) is a pragmatic compromise: it captures multi-hop coupling while keeping computation bounded and preventing cycles from diverging.

##### "Magic Number" Notes

- **Maximum depth \(D_{\max}=3\)**: chosen to capture common short request chains (gateway + 2–3 internal services) while keeping computation bounded. Increasing depth increases compute cost roughly multiplicatively with branching; decreasing depth reduces multi-hop coupling effects that are central to dependency-aware routing.
- **Visited-set cycle prevention**: real dependency graphs may contain cycles. In a recursive model, cycles can cause unbounded recursion and double counting. The visited-set implements a conservative “no repeated logical service per request” rule to ensure termination and to avoid pathological amplification in synthetic graphs.

## 6. Runtime State and Pressure Model

### 6.1 Inflight and Utilization

Each node \(v\) maintains runtime state including inflight count \(q(v,t)\). A node capacity parameter \(\text{cap}(v)\) is defined by type:
- services: base capacity RPS
- databases: max connections
- caches: constant nominal capacity
- brokers: partitions-scaled nominal capacity

Utilization is:

$$
u(v,t)=\frac{q(v,t)}{\mathrm{cap}(v)}.
$$

Host pressure is computed by summing inflight counts over all nodes colocated on the same host, then normalizing:

$$
\mathrm{host\_pressure}(v,t)=\frac{\sum_{x:\ h(x)=h(v)} q(x,t)}{\mathrm{cap}(v)}.
$$

#### Rationale

Utilization is modeled as normalized inflight to provide a single saturation indicator across heterogeneous node types (service instances, databases, caches, brokers). While real deployments require richer models (CPU, memory, connection pools), inflight-normalized-by-capacity is a robust baseline for comparative routing evaluation.

Host pressure models co-tenancy interference (“noisy neighbors”): even if one replica’s own inflight is moderate, sharing a host with other busy replicas can degrade its effective performance. This creates realistic placement-dependent tradeoffs for routing.

### 6.2 Resource Pressure Signals

The simulator maintains additional scalar pressures per node (dimensionless, research-friendly):
- database pressure \(p_{\text{db}}(v,t)\)
- cache miss risk \(p_{\text{cache}}(v,t)\)
- broker lag \(p_{\text{broker}}(v,t)\)
- error-rate estimate \(p_{\text{err}}(v,t)\)

These evolve as deterministic functions of utilization plus a periodic wave:

$$
w(t)=\left|\sin\left(2\pi\cdot \frac{t\bmod 100}{100}\right)\right|.
$$

The current implementation injects scenario-specific boosts (Section 7) and clamps to bounded ranges.

#### Rationale

The pressure signals are intentionally low-dimensional, policy-observable scalars analogous to production telemetry (EWMA latency, error rate, queue depth, hit rate, lag). The periodic wave introduces controlled non-stationarity to test policies under changing conditions without relying solely on stochastic noise that can obscure causal comparisons. The additive form keeps the model interpretable: higher pressure monotonically increases expected latency.

##### Why This Wave (And Why These Numbers)

The specific choice

$$
w(t)=\left|\sin\left(2\pi\cdot \frac{t\bmod 100}{100}\right)\right|
$$

is a pragmatic research device:
- \(|\sin|\) is strictly nonnegative and bounded in \([0,1]\), so it can be interpreted as a “degree of degradation” without introducing sign conventions.
- A 100-tick period provides multiple regime shifts within a single run, but changes slowly enough for policies to react under lagged observability (Section 8). In paper runs, the period and amplitude should be treated as experimental factors and included in sensitivity/ablation analysis.

### 6.3 Downstream Pressure Penalty

Pressure penalty used inside dependency latency is:

$$
\beta(x,t)=10\cdot\left(u(x,t)+p_{\mathrm{db}}(x,t)+p_{\mathrm{cache}}(x,t)+p_{\mathrm{broker}}(x,t)\right).
$$

## 7. Failure and Degradation Scenarios

Each experiment has a scenario label controlling additional pressure injections, such as:
- `healthy`
- `db-overloaded`
- `cache-degraded`
- `broker-lag`
- `zone-burst`
- `partial-failure`

These scenarios alter the ground-truth runtime signals \(u, p_{\text{db}}, p_{\text{cache}}, p_{\text{broker}}, p_{\text{err}}\) over time.

#### Rationale

Scenario injection provides controlled “what-if” regimes (overload, degradation, partial failure, zonal incidents) that are necessary for scientific evaluation. In benign conditions many baselines behave similarly; meaningful differentiation typically appears when candidates become asymmetrically degraded and downstream dependencies are stressed, which is precisely where dependency-aware routing is expected to help.

### 7.1 Synthetic Failure Probability

Upon dispatch, a request samples a Bernoulli failure outcome based on the chosen instance runtime:

$$
\Pr[\mathrm{fail}]=\mathrm{clamp}\left(p_{\mathrm{err}}(i,t)+0.02\cdot \max(0,u(i,t)),\ 0,\ 0.95\right).
$$

If failed, the request is counted as failed at completion time; otherwise it is counted as completed.

## 8. Observability Model (Anti-Circular Evaluation)

To prevent trivial circularity (policy observing the exact ground-truth signals used by the simulator), the routing policy does not observe \(u\) and pressures instantaneously.

Instead, policies observe a lagged and noisy snapshot:

- lag \( \Delta \) ticks
- noise amplitude \( \epsilon \) (small)

At tick \(t\), the policy is given:

$$
\tilde{x}(t)=x(t-\Delta)+\eta,\qquad \eta\sim \mathrm{Uniform}(-\epsilon,\epsilon),
$$

with appropriate clamping for bounded quantities (e.g. cache miss risk in \([0,1]\), error rate in \([0,0.95]\)).

The simulator itself always uses the ground-truth \(x(t)\) to compute latency and failures.

#### Rationale

Without an observability model, a score policy can trivially exploit exact simulator internals, yielding over-optimistic gains that would not transfer to real systems. Lagged/noisy observation is a standard technique in systems/control evaluation: it forces policies to operate on imperfect telemetry and mitigates circular evaluation (policy observing the same function used to compute ground truth).

## 9. Policies

Let candidates be \(C = I(s(r))\). All policies output \(i(r)\in C\). Ties are resolved deterministically by stable candidate ordering.

### 9.1 Random

Uniform selection:

$$
i(r)\sim \mathrm{Uniform}(C),
$$

using a deterministic seedable RNG.

### 9.2 Round-Robin

Maintain a cursor \(k_s\) per logical service \(s\). Choose:

$$
i(r)=C[k_{s(r)}\bmod |C|],\qquad k_{s(r)}\leftarrow k_{s(r)}+1.
$$

### 9.3 Least-Inflight

Choose the candidate with minimal observed inflight:

$$
i(r)=\arg\min_{c\in C}\tilde{q}(c,t).
$$

### 9.4 Score Policies (Score-v1 and Ablations)

Score policies compute a scalar score for each candidate and select the minimum:

$$
\mathrm{score}(c)=\sum_{j=1}^{J} w_j\, f_j(c),\qquad
i(r)=\arg\min_{c\in C}\mathrm{score}(c).
$$

All features are defined so that lower is better. Weights \(w_j>0\) are fixed per policy variant.

#### Rationale

The score-policy family mirrors a common production design: combine multiple telemetry signals into a single scalar and choose the minimum. This makes decisions explainable (via per-feature contributions) and enables ablation studies by selectively removing terms.

We start with a linear model \(\sum w_j f_j\) because it is interpretable and easy to validate. The purpose of this repository is to evaluate *which signals matter and under which topologies/scenarios*, not to hide behavior behind a black-box predictor. More complex nonlinear or learned policies can be introduced later using the same feature interface and the same experimental methodology.

#### 9.4.1 Feature: Network Latency

$$
f_{\mathrm{lat}}(c)=\frac{L(g,c)}{100}.
$$

#### 9.4.2 Feature: Network Cost

$$
f_{\mathrm{netcost}}(c)=\frac{K(g,c)}{100}.
$$

#### 9.4.3 Feature: Inflight Pressure

Let service instance capacity be \( \text{cap}(c) \). Then:

$$
f_{\mathrm{inflight}}(c)=\frac{\tilde{q}(c,t)}{\max(1,\mathrm{cap}(c))}.
$$

#### 9.4.4 Feature: Error Rate

$$
f_{\mathrm{err}}(c)=\tilde{p}_{\mathrm{err}}(c,t).
$$

#### 9.4.5 Feature: Host Pressure

$$
f_{\mathrm{host}}(c)=\widetilde{\mathrm{host\_pressure}}(c,t).
$$

#### 9.4.6 Feature: Downstream Pressure (Dependency-Aware)

For each dependency \(d\in D(s(c))\) and each resolved concrete target \(x\in T(c,d)\), define:

$$
f_{\mathrm{down}}(c)=\frac{1}{Z}\sum_{d\in D(s(c))}\sum_{x\in T(c,d)}
p_d\cdot\left(
\frac{L(c,x)}{100}
\ +\ \tilde{u}(x,t)
\ +\ \tilde{p}_{\mathrm{db}}(x,t)
\ +\ \tilde{p}_{\mathrm{cache}}(x,t)
\ +\ \tilde{p}_{\mathrm{broker}}(x,t)
\ +\ \tilde{p}_{\mathrm{err}}(x,t)
\right),
$$

where \(Z=\sum_{d\in D(s(c))}\sum_{x\in T(c,d)} \max(p_d, 0.01)\) is a normalization constant to keep the scale stable.

#### 9.4.7 Score-v1 Weighting

Score-v1 uses:

$$
\mathrm{score\mbox{-}v1}
=0.30 f_{\mathrm{lat}}
+0.25 f_{\mathrm{inflight}}
+0.15 f_{\mathrm{err}}
+0.10 f_{\mathrm{netcost}}
+0.15 f_{\mathrm{down}}
+0.05 f_{\mathrm{host}}.
$$

#### 9.4.8 Ablation Variants

The CLI includes multiple score variants used for ablation:
- `score-local-only`: local runtime only (inflight, error, host)
- `score-local+network`: local + network features
- `score-local+downstream`: local + downstream feature
- `score-no-downstream`: score-v1 minus downstream feature
- `score-no-host-pressure`: score-v1 minus host feature

Exact weights are defined in code to keep variants stable across runs.

## 10. Data Collection and Output Artifacts

### 10.1 Trace vs Aggregate Modes

The simulator supports two observation modes:
- full trace (JSONL events per request and tick)
- no trace (for large-scale runs)

For publication-scale experiments, the recommended configuration is:
- `--trace none`
- `--artifacts aggregate`

This writes only:
- `aggregate.csv` and `aggregate.json`: one row per experiment run
- `stats.csv`: grouped mean/std/CI95 per (topology, scenario, policy, load)
- `effect_sizes.csv`: delta and Cohen's \(d\) vs a baseline policy (default `random`)

### 10.2 Summary Metrics (Per Run)

For each run we report:
- created, completed, failed
- success rate \(=\frac{\text{completed}}{\text{completed}+\text{failed}}\)
- error rate \(=\frac{\text{failed}}{\text{completed}+\text{failed}}\)
- average latency and percentiles computed over completed request latencies:

$$
\bar{\ell}=\frac{1}{N}\sum_{i=1}^{N}\ell_i,\qquad
\mathrm{p95}=\ell_{(\lfloor 0.95(N-1)\rfloor)},\qquad
\mathrm{p99}=\ell_{(\lfloor 0.99(N-1)\rfloor)}.
$$

- throughput per tick over the measurement window:

$$
\mathrm{throughput}=\frac{\mathrm{completed}}{T_m}.
$$

### 10.3 Aggregation Across Seeds

For each group (topology, scenario, policy, load), we compute:
- mean and sample stddev
- 95% confidence interval (normal approximation):

$$
\mathrm{CI95}=1.96\cdot\frac{s}{\sqrt{n}}.
$$

Effect sizes vs baseline are computed as:
- relative delta percent:

$$
\Delta\% = 100\cdot\frac{\mu_{\mathrm{policy}}-\mu_{\mathrm{base}}}{\mu_{\mathrm{base}}}.
$$

- Cohen's \(d\) with pooled variance.

#### Rationale

Using multiple seeds is necessary to quantify variability under seeded randomness (topology jitter, workload ordering, scenario waves). Reporting group statistics (mean/stddev/CI) and effect sizes is standard scientific practice and prevents over-interpreting a single run. We include Cohen’s \(d\) because it captures standardized effect magnitude, not only relative percent differences.

## 11. Determinism and Reproducibility

Given identical inputs (topology generation parameters, experiment spec, seeds), the simulator is deterministic:
- workload generation is deterministic per seed
- baseline policies are deterministic per seed/state
- topology generators are deterministic per seed
- observability lag/noise uses a deterministic RNG stream derived from the experiment seed

## 12. Parameter Choices ("Magic Numbers") and Justification

This section documents the main fixed constants used by the model and why they are chosen this way. The intent is not to claim these are universally correct, but to (1) avoid hidden assumptions, and (2) motivate ablations/sensitivity analyses.

- Tick size: the simulator uses 10 ms per tick when converting a continuous latency value (ms) into discrete completion time. This makes the model fast and deterministic while keeping resolution fine enough to preserve ordering of typical microservice latencies. A sensitivity check should vary this (e.g., 1/5/20 ms) to ensure conclusions do not depend on discretization.
- Max dependency depth \(D_{\max}=3\): this bounds recursive service-to-service chains, preventing unbounded work and cycles. Depth 3 is a pragmatic compromise: it allows nontrivial multi-hop coupling (gateway \(\to\) A \(\to\) B \(\to\) C) without turning each request into a large graph traversal. We also forbid revisiting the same logical service via a visited-set to avoid cyclic amplification.
- Async/fire factors \(\alpha(\mathrm{async})=0.25\), \(\alpha(\mathrm{fire})=0.10\): these scale downstream contribution to end-to-end latency to reflect overlap. They are coarse research knobs; publication runs should include ablations that sweep these factors or compare against a fully synchronous model.
- Downstream pressure coefficient in \(\beta(x,t)\): the multiplier (currently 10) calibrates pressure magnitudes into milliseconds so that downstream degradation competes with network and base service time. This coefficient should be reported explicitly and included in sensitivity analysis.
- Score weights: the `score-v1` weights define a hypothesis about relative importance of features (local latency/inflight vs downstream health). Publication runs must include an ablation study that removes feature groups or perturbs weights to show the effect is not an artifact of one hand-tuned vector.

## 12. Recommended Publication Run Command

Example (large-scale, no trace):

```bash
cargo run -p micro-net-cli -- bench \
  --topologies star,ring,full-mesh,random-sparse \
  --policies random,round-robin,least-inflight,score,score-local-only,score-local+network,score-local+downstream,score-no-downstream,score-no-host-pressure \
  --scenarios healthy,db-overloaded,cache-degraded,broker-lag,zone-burst,partial-failure \
  --load-levels 1,5,10,25,50,100 \
  --seeds 50 \
  --duration-ticks 12000 --warmup-ticks 2000 --drain-ticks 1000 \
  --observability-lag-ticks 10 --observability-noise 0.03 \
  --parallel 12 \
  --trace none --artifacts aggregate --progress-ms 1000 \
  --out /tmp/micro-net-paper-001
```

## 13. Conclusion (Paper Section Template)

This repository implements a deterministic, tick-based simulator for routing policy evaluation in microservice graphs with explicit dependency bindings and controlled degradation/failure regimes. The core methodological contribution is a reproducible protocol (warmup/measurement/drain, multi-seed aggregation, effect sizes) and an anti-circular observability model that forces policies to operate on lagged/noisy telemetry. After the publication-scale run completes, the results will be reported quantitatively (CI95 and standardized effect sizes) across topologies, load levels, and degradation scenarios to characterize when dependency-aware routing improves tail latency and when tradeoffs appear.
