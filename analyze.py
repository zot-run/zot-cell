#!/usr/bin/env python3
"""ZOT Cell v2.0 results analyzer.
Parses results.txt from the Darwinian KPR organism."""

import csv
import sys

def load_results(path="results.txt"):
    rows = []
    with open(path) as f:
        reader = csv.DictReader(f)
        for row in reader:
            if row.get("cycle", "").startswith("-"):
                break
            try:
                r = {
                    "cycle": int(row["cycle"]),
                    "threat": int(row["threat"]) == 1,
                    "block": int(row["block"]) == 1,
                    "correct": int(row["correct"]) == 1,
                    "vote_pct": float(row.get("vote_pct", row.get("combined", "0"))),
                    "fire_count": int(row.get("fire_count", "0")),
                    "n_receptors": int(row.get("n_receptors", "0")),
                    "memory_cells": int(row.get("memory_cells", "0")),
                    "raw_mem": float(row.get("raw_mem", row.get("raw_ns", "0"))),
                    "raw_clk": float(row.get("raw_clk", "0")),
                    "raw_alc": float(row.get("raw_alc", "0")),
                }
                rows.append(r)
            except (ValueError, KeyError):
                continue
    return rows

def stats(vals):
    if not vals:
        return {"n": 0, "mean": 0, "std": 0, "min": 0, "max": 0,
                "p10": 0, "p50": 0, "p90": 0}
    vals = sorted(vals)
    n = len(vals)
    mean = sum(vals) / n
    var = sum((v - mean) ** 2 for v in vals) / n
    return {
        "n": n, "mean": mean, "std": var ** 0.5,
        "min": vals[0], "max": vals[-1],
        "p10": vals[max(0, n // 10)],
        "p50": vals[n // 2],
        "p90": vals[max(0, n * 9 // 10)],
    }

def analyze(rows):
    quiet = [r for r in rows if not r["threat"]]
    threat = [r for r in rows if r["threat"]]

    print("=" * 70)
    print("ZOT CELL v2.0 RESULTS ANALYSIS")
    print("=" * 70)

    correct = sum(1 for r in rows if r["correct"])
    det = sum(1 for r in threat if r["block"])
    fp = sum(1 for r in quiet if r["block"])
    print(f"\nOVERALL: {correct}/{len(rows)} correct ({100*correct/len(rows):.1f}%)")
    print(f"  Detection: {det}/{len(threat)} ({100*det/len(threat):.1f}%)")
    print(f"  False Pos: {fp}/{len(quiet)} ({100*fp/len(quiet):.1f}%)")

    # Raw readings — all 3 sensors
    sensor_names = [("raw_mem", "MEMORY"), ("raw_clk", "CLOCK"), ("raw_alc", "ALLOC")]
    raw_seps = []
    print("\n--- RAW SENSOR READINGS (ns) ---")
    for key, name in sensor_names:
        q_raw = stats([r[key] for r in quiet])
        t_raw = stats([r[key] for r in threat])
        sep = abs(t_raw['mean'] - q_raw['mean']) / max(1, (q_raw['std'] + t_raw['std']) / 2)
        raw_seps.append(sep)
        print(f"  {name}:")
        print(f"    Quiet:  mean={q_raw['mean']:.0f} std={q_raw['std']:.0f} "
              f"[p10={q_raw['p10']:.0f} p50={q_raw['p50']:.0f} p90={q_raw['p90']:.0f}]")
        print(f"    Threat: mean={t_raw['mean']:.0f} std={t_raw['std']:.0f} "
              f"[p10={t_raw['p10']:.0f} p50={t_raw['p50']:.0f} p90={t_raw['p90']:.0f}]")
        print(f"    Separation: {sep:.2f} std")

    # Cross-correlation: do threats affect multiple sensors?
    print("\n--- CROSS-CORRELATION ---")
    for r in threat:
        pass  # just need the data
    t_both_mem_clk = sum(1 for r in threat if r["raw_mem"] < q_raw['p50'] and r["raw_clk"] > q_raw['p50'])
    q_both_mem_clk = sum(1 for r in quiet if r["raw_mem"] < q_raw['p50'] and r["raw_clk"] > q_raw['p50'])
    print(f"  Threat multi-sensor hits: {t_both_mem_clk}/{len(threat)}")
    print(f"  Quiet  multi-sensor hits: {q_both_mem_clk}/{len(quiet)}")

    # Vote analysis
    print("\n--- RECEPTOR VOTE ---")
    q_vote = stats([r["vote_pct"] for r in quiet])
    t_vote = stats([r["vote_pct"] for r in threat])
    print(f"  Quiet vote:  mean={q_vote['mean']:.3f} std={q_vote['std']:.3f} "
          f"[p10={q_vote['p10']:.3f} p50={q_vote['p50']:.3f} p90={q_vote['p90']:.3f}]")
    print(f"  Threat vote: mean={t_vote['mean']:.3f} std={t_vote['std']:.3f} "
          f"[p10={t_vote['p10']:.3f} p50={t_vote['p50']:.3f} p90={t_vote['p90']:.3f}]")

    # Fire count
    print("\n--- FIRE COUNT ---")
    q_fire = stats([r["fire_count"] for r in quiet])
    t_fire = stats([r["fire_count"] for r in threat])
    print(f"  Quiet:  mean={q_fire['mean']:.1f} max={q_fire['max']:.0f}")
    print(f"  Threat: mean={t_fire['mean']:.1f} max={t_fire['max']:.0f}")

    # Population dynamics
    print("\n--- POPULATION ---")
    pop = stats([r["n_receptors"] for r in rows])
    mem = stats([r["memory_cells"] for r in rows])
    print(f"  Receptors: mean={pop['mean']:.0f} min={pop['min']:.0f} max={pop['max']:.0f}")
    print(f"  Memory:    mean={mem['mean']:.0f} min={mem['min']:.0f} max={mem['max']:.0f}")

    # Threat period analysis
    print("\n--- THREAT PERIODS ---")
    threat_types = {10: "CPU", 30: "Cache", 50: "Mixed", 70: "Cache", 90: "CPU"}
    for start in [10, 30, 50, 70, 90]:
        end = start + 10
        period = [r for r in rows if start <= r["cycle"] < end]
        det_p = sum(1 for r in period if r["block"])
        first = next((r["cycle"] for r in period if r["block"]), None)
        avg_vote = sum(r["vote_pct"] for r in period) / len(period) if period else 0
        avg_fire = sum(r["fire_count"] for r in period) / len(period) if period else 0
        ttype = threat_types.get(start, "?")
        print(f"  T[{start}-{end-1}] {ttype:>5}: det={det_p}/10 "
              f"first={'c'+str(first) if first else 'NONE':>5} "
              f"avg_vote={avg_vote:.3f} avg_fire={avg_fire:.1f}")

    # Quiet period analysis
    print("\n--- QUIET PERIODS ---")
    for start in [0, 20, 40, 60, 80]:
        end = start + 10
        period = [r for r in rows if start <= r["cycle"] < end]
        fp_p = sum(1 for r in period if r["block"])
        avg_vote = sum(r["vote_pct"] for r in period) / len(period) if period else 0
        avg_fire = sum(r["fire_count"] for r in period) / len(period) if period else 0
        print(f"  Q[{start}-{end-1}]: fp={fp_p}/10 "
              f"avg_vote={avg_vote:.3f} avg_fire={avg_fire:.1f}")

    # Evolution tracking
    print("\n--- EVOLUTION ---")
    for cycle_mark in [9, 19, 29, 39, 49, 59, 69, 79, 89, 99]:
        r = rows[cycle_mark] if cycle_mark < len(rows) else None
        if r:
            state = "T" if r["threat"] else "Q"
            acc_pct = r.get("cum_ok", 0)
            print(f"  c{cycle_mark:03d} [{state}]: pop={r['n_receptors']} mem={r['memory_cells']}")

    # DIAGNOSIS
    print("\n" + "=" * 70)
    print("DIAGNOSIS")
    print("=" * 70)

    det_rate = det / max(1, len(threat))
    fp_rate = fp / max(1, len(quiet))

    if det_rate >= 0.9 and fp_rate <= 0.1:
        print(f"  EXCELLENT: {det_rate:.0%} detection, {fp_rate:.0%} FP")
    elif det_rate >= 0.7 and fp_rate <= 0.2:
        print(f"  GOOD: {det_rate:.0%} detection, {fp_rate:.0%} FP. Room to improve.")
    elif det_rate < 0.5:
        print(f"  UNDER-DETECTING: {det_rate:.0%}. Receptors too conservative or dying in thymus.")
        if pop['min'] < 50:
            print(f"  -> Population crashed to {pop['min']:.0f}. Relax thymic selection.")
    elif fp_rate > 0.3:
        print(f"  OVER-FIRING: {fp_rate:.0%} FP. Receptors too aggressive.")
        print(f"  -> Tighten thymic selection or raise QUORUM_FRACTION.")

    if raw_seps[0] < 1.0:
        print(f"  WEAK SIGNAL: Memory separation only {raw_seps[0]:.2f} std.")
        print(f"  -> Calibration may have captured threat-like state.")
    for i, (_, name) in enumerate(sensor_names):
        if raw_seps[i] >= 1.5:
            print(f"  STRONG {name}: separation {raw_seps[i]:.2f} std — good discriminator.")

if __name__ == "__main__":
    path = sys.argv[1] if len(sys.argv) > 1 else "results.txt"
    rows = load_results(path)
    if not rows:
        print("No data loaded.")
        sys.exit(1)
    analyze(rows)
