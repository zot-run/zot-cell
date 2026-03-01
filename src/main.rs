//! ZOT Cell v2.0 — Darwinian Kinetic Proofreading Organism
//!
//! THE MERGE: v0.8 receptor diversity + v1.2 time-aware signaling.
//!
//! KEY FIX: Multi-sensor cross-correlation.
//! Quiet fast-mode: memory drops to 180K but clock/alloc stay normal.
//! Threat: memory drops AND clock/alloc change (cross-pressure).
//! Each receptor picks 2 of 3 sensors. Must see anomaly in BOTH.
//!
//! Each receptor has:
//!   - Sensor pair selection (which 2 of 3 sensors to watch)
//!   - Signal weights (pct_w, trans_w, lock_w)
//!   - KPR parameters (kpr_n, threshold, feedback)
//!   - Temporal window sizes
//!   - Independent cascade state
//!   - Confidence earned through Darwinian selection

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const PROBES_PER_READ: usize = 50;
const TOTAL_CYCLES: u32 = 100;
const NUM_SENSORS: usize = 3; // memory, clock, alloc

const CAL_DURATION_SECS: u64 = 60;
const CAL_INTERVAL_MS: u64 = 50;

const THYMUS_CANDIDATES: usize = 2000;
const MAX_RECEPTORS: usize = 500;
const CLONE_BATCH: usize = 5;
const MUTATION_RATE: f64 = 0.15;
const MAX_SELF_FIRE_PCT: f64 = 0.10;
const MIN_CONFIDENCE: f64 = 0.02;
const MEMORY_THRESHOLD: f64 = 0.80;
const QUORUM_FRACTION: f64 = 0.10;

// --- PROBES ---

fn probe_memory() -> f64 {
    const SIZE: usize = 1024 * 1024;
    let mut buf = vec![0u8; SIZE];
    for i in (0..SIZE).step_by(64) { buf[i] = ((i + 64) % SIZE & 0xFF) as u8; }
    let start = Instant::now();
    let mut idx: usize = 0;
    for _ in 0..(SIZE / 64) * 6 {
        idx = ((buf[idx] as usize) | ((idx + 64) & !0xFF)) % SIZE;
        idx &= !(64 - 1);
    }
    std::hint::black_box(idx);
    start.elapsed().as_nanos() as f64
}

fn probe_clock() -> f64 {
    let a = Instant::now();
    for _ in 0..80 { std::hint::black_box(Instant::now()); }
    Instant::now().duration_since(a).as_nanos() as f64
}

fn probe_alloc() -> f64 {
    let start = Instant::now();
    for _ in 0..8 {
        let mut v = vec![0u8; 64 * 1024];
        for i in (0..v.len()).step_by(4096) { v[i] = 1; }
        std::hint::black_box(&v);
        drop(v);
    }
    start.elapsed().as_nanos() as f64
}

fn read_sensors() -> [f64; NUM_SENSORS] {
    let mut mems: Vec<f64> = (0..PROBES_PER_READ).map(|_| probe_memory()).collect();
    let mut clks: Vec<f64> = (0..PROBES_PER_READ).map(|_| probe_clock()).collect();
    let mut alcs: Vec<f64> = (0..PROBES_PER_READ).map(|_| probe_alloc()).collect();
    mems.sort_by(|a, b| a.partial_cmp(b).unwrap());
    clks.sort_by(|a, b| a.partial_cmp(b).unwrap());
    alcs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    [mems[mems.len()/2], clks[clks.len()/2], alcs[alcs.len()/2]]
}

// --- CALIBRATION ---
// Per-sensor calibration profiles.

struct SensorProfile {
    sorted: Vec<f64>,
    mean: f64,
    std: f64,
    p20: f64,
    p10: f64,
    p2: f64,
}

impl SensorProfile {
    fn from_samples(samples: &[f64]) -> Self {
        let n = samples.len();
        let mean = samples.iter().sum::<f64>() / n as f64;
        let var = samples.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
        let mut sorted = samples.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p20 = sorted[n * 20 / 100];
        let p10 = sorted[n * 10 / 100];
        let p2 = sorted[n * 2 / 100];
        SensorProfile { sorted, mean, std: var.sqrt().max(1.0), p20, p10, p2 }
    }

    /// Anomaly signal: how far below p20 is this reading?
    /// Returns 0.0 if at or above p20, up to 1.0 if at p2 or below.
    fn anomaly(&self, value: f64) -> f64 {
        if value >= self.p20 { return 0.0; }
        let range = (self.p20 - self.p2).max(1.0);
        ((self.p20 - value) / range).max(0.0).min(1.0)
    }
}

struct CalibrationProfile {
    sensors: [SensorProfile; NUM_SENSORS],
    raw_history: Vec<[f64; NUM_SENSORS]>, // time-ordered for thymic selection
}

impl CalibrationProfile {
    fn from_samples(samples: Vec<[f64; NUM_SENSORS]>) -> Self {
        let s0: Vec<f64> = samples.iter().map(|s| s[0]).collect();
        let s1: Vec<f64> = samples.iter().map(|s| s[1]).collect();
        let s2: Vec<f64> = samples.iter().map(|s| s[2]).collect();
        CalibrationProfile {
            sensors: [
                SensorProfile::from_samples(&s0),
                SensorProfile::from_samples(&s1),
                SensorProfile::from_samples(&s2),
            ],
            raw_history: samples,
        }
    }
}

// --- RECEPTOR ---
// Each receptor watches 2 sensors and has its own KPR cascade.

#[derive(Clone)]
struct Receptor {
    // Which 2 sensors this receptor watches (0-2)
    sensor_a: usize,
    sensor_b: usize,
    // How much weight on each sensor's anomaly (sum to 1.0)
    weight_a: f64,
    weight_b: f64,

    // Signal processing weights
    pct_w: f64,    // percentile anomaly weight
    trans_w: f64,  // transition weight
    lock_w: f64,   // lock (stability) weight

    // Temporal windows
    short_window: usize,
    long_window: usize,

    // KPR cascade parameters
    kpr_n: u32,
    kpr_m: u32,
    threshold: f64,
    feedback_boost: f64,

    // Runtime cascade state
    consecutive: u32,
    gap_count: u32,
    feedback_active: bool,
    eff_threshold: f64,

    // Fitness
    confidence: f64,
    fires: u32,
    correct: u32,
    is_memory: bool,
    age: u32,
    generation: u32,
}

impl Receptor {
    fn random(seed: u64) -> Self {
        let mut rng = seed;
        let mut next = || -> f64 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((rng >> 33) as f64) / (u32::MAX as f64)
        };

        // Pick 2 different sensors
        let sa = (next() * NUM_SENSORS as f64) as usize % NUM_SENSORS;
        let mut sb = (next() * (NUM_SENSORS - 1) as f64) as usize % NUM_SENSORS;
        if sb >= sa { sb = (sb + 1) % NUM_SENSORS; }

        let wa = 0.3 + next() * 0.4; // 0.3-0.7
        let wb = 1.0 - wa;

        let pw = 0.2 + next() * 0.6;
        let tw = next() * 0.6;
        let lw = next() * 0.6;
        let total = pw + tw + lw;

        let short = 2 + (next() * 4.0) as usize;
        let long = short + 3 + (next() * 8.0) as usize;
        let kpr_n = 2 + (next() * 4.0) as u32;
        let kpr_m = 1 + (next() * (kpr_n - 1) as f64) as u32;
        let threshold = 0.20 + next() * 0.50;
        let feedback = 0.05 + next() * 0.15;

        Receptor {
            sensor_a: sa, sensor_b: sb,
            weight_a: wa, weight_b: wb,
            pct_w: pw / total, trans_w: tw / total, lock_w: lw / total,
            short_window: short, long_window: long,
            kpr_n, kpr_m: kpr_m.min(kpr_n - 1).max(1),
            threshold, feedback_boost: feedback,
            consecutive: 0, gap_count: 0, feedback_active: false,
            eff_threshold: threshold,
            confidence: 0.4, fires: 0, correct: 0,
            is_memory: false, age: 0, generation: 0,
        }
    }

    fn mutate(&self, seed: u64) -> Self {
        let mut child = self.clone();
        let mut rng = seed;
        let mut perturb = |val: f64, lo: f64, hi: f64| -> f64 {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let delta = (((rng >> 33) as f64 / u32::MAX as f64) - 0.5) * 2.0 * MUTATION_RATE;
            (val + delta * (hi - lo)).max(lo).min(hi)
        };

        child.weight_a = perturb(self.weight_a, 0.2, 0.8);
        child.weight_b = 1.0 - child.weight_a;

        child.pct_w = perturb(self.pct_w, 0.1, 0.8);
        child.trans_w = perturb(self.trans_w, 0.0, 0.7);
        child.lock_w = perturb(self.lock_w, 0.0, 0.7);
        let t = child.pct_w + child.trans_w + child.lock_w;
        child.pct_w /= t; child.trans_w /= t; child.lock_w /= t;

        child.threshold = perturb(self.threshold, 0.15, 0.65);
        child.feedback_boost = perturb(self.feedback_boost, 0.03, 0.25);

        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        if (rng >> 60) < 4 {
            child.short_window = (self.short_window as i32 + if rng & 1 == 0 { 1 } else { -1 })
                .max(2).min(6) as usize;
        }
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        if (rng >> 60) < 4 {
            child.long_window = (self.long_window as i32 + if rng & 1 == 0 { 1 } else { -1 })
                .max(child.short_window as i32 + 3).min(15) as usize;
        }
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        if (rng >> 60) < 3 {
            child.kpr_n = (self.kpr_n as i32 + if rng & 1 == 0 { 1 } else { -1 }).max(2).min(6) as u32;
            child.kpr_m = child.kpr_m.min(child.kpr_n - 1).max(1);
        }
        // Occasionally swap a sensor
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        if (rng >> 60) < 2 { // ~12%
            let new_s = (rng >> 50) as usize % NUM_SENSORS;
            if new_s != child.sensor_b { child.sensor_a = new_s; }
            else if new_s != child.sensor_a { child.sensor_b = new_s; }
        }

        child.consecutive = 0; child.gap_count = 0;
        child.feedback_active = false; child.eff_threshold = child.threshold;
        child.confidence = 0.35; child.fires = 0; child.correct = 0;
        child.is_memory = false; child.age = 0;
        child.generation = self.generation + 1;
        child
    }

    /// Compute signal from multi-sensor history.
    fn compute_signal(&self, history: &[[f64; NUM_SENSORS]], profile: &CalibrationProfile) -> f64 {
        let n = history.len();
        if n == 0 { return 0.0; }

        let current = &history[n - 1];

        // Per-sensor anomaly (gated at p20)
        let anom_a = profile.sensors[self.sensor_a].anomaly(current[self.sensor_a]);
        let anom_b = profile.sensors[self.sensor_b].anomaly(current[self.sensor_b]);

        // CROSS-CORRELATION GATE: both sensors must show anomaly.
        // Quiet fast-mode: memory drops but clock/alloc stay normal -> one sensor = 0.
        // Threat: multiple sensors affected -> both > 0.
        let pct = anom_a * self.weight_a + anom_b * self.weight_b;
        if pct <= 0.0 { return 0.0; }
        // Require both sensors to contribute. Single-sensor anomaly is damped.
        if anom_a < 0.01 || anom_b < 0.01 { return pct * 0.3; }

        // Transition signal on primary sensor
        let trans = if n >= self.long_window {
            let sa = self.sensor_a;
            let short_start = n.saturating_sub(self.short_window);
            let long_start = n.saturating_sub(self.long_window);
            let short_mean = history[short_start..].iter().map(|h| h[sa]).sum::<f64>()
                / self.short_window as f64;
            let long_slice = &history[long_start..short_start];
            let long_mean = if long_slice.is_empty() { short_mean }
                else { long_slice.iter().map(|h| h[sa]).sum::<f64>() / long_slice.len() as f64 };
            let delta = (long_mean - short_mean) / profile.sensors[sa].std;
            delta.max(0.0).min(1.0)
        } else { 0.0 };

        // Lock signal on primary sensor — measures stability
        let lock = if n >= self.short_window {
            let sa = self.sensor_a;
            let start = n.saturating_sub(self.short_window);
            let w: Vec<f64> = history[start..].iter().map(|h| h[sa]).collect();
            let wm = w.iter().sum::<f64>() / w.len() as f64;
            let wstd = (w.iter().map(|v| (v - wm).powi(2)).sum::<f64>() / w.len() as f64).sqrt();
            (1.0 - wstd / profile.sensors[sa].std).max(0.0).min(1.0)
        } else { 0.0 };

        (pct * self.pct_w + trans * self.trans_w + lock * self.lock_w).min(1.0)
    }

    fn kpr_step(&mut self, signal: f64) -> bool {
        if signal > self.eff_threshold {
            self.gap_count = 0;
            self.consecutive += 1;
            if self.consecutive >= self.kpr_m && !self.feedback_active {
                self.feedback_active = true;
                self.eff_threshold = self.threshold + self.feedback_boost;
            }
        } else {
            self.gap_count += 1;
            if signal <= 0.01 || self.gap_count >= 3 {
                self.consecutive = 0;
                self.feedback_active = false;
                self.eff_threshold = self.threshold;
                self.gap_count = 0;
            }
        }
        self.consecutive >= self.kpr_n
    }

    fn learn(&mut self, fired: bool, truth: bool) {
        self.age += 1;
        if fired {
            self.fires += 1;
            if truth {
                self.correct += 1;
                self.confidence = (self.confidence + 0.04).min(1.0);
                if self.confidence >= MEMORY_THRESHOLD { self.is_memory = true; }
            } else {
                self.confidence = (self.confidence - 0.10).max(0.0);
            }
        }
    }

    fn accuracy(&self) -> f64 {
        if self.fires == 0 { 0.5 } else { self.correct as f64 / self.fires as f64 }
    }
}


// --- THREATS ---

fn spawn_cpu_stress(stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut x: u64 = 0;
        while !stop.load(Ordering::Relaxed) {
            for _ in 0..10_000 {
                x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            }
            std::hint::black_box(x);
            thread::yield_now();
        }
    })
}

fn spawn_cache_thrash(stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut data = vec![0u8; 8 * 1024 * 1024];
        let mut idx: usize = 7;
        while !stop.load(Ordering::Relaxed) {
            for _ in 0..10_000 {
                idx = (idx.wrapping_mul(1103515245).wrapping_add(12345)) % data.len();
                data[idx] = data[idx].wrapping_add(1);
            }
            std::hint::black_box(data[idx]);
        }
    })
}

fn start_threat(kind: u32) -> (Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
    let stop = Arc::new(AtomicBool::new(false));
    let handles = match kind % 3 {
        0 => (0..4).map(|_| spawn_cpu_stress(stop.clone())).collect(),
        1 => (0..4).map(|_| spawn_cache_thrash(stop.clone())).collect(),
        _ => {
            let mut h: Vec<thread::JoinHandle<()>> = (0..2).map(|_| spawn_cpu_stress(stop.clone())).collect();
            h.extend((0..2).map(|_| spawn_cache_thrash(stop.clone())));
            h
        }
    };
    thread::sleep(Duration::from_millis(500)); // let threat saturate substrate
    (stop, handles)
}

fn stop_threat(stop: Arc<AtomicBool>, handles: Vec<thread::JoinHandle<()>>) {
    stop.store(true, Ordering::Relaxed);
    for h in handles { let _ = h.join(); }
    thread::sleep(Duration::from_millis(200));
}


// --- CELL: the organism ---

struct Cell {
    receptors: Vec<Receptor>,
    history: Vec<[f64; NUM_SENSORS]>,
    profile: CalibrationProfile,
    seed: u64,
    prev_vote: f64,
    was_blocking: bool,
    suppression: f64, // regulatory T-cell suppression (0.0 = none, decays over time)
}

impl Cell {
    fn calibrate() -> Self {
        eprintln!("CALIBRATING ({CAL_DURATION_SECS}s)...");
        let mut samples = Vec::new();
        let start = Instant::now();
        while start.elapsed().as_secs() < CAL_DURATION_SECS {
            samples.push(read_sensors());
            thread::sleep(Duration::from_millis(CAL_INTERVAL_MS));
        }
        eprintln!("  collected {} samples", samples.len());
        let profile = CalibrationProfile::from_samples(samples);
        eprintln!("  sensor 0 (mem):   mean={:.0} std={:.0} p20={:.0}",
            profile.sensors[0].mean, profile.sensors[0].std, profile.sensors[0].p20);
        eprintln!("  sensor 1 (clock): mean={:.0} std={:.0} p20={:.0}",
            profile.sensors[1].mean, profile.sensors[1].std, profile.sensors[1].p20);
        eprintln!("  sensor 2 (alloc): mean={:.0} std={:.0} p20={:.0}",
            profile.sensors[2].mean, profile.sensors[2].std, profile.sensors[2].p20);

        let seed = Instant::now().elapsed().as_nanos() as u64 ^ 0xDEAD_BEEF;
        let mut cell = Cell {
            receptors: Vec::new(),
            history: profile.raw_history.clone(),
            profile,
            seed,
            prev_vote: 0.0,
            was_blocking: false,
            suppression: 0.0,
        };
        cell.thymic_selection();
        cell
    }

    fn next_seed(&mut self) -> u64 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.seed
    }

    fn thymic_selection(&mut self) {
        eprintln!("THYMIC SELECTION: generating {THYMUS_CANDIDATES} candidates...");
        let cal_len = self.profile.raw_history.len();
        let mut candidates: Vec<Receptor> = (0..THYMUS_CANDIDATES)
            .map(|i| {
                let s = self.next_seed().wrapping_add(i as u64);
                Receptor::random(s)
            })
            .collect();

        // Test each candidate against calibration history (self).
        // Reject any that fire on self more than MAX_SELF_FIRE_PCT.
        let window_size = 15.min(cal_len);
        let test_windows: Vec<&[[f64; NUM_SENSORS]]> = (0..cal_len.saturating_sub(window_size))
            .step_by(3)
            .map(|start| &self.profile.raw_history[start..start + window_size])
            .collect();

        let mut survivors = Vec::new();
        for c in candidates.iter_mut() {
            let mut fires = 0;
            let total = test_windows.len();
            for window in &test_windows {
                let sig = c.compute_signal(window, &self.profile);
                let fired = c.kpr_step(sig);
                if fired { fires += 1; }
                // Reset cascade between windows
                c.consecutive = 0;
                c.gap_count = 0;
                c.feedback_active = false;
                c.eff_threshold = c.threshold;
            }
            let fire_rate = fires as f64 / total.max(1) as f64;
            if fire_rate <= MAX_SELF_FIRE_PCT {
                c.consecutive = 0;
                c.gap_count = 0;
                c.feedback_active = false;
                c.eff_threshold = c.threshold;
                survivors.push(c.clone());
            }
        }

        survivors.truncate(MAX_RECEPTORS);
        eprintln!("  survivors: {}/{THYMUS_CANDIDATES}", survivors.len());
        self.receptors = survivors;
    }


    fn decide(&mut self) -> (bool, usize, f64) {
        let reading = read_sensors();
        self.history.push(reading);
        if self.history.len() > 200 {
            self.history.drain(0..self.history.len() - 200);
        }

        let mut fire_count = 0usize;
        let mut weighted_vote = 0.0f64;
        let mut total_weight = 0.0f64;

        for r in self.receptors.iter_mut() {
            let sig = r.compute_signal(&self.history, &self.profile);
            let fired = r.kpr_step(sig);
            let w = r.confidence;
            total_weight += w;
            if fired {
                fire_count += 1;
                weighted_vote += w;
            }
        }

        let vote_pct = if total_weight > 0.0 { weighted_vote / total_weight } else { 0.0 };

        // REGULATORY T-CELL SUPPRESSION:
        // When transitioning from blocking to non-blocking (threat resolving),
        // activate suppression. This prevents recovery-phase false positives.
        // Biology: Tregs suppress effector T-cells after pathogen clearance.
        if self.was_blocking && vote_pct < self.prev_vote * 0.8 {
            // Signal dropped significantly — threat is leaving, activate suppression
            self.suppression = 0.9;
        }
        // Decay suppression over time
        self.suppression *= 0.4; // fast decay: 0.9 -> 0.36 -> 0.14 -> 0.06 -> ~0

        // Apply suppression: raise effective quorum
        let effective_quorum = QUORUM_FRACTION + self.suppression * (1.0 - QUORUM_FRACTION);
        let block = vote_pct >= effective_quorum;

        self.prev_vote = vote_pct;
        self.was_blocking = block;
        (block, fire_count, vote_pct)
    }

    fn learn(&mut self, truth: bool) {
        // Tell each receptor whether it was right
        for r in self.receptors.iter_mut() {
            let fired = r.consecutive >= r.kpr_n;
            r.learn(fired, truth);
        }
    }

    fn evolve(&mut self) {
        // Kill low-confidence non-memory receptors
        self.receptors.retain(|r| r.confidence >= MIN_CONFIDENCE || r.is_memory);

        // Clone top performers
        let mut by_fitness: Vec<usize> = (0..self.receptors.len()).collect();
        by_fitness.sort_by(|&a, &b| {
            self.receptors[b].confidence.partial_cmp(&self.receptors[a].confidence).unwrap()
        });

        let mut new_receptors = Vec::new();
        let clone_count = CLONE_BATCH.min(by_fitness.len());
        for &idx in by_fitness.iter().take(clone_count) {
            if self.receptors.len() + new_receptors.len() >= MAX_RECEPTORS { break; }
            let s = self.next_seed();
            let child = self.receptors[idx].mutate(s);
            new_receptors.push(child);
        }
        self.receptors.extend(new_receptors);

        // If population too low, inject fresh random receptors
        while self.receptors.len() < MAX_RECEPTORS / 4 {
            let s = self.next_seed();
            self.receptors.push(Receptor::random(s));
        }
    }

    fn memory_count(&self) -> usize {
        self.receptors.iter().filter(|r| r.is_memory).count()
    }
}


// --- MAIN ---

fn main() {
    eprintln!("ZOT Cell v2.0 — Darwinian KPR Organism");
    eprintln!("  sensors: memory, clock, alloc");
    eprintln!("  receptors: {MAX_RECEPTORS} (from {THYMUS_CANDIDATES} candidates)");
    eprintln!("  cycles: {TOTAL_CYCLES}");
    eprintln!();

    let mut cell = Cell::calibrate();

    // CSV output
    let mut out = File::create("results.txt").expect("cannot create results.txt");
    writeln!(out, "cycle,threat,block,correct,vote_pct,fire_count,n_receptors,memory_cells,raw_mem,raw_clk,raw_alc").unwrap();

    let mut total_correct = 0u32;
    let mut active_threat: Option<(Arc<AtomicBool>, Vec<thread::JoinHandle<()>>)> = None;

    // Schedule: 10 quiet, 10 threat, repeating. 5 threat periods across 100 cycles.
    // Threat types rotate: CPU(0), Cache(1), Mixed(2), Cache(1), CPU(0)
    let threat_types = [0u32, 1, 2, 1, 0];

    for cycle in 0..TOTAL_CYCLES {
        let period = cycle / 10;
        let is_threat_period = period % 2 == 1;
        let phase_start = cycle % 10 == 0;

        // Start/stop threats at period boundaries
        if phase_start {
            if let Some((stop, handles)) = active_threat.take() {
                stop_threat(stop, handles);
            }
            if is_threat_period {
                let kind = threat_types[(period / 2) as usize % threat_types.len()];
                let tname = match kind % 3 { 0 => "CPU", 1 => "Cache", _ => "Mixed" };
                eprintln!("  c{cycle:03}: START threat ({tname})");
                active_threat = Some(start_threat(kind));
            }
        }

        let truth = is_threat_period;
        let (block, fire_count, vote_pct) = cell.decide();
        let correct = block == truth;
        if correct { total_correct += 1; }

        // Get raw sensor values for CSV (last reading in history)
        let raw = cell.history.last().copied().unwrap_or([0.0; NUM_SENSORS]);

        let mark = if correct { "ok" } else if block && !truth { "FP" } else { "MISS" };
        eprintln!("  c{cycle:03} [{}] vote={vote_pct:.3} fire={fire_count} pop={} mem={} {mark}",
            if truth { "T" } else { "Q" },
            cell.receptors.len(), cell.memory_count());

        writeln!(out, "{cycle},{},{},{},{vote_pct:.4},{fire_count},{},{},{:.0},{:.0},{:.0}",
            truth as u8, block as u8, correct as u8,
            cell.receptors.len(), cell.memory_count(),
            raw[0], raw[1], raw[2]).unwrap();

        // Feedback
        cell.learn(truth);

        // Evolve every 5 cycles
        if cycle % 5 == 4 {
            cell.evolve();
        }

        thread::sleep(Duration::from_millis(200));
    }

    // Cleanup
    if let Some((stop, handles)) = active_threat.take() {
        stop_threat(stop, handles);
    }

    let acc = total_correct as f64 / TOTAL_CYCLES as f64;
    eprintln!("\nFINAL: {total_correct}/{TOTAL_CYCLES} correct ({:.1}%)", acc * 100.0);
    eprintln!("  population: {} receptors, {} memory", cell.receptors.len(), cell.memory_count());

    // Summary line in CSV
    writeln!(out, "---,---,---,---,---,---,---,---,---,---,---").unwrap();
    writeln!(out, "# accuracy={:.4} detection=? fp=? pop={} mem={}",
        acc, cell.receptors.len(), cell.memory_count()).unwrap();
}
