<p align="center">
  <strong>Z O T &nbsp; C E L L</strong><br>
  <em>Darwinian Kinetic Proofreading Organism</em>
</p>

<p align="center">
  <a href="https://github.com/zot-run/zot-cell/actions/workflows/build.yml"><img src="https://github.com/zot-run/zot-cell/actions/workflows/build.yml/badge.svg" alt="Build"></a>
  <a href="https://github.com/zot-run/zot-cell/actions/workflows/benchmark.yml"><img src="https://github.com/zot-run/zot-cell/actions/workflows/benchmark.yml/badge.svg" alt="Benchmark"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.70%2B-orange.svg" alt="Rust 1.70+"></a>
  <img src="https://img.shields.io/badge/zero-dependencies-brightgreen.svg" alt="Zero Dependencies">
  <img src="https://img.shields.io/badge/accuracy-93--95%25-success.svg" alt="Accuracy: 93-95%">
  <img src="https://img.shields.io/badge/false%20positives-0--2%25-success.svg" alt="FP: 0-2%">
</p>

---

A digital immune system in a single Rust file. No dependencies. No ML frameworks.
No training data. 500 evolved receptors sense the physical substrate through
three independent probes, discriminate self from non-self through kinetic
proofreading cascades, and adapt through Darwinian selection — all in real time.

This is not anomaly detection. This is a living organism that feels its own body.

## What It Does

ZOT Cell drops onto a machine, calibrates against the quiet substrate for 60 seconds,
evolves a population of immune receptors through thymic negative selection, then
continuously monitors for computational threats (CPU stress, cache thrash, memory
pressure) by sensing nanosecond-level timing perturbations across three physical channels.

```
Accuracy:  93-95%  (consistent across runs)
Detection: 88-92%  (threats correctly identified)
False Pos: 0-2%    (quiet periods incorrectly flagged)
```

Tested on macOS Apple Silicon. 100 decision cycles. 5 threat types. Zero false
positives in most runs.

## Quick Start

```bash
git clone https://github.com/zot-run/zot-cell.git
cd zot-cell
cargo build --release
cargo run --release          # 60s calibration + ~20s test = ~90s total
python3 analyze.py           # parse results, print analysis
```

## How It Works

</p>

### Three Probes (Sensory Organs)

| Probe | What It Measures | How |
|-------|-----------------|-----|
| Memory | Cache hierarchy latency | 1MB pointer chase, 6 passes across cache lines |
| Clock | Timer access contention | 80 consecutive `Instant::now()` calls |
| Alloc | Allocator pressure | 8 cycles of 64KB alloc/touch/drop |

Each probe runs 50 times per cycle. The median is the reading. Three readings
form a sensor vector `[f64; 3]` — the organism's perception of its body at
that instant.

### Calibration (60 seconds)

During quiet operation, the organism builds per-sensor statistical profiles:
mean, standard deviation, and percentile boundaries (p2, p10, p20). These
define "self" — the normal substrate signature.

### Thymic Selection (Negative Selection)

2,000 random receptors are generated. Each is tested against the calibration
history. Any receptor that fires on self more than 10% of the time is killed.
Survivors (up to 500) form the initial immune repertoire.

### The Receptor (~50 bytes)

Each receptor is a complete immune cell:

```
sensor_a, sensor_b       which 2 of 3 sensors to watch
weight_a, weight_b       per-sensor anomaly weighting
pct_w, trans_w, lock_w   signal component weights
short_window, long_window temporal integration windows
kpr_n, kpr_m             cascade depth and feedback trigger
threshold, feedback_boost KPR parameters
confidence, is_memory     Darwinian fitness state
```

Receptor 1 might evolve high transition weight + short KPR window (fast-acting,
catches sudden spikes). Receptor 2 might evolve high lock weight + long KPR
window (slow-acting, catches stealthy sustained drains). The organism evaluates
the substrate through 500 different time-aware lenses simultaneously.

### Signal Computation

```
1. Per-sensor anomaly     gated at p20, normalized to [0,1] against p2
2. Cross-correlation gate BOTH sensors must show anomaly (key discriminator)
3. Transition signal      short-window mean vs long-window mean drop
4. Lock signal            low variance in short window = sustained threat
5. Weighted sum           pct * pct_w + trans * trans_w + lock * lock_w
```

### Kinetic Proofreading Cascade

Inspired by McKeithan 1995. Each receptor maintains an independent cascade:

- Signal above threshold: `consecutive++`
- Consecutive reaches `kpr_m`: feedback boost activates (raises threshold)
- Consecutive reaches `kpr_n`: receptor FIRES
- Signal drops: gap counter increments, cascade resets after 3 gaps

This filters transient noise. Only sustained, multi-cycle anomalies trigger firing.

### Quorum Decision

Weighted vote across all receptors. If `vote >= 10%` of total confidence weight,
the organism blocks. Regulatory T-cell suppression activates when vote is declining
from a previous block — prevents recovery-phase false positives.

### Darwinian Evolution (every 5 cycles)

- Kill receptors with confidence below 2% (unless memory cells)
- Clone top 5 performers with mutation (15% perturbation)
- Promote receptors with confidence above 80% to permanent memory cells
- Inject random receptors if population drops below 125

## The Core Problem Solved

On macOS, the memory subsystem naturally visits "fast mode" (~180K ns) during
quiet periods. These readings are physically identical to threat readings on
any single sensor. No threshold on one sensor can distinguish them.

Three mechanisms solve this:

1. **Cross-correlation gate** — threats affect multiple sensors simultaneously.
   Quiet fast-mode only affects memory. A receptor watching memory+clock sees
   clock anomaly = 0 during quiet dips, damping signal to 30%.

2. **KPR cascade** — requires N consecutive above-threshold signals. Quiet dips
   are transient (1-2 cycles). Threats are sustained (10+ cycles).

3. **Regulatory suppression** — after threat resolves, declining vote pattern
   activates Treg analog, raising effective quorum to prevent recovery FP.

## Results

Sample output from `analyze.py`:

```
======================================================================
ZOT CELL v2.0 RESULTS ANALYSIS
======================================================================

OVERALL: 95/100 correct (95.0%)
  Detection: 45/50 (90.0%)
  False Pos: 0/50 (0.0%)

--- RAW SENSOR READINGS (ns) ---
  MEMORY:
    Quiet:  mean=288933 std=27687 [p10=254208 p50=287000 p90=323375]
    Threat: mean=188490 std=11828 [p10=179500 p50=184792 p90=203083]
    Separation: 5.08 std
  CLOCK:
    Quiet:  mean=2199 std=709 [p10=1709 p50=1917 p90=3375]
    Threat: mean=1301 std=70  [p10=1291 p50=1291 p90=1292]
    Separation: 2.31 std
  ALLOC:
    Quiet:  mean=17786 std=3843 [p10=12333 p50=18625 p90=22125]
    Threat: mean=9419  std=1623 [p10=8334  p50=8583  p90=11125]
    Separation: 3.06 std

--- THREAT PERIODS ---
  T[10-19]   CPU: det=9/10  first=c11  avg_vote=0.846
  T[30-39] Cache: det=9/10  first=c31  avg_vote=0.848
  T[50-59] Mixed: det=9/10  first=c51  avg_vote=0.845
  T[70-79] Cache: det=9/10  first=c71  avg_vote=0.850
  T[90-99]   CPU: det=9/10  first=c91  avg_vote=0.845

--- QUIET PERIODS ---
  Q[0-9]:   fp=0/10  avg_vote=0.000
  Q[20-29]: fp=0/10  avg_vote=0.000
  Q[40-49]: fp=0/10  avg_vote=0.000
  Q[60-69]: fp=0/10  avg_vote=0.000
  Q[80-89]: fp=0/10  avg_vote=0.000

DIAGNOSIS
  EXCELLENT: 90% detection, 0% FP
```

Each threat period detects 9/10 — the first cycle is always a MISS due to KPR
ramp-up. This is biologically correct: the immune system does not respond instantly.

## Architecture

```
SUBSTRATE (the body)
    |
    v
3 PROBES ──────────────> read_sensors() -> [f64; 3]
    |
    v
CALIBRATION (60s) ─────> SensorProfile { mean, std, p2, p10, p20 }
    |
    v
THYMIC SELECTION ──────> 2000 candidates -> 500 survivors
    |
    v
500 RECEPTORS ─────────> each: 2 sensors, KPR cascade, signal weights
    |
    v
SIGNAL + KPR ──────────> per-receptor fire/no-fire decision
    |
    v
QUORUM + TREG ─────────> organism-level block/allow decision
    |
    v
DARWINIAN EVOLUTION ───> clone winners, kill losers, mutate children
```

## Configuration

All constants are at the top of `src/main.rs`:

| Constant | Default | Purpose |
|----------|---------|---------|
| `PROBES_PER_READ` | 50 | Readings per sensor per cycle |
| `TOTAL_CYCLES` | 100 | Test duration in decision cycles |
| `CAL_DURATION_SECS` | 60 | Calibration period |
| `THYMUS_CANDIDATES` | 2000 | Random receptors generated |
| `MAX_RECEPTORS` | 500 | Population cap |
| `MAX_SELF_FIRE_PCT` | 0.10 | Thymic rejection threshold |
| `QUORUM_FRACTION` | 0.10 | Minimum vote to block |
| `MEMORY_THRESHOLD` | 0.80 | Confidence for memory cell promotion |
| `MUTATION_RATE` | 0.15 | Child parameter perturbation range |
| `CLONE_BATCH` | 5 | Top performers cloned per evolution |
| `MIN_CONFIDENCE` | 0.02 | Kill threshold for non-memory cells |

## Output

- `stderr`: live progress (calibration, per-cycle decisions, final score)
- `results.txt`: CSV for analysis

CSV columns:
```
cycle,threat,block,correct,vote_pct,fire_count,n_receptors,memory_cells,raw_mem,raw_clk,raw_alc
```

## Version History

| Version | Architecture | Accuracy | Notes |
|---------|-------------|----------|-------|
| v0.6 | Calibrated probes | N/A | Validated substrate sensing |
| v0.7 | Single-receptor z-score | 66% | Inconsistent signal |
| v0.8 | HAMP receptor diversity | 0% | Failed on macOS |
| v0.9 | Counter-based KPR | 66% | Z-score inconsistent |
| v1.0 | Otsu + percentile-rank | 62% | Variance caused FP |
| v1.1 | Transition-aware KPR | 83% | Inconsistent across runs |
| v2.0 | Darwinian KPR organism | 93-95% | Multi-sensor + Treg suppression |

## Research

The theoretical architecture spans 15 internal research papers. These are not published.

The biological foundations — enough to understand why each mechanism exists — are
documented at [zot.run/research](https://zot.run/research).

The code is the proof. Run it on your machine and see.

## License

MIT
