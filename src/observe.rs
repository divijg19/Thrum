//! Observation engine: computes structured explanations of system behavior.
//!
//! Observations are derived explanations — not raw metrics. They answer *why*
//! questions about system behavior and form the core primitive of the Thrum
//! observatory architecture.
//!
//! The engine is pure (no IO, no mutation), stateless (all state passed in),
//! and composable (individual observation functions are independent).
//! Observations are ephemeral — recomputed every frame from current and
//! previous sample snapshots.

use std::collections::{HashMap, HashSet};

use crate::samplers::{ProcessInfo, Samples};

/// Priority of an observation, used for sorting and rendering prominence.
///
/// Variants are ordered Low < Medium < High < Critical by declaration order
/// so that derived `Ord` matches semantic priority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObservationPriority {
    Low,
    Medium,
    High,
    Critical,
}

/// Direction of a pressure or activity trend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressureTrend {
    Emerging,
    Stable,
    Subsiding,
}

/// Direction of a recent process change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Surge,
    Drop,
    New,
    Exited,
}

/// The kind of observation — what the observation is explaining.
#[derive(Clone, Debug, PartialEq)]
pub enum ObservationKind {
    /// A process accounts for a disproportionate share of system activity.
    /// This answers "who is consuming resources?" by composite (CPU + memory) score.
    DominantProcess {
        pid: u32,
        name: String,
        cpu_pct: f32,
        mem_bytes: u64,
    },
    /// A significant change since the previous sample.
    /// This answers "what just changed?" by detecting CPU deltas, new processes,
    /// and exited processes.
    RecentChange {
        pid: u32,
        name: String,
        kind: ChangeKind,
        delta_cpu: f32,
        prev_cpu: f32,
        curr_cpu: f32,
    },
    /// CPU load trend — derived from the ratio of load_one to load_five.
    /// Emerging CPU pressure means short-term load exceeds the trailing average.
    /// Subsiding means short-term load is falling relative to the trailing average.
    CpuPressureTrend(PressureTrend),
    /// Memory usage trend — derived from the rate of change across frames.
    /// Emerging memory pressure means usage is climbing faster than 5pp/frame.
    MemoryPressureTrend(PressureTrend),
    /// Sustained network transfer detected at the system level.
    /// No per-process attribution — just "something is moving data."
    NetworkTransferDetected,
    /// Burst disk activity detected at the system level.
    /// No per-process attribution — just "something is writing or reading."
    DiskActivityDetected,
}

/// An atomic unit of explanation — a synthesized insight about system behavior.
///
/// Each observation has a kind (what it explains) and a priority (how important).
/// Observations are recomputed every frame and have no persistent identity.
#[derive(Clone, Debug)]
pub struct Observation {
    pub kind: ObservationKind,
    pub priority: ObservationPriority,
}

/// Minimal snapshot of system state needed for trend and change detection.
///
/// Stored between frames and updated on non-paused ticks. Kept intentionally
/// small — only fields the observation engine queries from the previous frame.
#[derive(Clone, Default)]
pub struct PrevState {
    pub processes: Vec<ProcessInfo>,
    pub load_one: f64,
    pub mem_used: u64,
    pub swap_used: u64,
}

impl From<&Samples> for PrevState {
    fn from(s: &Samples) -> Self {
        Self {
            processes: s.processes.clone(),
            load_one: s.load_one,
            mem_used: s.mem_used,
            swap_used: s.swap_used,
        }
    }
}

// ── Threshold constants ─────────────────────────────────────────────────────

const DOMINANT_MIN_SCORE: f64 = 0.05;
const DOMINANT_MAX_COUNT: usize = 3;

const CHANGE_MIN_DELTA: f32 = 5.0;
const CHANGE_NEW_MIN_CPU: f32 = 1.0;
const CHANGE_MAX_COUNT: usize = 3;

const NETWORK_THRESHOLD_BYTES: u64 = 10_000_000;
const DISK_THRESHOLD_BYTES: u64 = 50_000_000;

const MAX_OBSERVATIONS: usize = 9;

// ── Priority thresholds ─────────────────────────────────────────────────────

const PRIORITY_CRITICAL_SCORE: f64 = 0.8;
const PRIORITY_HIGH_SCORE: f64 = 0.4;
const PRIORITY_MEDIUM_SCORE: f64 = 0.15;

const PRIORITY_CRITICAL_DELTA: f32 = 50.0;
const PRIORITY_HIGH_DELTA: f32 = 20.0;
const PRIORITY_MEDIUM_DELTA: f32 = 10.0;

const PRIORITY_CRITICAL_LOAD: f64 = 8.0;
const PRIORITY_HIGH_LOAD: f64 = 4.0;
const PRIORITY_MEDIUM_LOAD: f64 = 2.0;

const PRIORITY_CRITICAL_MEM_PCT: f64 = 95.0;
const PRIORITY_HIGH_MEM_PCT: f64 = 85.0;
const PRIORITY_MEDIUM_MEM_PCT: f64 = 75.0;

const CPU_PRESSURE_MIN_LOAD: f64 = 1.0;
const CPU_TREND_EMERGING_RATIO: f64 = 1.5;
const CPU_TREND_SUBSIDING_RATIO: f64 = 0.67;

const MEM_PRESSURE_MIN_PCT: f64 = 60.0;
const MEM_TREND_MIN_DELTA_PP: f64 = 5.0;

// ── Public API ──────────────────────────────────────────────────────────────

/// Compute all observations from the current sample and previous system state.
///
/// Returns observations sorted by priority descending, limited to
/// [`MAX_OBSERVATIONS`]. On the first frame (empty prev processes),
/// recent-change and memory-trend detection are skipped since there is
/// no baseline. CPU-pressure trend, network, and disk activity are
/// produced from within-frame data alone.
pub fn observe(current: &Samples, prev: &PrevState) -> Vec<Observation> {
    let mut results: Vec<Observation> = Vec::new();

    observe_dominant_processes(&mut results, current);
    observe_recent_changes(&mut results, current, prev);
    observe_cpu_pressure_trend(&mut results, current);
    observe_memory_pressure_trend(&mut results, current, prev);
    observe_network_transfer(&mut results, current);
    observe_disk_activity(&mut results, current);

    results.sort_by_key(|item| std::cmp::Reverse(item.priority));
    results.truncate(MAX_OBSERVATIONS);
    results
}

// ── Observation functions ───────────────────────────────────────────────────

fn observe_dominant_processes(results: &mut Vec<Observation>, current: &Samples) {
    let max_mem = current.mem_total.max(1);
    let mut scored: Vec<(f64, &ProcessInfo)> = Vec::new();

    for p in &current.processes {
        let score = composite_score(p, max_mem);
        if !score.is_finite() || score < DOMINANT_MIN_SCORE {
            continue;
        }
        scored.push((score, p));
    }

    scored.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(DOMINANT_MAX_COUNT);

    for (score, p) in scored {
        results.push(Observation {
            kind: ObservationKind::DominantProcess {
                pid: p.pid,
                name: p.name.clone(),
                cpu_pct: p.cpu,
                mem_bytes: p.memory,
            },
            priority: dominant_priority(score),
        });
    }
}

fn observe_recent_changes(results: &mut Vec<Observation>, current: &Samples, prev: &PrevState) {
    if prev.processes.is_empty() || current.processes.is_empty() {
        return;
    }

    let prev_cpu: HashMap<u32, f32> = prev
        .processes
        .iter()
        .filter(|p| p.cpu.is_finite())
        .map(|p| (p.pid, p.cpu))
        .collect();

    let mut deltas: Vec<(f32, &ProcessInfo, f32, f32)> = Vec::new();

    for p in &current.processes {
        if !p.cpu.is_finite() {
            continue;
        }
        let prev_val = prev_cpu.get(&p.pid).copied().unwrap_or(0.0);
        let delta = p.cpu - prev_val;
        if delta.abs() >= CHANGE_MIN_DELTA {
            deltas.push((delta.abs(), p, prev_val, p.cpu));
        }
    }

    let curr_pids: HashSet<u32> = current.processes.iter().map(|p| p.pid).collect();

    for p in &prev.processes {
        if !p.cpu.is_finite() {
            continue;
        }
        if !curr_pids.contains(&p.pid) && p.cpu >= CHANGE_NEW_MIN_CPU {
            deltas.push((p.cpu, p, p.cpu, 0.0));
        }
    }

    deltas.sort_unstable_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    for (_, p, prev_cpu, curr_cpu) in deltas.into_iter().take(CHANGE_MAX_COUNT) {
        let kind = if curr_cpu == 0.0 {
            ChangeKind::Exited
        } else if prev_cpu == 0.0 {
            ChangeKind::New
        } else if curr_cpu > prev_cpu {
            ChangeKind::Surge
        } else {
            ChangeKind::Drop
        };
        results.push(Observation {
            kind: ObservationKind::RecentChange {
                pid: p.pid,
                name: p.name.clone(),
                kind,
                delta_cpu: (curr_cpu - prev_cpu).abs(),
                prev_cpu,
                curr_cpu,
            },
            priority: change_priority(curr_cpu.max(prev_cpu)),
        });
    }
}

fn observe_cpu_pressure_trend(results: &mut Vec<Observation>, current: &Samples) {
    let Some((trend, priority)) = compute_cpu_pressure_trend(current.load_one, current.load_five)
    else {
        return;
    };
    results.push(Observation {
        kind: ObservationKind::CpuPressureTrend(trend),
        priority,
    });
}

fn observe_memory_pressure_trend(
    results: &mut Vec<Observation>,
    current: &Samples,
    prev: &PrevState,
) {
    if prev.processes.is_empty() || current.mem_total == 0 {
        return;
    }

    let used_pct = current.mem_used as f64 / current.mem_total as f64 * 100.0;
    if !used_pct.is_finite() || used_pct < MEM_PRESSURE_MIN_PCT {
        return;
    }

    let prev_pct = prev.mem_used as f64 / current.mem_total as f64 * 100.0;
    if !prev_pct.is_finite() {
        return;
    }

    let delta = used_pct - prev_pct;
    let trend = if delta > MEM_TREND_MIN_DELTA_PP {
        PressureTrend::Emerging
    } else if delta < -MEM_TREND_MIN_DELTA_PP {
        PressureTrend::Subsiding
    } else {
        PressureTrend::Stable
    };

    let base = mem_pressure_priority(used_pct);
    let priority = if trend == PressureTrend::Emerging {
        bump_priority(base)
    } else {
        base
    };

    results.push(Observation {
        kind: ObservationKind::MemoryPressureTrend(trend),
        priority,
    });
}

fn push_if_activity(
    results: &mut Vec<Observation>,
    total: u64,
    threshold: u64,
    kind: ObservationKind,
) {
    if total >= threshold {
        results.push(Observation {
            kind,
            priority: ObservationPriority::Medium,
        });
    }
}

fn observe_network_transfer(results: &mut Vec<Observation>, current: &Samples) {
    push_if_activity(
        results,
        current.net_tx_rate + current.net_rx_rate,
        NETWORK_THRESHOLD_BYTES,
        ObservationKind::NetworkTransferDetected,
    );
}

fn observe_disk_activity(results: &mut Vec<Observation>, current: &Samples) {
    push_if_activity(
        results,
        current.disk_read_rate + current.disk_write_rate,
        DISK_THRESHOLD_BYTES,
        ObservationKind::DiskActivityDetected,
    );
}

// ── Scoring and priority helpers ────────────────────────────────────────────

fn composite_score(p: &ProcessInfo, max_mem: u64) -> f64 {
    if !p.cpu.is_finite() {
        return f64::NEG_INFINITY;
    }
    let cpu_norm = (p.cpu as f64) / 100.0;
    let mem_norm = (p.memory as f64) / max_mem as f64;
    cpu_norm + mem_norm
}

fn dominant_priority(score: f64) -> ObservationPriority {
    if !score.is_finite() {
        return ObservationPriority::Low;
    }
    if score >= PRIORITY_CRITICAL_SCORE {
        ObservationPriority::Critical
    } else if score >= PRIORITY_HIGH_SCORE {
        ObservationPriority::High
    } else if score >= PRIORITY_MEDIUM_SCORE {
        ObservationPriority::Medium
    } else {
        ObservationPriority::Low
    }
}

fn change_priority(max_cpu: f32) -> ObservationPriority {
    if !max_cpu.is_finite() {
        return ObservationPriority::Low;
    }
    if max_cpu >= PRIORITY_CRITICAL_DELTA {
        ObservationPriority::Critical
    } else if max_cpu >= PRIORITY_HIGH_DELTA {
        ObservationPriority::High
    } else if max_cpu >= PRIORITY_MEDIUM_DELTA {
        ObservationPriority::Medium
    } else {
        ObservationPriority::Low
    }
}

fn cpu_pressure_priority(load_one: f64) -> ObservationPriority {
    if !load_one.is_finite() {
        return ObservationPriority::Low;
    }
    if load_one >= PRIORITY_CRITICAL_LOAD {
        ObservationPriority::Critical
    } else if load_one >= PRIORITY_HIGH_LOAD {
        ObservationPriority::High
    } else if load_one >= PRIORITY_MEDIUM_LOAD {
        ObservationPriority::Medium
    } else {
        ObservationPriority::Low
    }
}

fn mem_pressure_priority(used_pct: f64) -> ObservationPriority {
    if !used_pct.is_finite() {
        return ObservationPriority::Low;
    }
    if used_pct >= PRIORITY_CRITICAL_MEM_PCT {
        ObservationPriority::Critical
    } else if used_pct >= PRIORITY_HIGH_MEM_PCT {
        ObservationPriority::High
    } else if used_pct >= PRIORITY_MEDIUM_MEM_PCT {
        ObservationPriority::Medium
    } else {
        ObservationPriority::Low
    }
}

fn compute_cpu_pressure_trend(
    load_one: f64,
    load_five: f64,
) -> Option<(PressureTrend, ObservationPriority)> {
    if !load_one.is_finite() || load_one < CPU_PRESSURE_MIN_LOAD {
        return None;
    }

    let trend = if !load_five.is_finite() || load_five < 0.01 {
        PressureTrend::Emerging
    } else {
        let ratio = load_one / load_five;
        if ratio > CPU_TREND_EMERGING_RATIO {
            PressureTrend::Emerging
        } else if ratio < CPU_TREND_SUBSIDING_RATIO {
            PressureTrend::Subsiding
        } else {
            PressureTrend::Stable
        }
    };

    let base = cpu_pressure_priority(load_one);
    let priority = if trend == PressureTrend::Emerging {
        bump_priority(base)
    } else {
        base
    };

    Some((trend, priority))
}

fn bump_priority(p: ObservationPriority) -> ObservationPriority {
    match p {
        ObservationPriority::Low => ObservationPriority::Medium,
        ObservationPriority::Medium => ObservationPriority::High,
        ObservationPriority::High | ObservationPriority::Critical => ObservationPriority::Critical,
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_proc(pid: u32, name: &str, cpu: f32, memory: u64) -> ProcessInfo {
        ProcessInfo {
            name: name.to_owned(),
            pid,
            cpu,
            memory,
            virtual_memory: 0,
            run_time: 0,
            status: "Running",
        }
    }

    fn make_samples(procs: Vec<ProcessInfo>, mem_total: u64) -> Samples {
        Samples {
            processes: procs,
            mem_total,
            ..Samples::default()
        }
    }

    fn make_prev(procs: Vec<ProcessInfo>) -> PrevState {
        PrevState {
            processes: procs,
            ..PrevState::default()
        }
    }

    // ── DominantProcess ──────────────────────────────────────────────────

    #[test]
    fn dominant_top_3_by_score() {
        let procs = vec![
            make_proc(1, "cargo", 80.0, 1_000_000_000),
            make_proc(2, "firefox", 30.0, 500_000_000),
            make_proc(3, "chrome", 20.0, 300_000_000),
            make_proc(4, "bash", 1.0, 10_000_000),
        ];
        let samples = make_samples(procs, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert_eq!(dominant.len(), 3, "top 3 dominant processes");
        if let ObservationKind::DominantProcess { pid, .. } = &dominant[0].kind {
            assert_eq!(*pid, 1, "cargo is most dominant");
        }
    }

    #[test]
    fn dominant_filters_low_score() {
        let procs = vec![
            make_proc(1, "bash", 0.5, 1_000_000),
            make_proc(2, "sshd", 0.3, 500_000),
        ];
        let samples = make_samples(procs, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert!(dominant.is_empty(), "low-score processes filtered out");
    }

    #[test]
    fn dominant_empty_processes() {
        let samples = make_samples(vec![], 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert!(dominant.is_empty());
    }

    #[test]
    fn dominant_score_boundary() {
        let max_mem = 4_000_000_000u64;
        let samples_high = make_samples(vec![make_proc(1, "p", 100.0, 2_000_000_000)], max_mem);
        let obs_high = observe(&samples_high, &PrevState::default());
        let high_scores: Vec<&Observation> = obs_high
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert!(!high_scores.is_empty(), "high score emitted");

        let score_04 = composite_score(&make_proc(1, "p", 20.0, 1_600_000_000), max_mem);
        assert!(
            (score_04 - 0.6).abs() < 0.001,
            "score ~0.6 for moderate process"
        );

        let score_004 = composite_score(&make_proc(1, "p", 0.1, 100_000_000), max_mem);
        assert!(score_004 < DOMINANT_MIN_SCORE, "low score filtered");
    }

    // ── RecentChange ─────────────────────────────────────────────────────

    #[test]
    fn change_detects_surge() {
        let prev = vec![make_proc(1, "firefox", 5.0, 100_000_000)];
        let curr = vec![make_proc(1, "firefox", 55.0, 100_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert_eq!(changes.len(), 1, "surge detected");
        if let ObservationKind::RecentChange {
            kind, delta_cpu, ..
        } = &changes[0].kind
        {
            assert_eq!(*kind, ChangeKind::Surge);
            assert!((delta_cpu - 50.0).abs() < f32::EPSILON, "delta = 50");
        }
    }

    #[test]
    fn change_detects_new_process() {
        let prev = vec![make_proc(1, "bash", 1.0, 10_000_000)];
        let curr = vec![
            make_proc(1, "bash", 1.0, 10_000_000),
            make_proc(2, "cargo", 60.0, 500_000_000),
        ];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(
            changes.iter().any(|o| matches!(
                &o.kind,
                ObservationKind::RecentChange {
                    kind: ChangeKind::New,
                    ..
                }
            )),
            "new process detected"
        );
    }

    #[test]
    fn change_detects_exited_process() {
        let prev = vec![
            make_proc(1, "bash", 1.0, 10_000_000),
            make_proc(2, "cargo", 60.0, 500_000_000),
        ];
        let curr = vec![make_proc(1, "bash", 1.0, 10_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(
            changes.iter().any(|o| matches!(
                &o.kind,
                ObservationKind::RecentChange {
                    kind: ChangeKind::Exited,
                    ..
                }
            )),
            "exited process detected"
        );
    }

    #[test]
    fn change_ignores_small_deltas() {
        let prev = vec![make_proc(1, "bash", 10.0, 10_000_000)];
        let curr = vec![make_proc(1, "bash", 12.0, 10_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(changes.is_empty(), "small delta ignored");
    }

    #[test]
    fn change_skipped_on_first_frame() {
        let curr = vec![make_proc(1, "firefox", 45.0, 500_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(changes.is_empty(), "no change detection on first frame");
    }

    #[test]
    fn change_delta_boundary() {
        let prev = vec![make_proc(1, "p", 0.0, 10_000_000)];
        let below = vec![make_proc(1, "p", 4.99, 10_000_000)];
        assert!(
            observe(
                &make_samples(below, 4_000_000_000),
                &make_prev(prev.clone())
            )
            .iter()
            .all(|o| !matches!(o.kind, ObservationKind::RecentChange { .. })),
            "delta 4.99 ignored"
        );

        let equal = vec![make_proc(1, "p", 5.0, 10_000_000)];
        assert!(
            observe(
                &make_samples(equal, 4_000_000_000),
                &make_prev(prev.clone())
            )
            .iter()
            .any(|o| matches!(o.kind, ObservationKind::RecentChange { .. })),
            "delta 5.0 detected"
        );

        let above = vec![make_proc(1, "p", 5.01, 10_000_000)];
        assert!(
            observe(&make_samples(above, 4_000_000_000), &make_prev(prev))
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::RecentChange { .. })),
            "delta 5.01 detected"
        );
    }

    #[test]
    fn change_new_cpu_boundary() {
        let prev = vec![
            make_proc(1, "p1", 0.0, 10_000_000),
            make_proc(2, "p2", 0.99, 10_000_000),
            make_proc(3, "p3", 1.0, 10_000_000),
            make_proc(4, "p4", 1.01, 10_000_000),
        ];
        let curr = vec![make_proc(1, "p1", 0.0, 10_000_000)];

        let obs = observe(&make_samples(curr, 4_000_000_000), &make_prev(prev));

        let exited: Vec<&Observation> = obs
            .iter()
            .filter(|o| {
                matches!(
                    &o.kind,
                    ObservationKind::RecentChange {
                        kind: ChangeKind::Exited,
                        ..
                    }
                )
            })
            .collect();

        assert!(
            exited
                .iter()
                .any(|o| matches!(&o.kind, ObservationKind::RecentChange { pid: 3, .. }))
        );
        assert!(
            exited
                .iter()
                .any(|o| matches!(&o.kind, ObservationKind::RecentChange { pid: 4, .. }))
        );
        assert!(
            !exited
                .iter()
                .any(|o| matches!(&o.kind, ObservationKind::RecentChange { pid: 2, .. }))
        );
    }

    // ── CpuPressureTrend ─────────────────────────────────────────────────

    #[test]
    fn cpu_pressure_emerging() {
        let samples = Samples {
            load_one: 6.0,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Emerging)
            )),
            "cpu pressure emerging (load 6 vs 2)"
        );
    }

    #[test]
    fn cpu_pressure_stable() {
        let samples = Samples {
            load_one: 3.0,
            load_five: 3.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Stable)
            )),
            "cpu pressure stable (load equal)"
        );
    }

    #[test]
    fn cpu_pressure_subsiding() {
        let samples = Samples {
            load_one: 2.0,
            load_five: 6.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Subsiding)
            )),
            "cpu pressure subsiding (load 2 vs 6)"
        );
    }

    #[test]
    fn cpu_pressure_below_threshold() {
        let samples = Samples {
            load_one: 0.5,
            load_five: 0.5,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::CpuPressureTrend(_))),
            "no cpu pressure below load_one=0.5"
        );
    }

    #[test]
    fn cpu_pressure_zero_load_five() {
        let samples = Samples {
            load_one: 5.0,
            load_five: 0.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Emerging)
            )),
            "emerging when load_five is zero"
        );
    }

    #[test]
    fn cpu_pressure_ratio_boundaries() {
        let emerging = Samples {
            load_one: 3.03,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs_e = observe(&emerging, &PrevState::default());
        assert!(
            obs_e.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Emerging)
            )),
            "ratio 1.515 is emerging"
        );

        let stable = Samples {
            load_one: 3.0,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs_s = observe(&stable, &PrevState::default());
        let stable_trends: Vec<_> = obs_s
            .iter()
            .filter_map(|o| match &o.kind {
                ObservationKind::CpuPressureTrend(t) => Some(t),
                _ => None,
            })
            .collect();
        if let Some(trend) = stable_trends.first() {
            assert_eq!(*trend, &PressureTrend::Stable, "ratio 1.5 is stable");
        }

        let subsiding = Samples {
            load_one: 1.32,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs_sub = observe(&subsiding, &PrevState::default());
        assert!(
            obs_sub.iter().any(|o| matches!(
                o.kind,
                ObservationKind::CpuPressureTrend(PressureTrend::Subsiding)
            )),
            "ratio 0.66 is subsiding"
        );
    }

    // ── MemoryPressureTrend ─────────────────────────────────────────────────

    #[test]
    fn memory_pressure_emerging() {
        let samples = Samples {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_total: 4_000_000_000,
            mem_used: 3_200_000_000,
            ..Samples::default()
        };
        let prev = PrevState {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_used: 2_400_000_000,
            ..PrevState::default()
        };
        let obs = observe(&samples, &prev);
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::MemoryPressureTrend(PressureTrend::Emerging)
            )),
            "80% and rising 20pp is emerging"
        );
    }

    #[test]
    fn memory_pressure_subsiding() {
        let samples = Samples {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_total: 4_000_000_000,
            mem_used: 2_800_000_000,
            ..Samples::default()
        };
        let prev = PrevState {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_used: 3_600_000_000,
            ..PrevState::default()
        };
        let obs = observe(&samples, &prev);
        assert!(
            obs.iter().any(|o| matches!(
                o.kind,
                ObservationKind::MemoryPressureTrend(PressureTrend::Subsiding)
            )),
            "70% and falling 20pp is subsiding"
        );
    }

    #[test]
    fn memory_pressure_below_threshold() {
        let samples = Samples {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_total: 16_000_000_000,
            mem_used: 4_000_000_000,
            ..Samples::default()
        };
        let prev = PrevState {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_used: 4_000_000_000,
            ..PrevState::default()
        };
        let obs = observe(&samples, &prev);
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::MemoryPressureTrend(_))),
            "25% used is below memory pressure threshold"
        );
    }

    #[test]
    fn memory_pressure_first_frame_skip() {
        let samples = Samples {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_total: 4_000_000_000,
            mem_used: 3_600_000_000,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::MemoryPressureTrend(_))),
            "memory pressure skipped on first frame"
        );
    }

    // ── NetworkTransferDetected ─────────────────────────────────────────

    #[test]
    fn network_transfer_detected() {
        let samples = Samples {
            processes: vec![make_proc(1, "bash", 0.0, 10_000_000)],
            net_tx_rate: 15_000_000,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .any(|o| matches!(o.kind, ObservationKind::NetworkTransferDetected)),
            "15 MB/s network detected"
        );
    }

    #[test]
    fn network_transfer_below() {
        let samples = Samples {
            processes: vec![make_proc(1, "bash", 0.0, 10_000_000)],
            net_tx_rate: 1_000_000,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::NetworkTransferDetected)),
            "1 MB/s network not detected"
        );
    }

    #[test]
    fn network_transfer_boundary() {
        let below = Samples {
            net_tx_rate: 9_999_999,
            ..Samples::default()
        };
        assert!(
            !observe(&below, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::NetworkTransferDetected)),
            "9.9 MB/s not detected"
        );

        let equal = Samples {
            net_tx_rate: 10_000_000,
            ..Samples::default()
        };
        assert!(
            observe(&equal, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::NetworkTransferDetected)),
            "10 MB/s detected"
        );

        let above = Samples {
            net_tx_rate: 10_000_001,
            ..Samples::default()
        };
        assert!(
            observe(&above, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::NetworkTransferDetected)),
            "10.1 MB/s detected"
        );
    }

    // ── DiskActivityDetected ────────────────────────────────────────────

    #[test]
    fn disk_activity_detected() {
        let samples = Samples {
            processes: vec![make_proc(1, "bash", 0.0, 10_000_000)],
            disk_write_rate: 100_000_000,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .any(|o| matches!(o.kind, ObservationKind::DiskActivityDetected)),
            "100 MB/s disk activity detected"
        );
    }

    #[test]
    fn disk_activity_below() {
        let samples = Samples {
            processes: vec![make_proc(1, "bash", 0.0, 10_000_000)],
            disk_write_rate: 5_000_000,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::DiskActivityDetected)),
            "5 MB/s disk activity not detected"
        );
    }

    #[test]
    fn disk_activity_boundary() {
        let below = Samples {
            disk_write_rate: 49_999_999,
            ..Samples::default()
        };
        assert!(
            !observe(&below, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::DiskActivityDetected)),
            "49.9 MB/s not detected"
        );

        let equal = Samples {
            disk_write_rate: 50_000_000,
            ..Samples::default()
        };
        assert!(
            observe(&equal, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::DiskActivityDetected)),
            "50 MB/s detected"
        );

        let above = Samples {
            disk_write_rate: 50_000_001,
            ..Samples::default()
        };
        assert!(
            observe(&above, &PrevState::default())
                .iter()
                .any(|o| matches!(o.kind, ObservationKind::DiskActivityDetected)),
            "50.1 MB/s detected"
        );
    }

    // ── Global ordering ──────────────────────────────────────────────────

    #[test]
    fn observations_sorted_by_priority() {
        let prev = vec![make_proc(1, "bash", 1.0, 10_000_000)];
        let curr = vec![
            make_proc(1, "bash", 1.0, 10_000_000),
            make_proc(2, "cargo", 85.0, 2_000_000_000),
            make_proc(3, "firefox", 5.0, 100_000_000),
        ];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        for window in obs.windows(2) {
            assert!(
                window[0].priority >= window[1].priority,
                "observations in priority order"
            );
        }
    }

    #[test]
    fn max_observations_enforced() {
        let prev = vec![make_proc(1, "prev", 1.0, 10_000_000)];
        let mut curr = Vec::with_capacity(20);
        for i in 0..20 {
            curr.push(make_proc(
                100 + i,
                &format!("proc-{i}"),
                50.0 + (i as f32) * 2.0,
                100_000_000 + (i as u64) * 10_000_000,
            ));
        }
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        assert!(
            obs.len() <= MAX_OBSERVATIONS,
            "capped at {MAX_OBSERVATIONS}"
        );
    }

    #[test]
    fn idle_system_returns_empty() {
        let samples = make_samples(vec![], 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        assert!(obs.is_empty(), "idle system produces no observations");
    }

    // ── Priority assignment ──────────────────────────────────────────────

    #[test]
    fn dominant_priority_assignments() {
        let max_mem = 4_000_000_000u64;

        let high_proc = make_proc(1, "heavy", 80.0, 2_000_000_000);
        let score_high = composite_score(&high_proc, max_mem);
        assert!(dominant_priority(score_high) >= ObservationPriority::High);

        let low_proc = make_proc(2, "light", 2.0, 50_000_000);
        let score_low = composite_score(&low_proc, max_mem);
        assert_eq!(dominant_priority(score_low), ObservationPriority::Low);
    }

    #[test]
    fn priority_score_boundaries() {
        let max_mem = 4_000_000_000u64;

        let p_crit = make_proc(1, "p", 80.0, 3_200_000_000);
        let s_crit = composite_score(&p_crit, max_mem);
        assert!(s_crit >= PRIORITY_CRITICAL_SCORE, "score {s_crit} >= 0.8");

        let p_high = make_proc(1, "p", 30.0, 1_200_000_000);
        let s_high = composite_score(&p_high, max_mem);
        assert!(
            (PRIORITY_HIGH_SCORE..PRIORITY_CRITICAL_SCORE).contains(&s_high),
            "score {s_high} in [0.4, 0.8)"
        );

        let p_med = make_proc(1, "p", 10.0, 400_000_000);
        let s_med = composite_score(&p_med, max_mem);
        assert!(
            (PRIORITY_MEDIUM_SCORE..PRIORITY_HIGH_SCORE).contains(&s_med),
            "score {s_med} in [0.15, 0.4)"
        );

        let p_low = make_proc(1, "p", 1.0, 20_000_000);
        let s_low = composite_score(&p_low, max_mem);
        assert!(s_low < PRIORITY_MEDIUM_SCORE, "score {s_low} < 0.15");
    }

    #[test]
    fn change_priority_assignments() {
        assert_eq!(change_priority(60.0), ObservationPriority::Critical);
        assert_eq!(change_priority(30.0), ObservationPriority::High);
        assert_eq!(change_priority(15.0), ObservationPriority::Medium);
        assert_eq!(change_priority(5.0), ObservationPriority::Low);
    }

    #[test]
    fn priority_delta_boundaries() {
        assert_eq!(
            change_priority(9.9),
            ObservationPriority::Low,
            "delta 9.9 is Low"
        );
        assert_eq!(
            change_priority(10.0),
            ObservationPriority::Medium,
            "delta 10.0 is Medium"
        );
        assert_eq!(
            change_priority(19.9),
            ObservationPriority::Medium,
            "delta 19.9 is Medium"
        );
        assert_eq!(
            change_priority(20.0),
            ObservationPriority::High,
            "delta 20.0 is High"
        );
        assert_eq!(
            change_priority(49.9),
            ObservationPriority::High,
            "delta 49.9 is High"
        );
        assert_eq!(
            change_priority(50.0),
            ObservationPriority::Critical,
            "delta 50.0 is Critical"
        );
    }

    #[test]
    fn cpu_pressure_priority_tiers() {
        assert_eq!(
            cpu_pressure_priority(9.0),
            ObservationPriority::Critical,
            "load >8 is Critical"
        );
        assert_eq!(
            cpu_pressure_priority(5.0),
            ObservationPriority::High,
            "load >4 is High"
        );
        assert_eq!(
            cpu_pressure_priority(3.0),
            ObservationPriority::Medium,
            "load >2 is Medium"
        );
        assert_eq!(
            cpu_pressure_priority(1.5),
            ObservationPriority::Low,
            "load >1 is Low"
        );
    }

    #[test]
    fn mem_pressure_priority_tiers() {
        assert_eq!(
            mem_pressure_priority(96.0),
            ObservationPriority::Critical,
            ">95% is Critical"
        );
        assert_eq!(
            mem_pressure_priority(90.0),
            ObservationPriority::High,
            ">85% is High"
        );
        assert_eq!(
            mem_pressure_priority(80.0),
            ObservationPriority::Medium,
            ">75% is Medium"
        );
        assert_eq!(
            mem_pressure_priority(65.0),
            ObservationPriority::Low,
            ">60% is Low"
        );
    }

    // ── Edge cases and NaN guards ────────────────────────────────────────

    #[test]
    fn dominant_nan_cpu_filtered() {
        let procs = vec![
            make_proc(1, "nan-proc", f32::NAN, 1_000_000_000),
            make_proc(2, "normal", 80.0, 1_000_000_000),
        ];
        let samples = make_samples(procs, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert_eq!(dominant.len(), 1, "nan process filtered out");
        if let ObservationKind::DominantProcess { pid, .. } = &dominant[0].kind {
            assert_eq!(*pid, 2, "normal process is dominant");
        }
    }

    #[test]
    fn change_nan_cpu_skipped() {
        let prev = vec![make_proc(1, "p", 5.0, 10_000_000)];
        let curr = vec![make_proc(1, "p", f32::NAN, 10_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(
            changes.is_empty(),
            "nan cpu should not produce change observation"
        );
    }

    #[test]
    fn change_inf_cpu_skipped() {
        let prev = vec![make_proc(1, "p", 5.0, 10_000_000)];
        let curr = vec![make_proc(1, "p", f32::INFINITY, 10_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(
            changes.is_empty(),
            "inf cpu should not produce change observation"
        );
    }

    #[test]
    fn cpu_pressure_nan_load_ignored() {
        let samples = Samples {
            load_one: f64::NAN,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::CpuPressureTrend(_))),
            "nan load_one should not produce cpu pressure"
        );
    }

    #[test]
    fn cpu_pressure_inf_load_ignored() {
        let samples = Samples {
            load_one: f64::INFINITY,
            load_five: 2.0,
            ..Samples::default()
        };
        let obs = observe(&samples, &PrevState::default());
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::CpuPressureTrend(_))),
            "inf load_one should not produce cpu pressure"
        );
    }

    #[test]
    fn change_prev_nan_cpu_guarded() {
        let prev = vec![make_proc(1, "p", f32::NAN, 10_000_000)];
        let curr = vec![make_proc(1, "p", 55.0, 10_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &make_prev(prev));
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(
            changes.len() == 1,
            "normal cpu in current with nan in prev should still detect"
        );
    }

    #[test]
    fn dominant_inf_cpu_filtered() {
        let procs = vec![
            make_proc(1, "inf-proc", f32::INFINITY, 1_000_000_000),
            make_proc(2, "normal", 50.0, 500_000_000),
        ];
        let samples = make_samples(procs, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert_eq!(dominant.len(), 1, "inf cpu process filtered from dominant");
        if let ObservationKind::DominantProcess { pid, .. } = &dominant[0].kind {
            assert_eq!(*pid, 2, "normal process selected");
        }
    }

    #[test]
    fn long_process_name_no_panic() {
        let long_name = "a".repeat(4096);
        let procs = vec![make_proc(1, &long_name, 90.0, 2_000_000_000)];
        let samples = make_samples(procs, 4_000_000_000);
        let obs = observe(&samples, &PrevState::default());
        assert!(!obs.is_empty(), "long name should not cause panic");
    }

    #[test]
    fn memory_pressure_nan_pct_ignored() {
        let samples = Samples {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_total: 0,
            mem_used: 0,
            ..Samples::default()
        };
        let prev = PrevState {
            processes: vec![make_proc(1, "app", 1.0, 10_000_000)],
            mem_used: 100,
            ..PrevState::default()
        };
        let obs = observe(&samples, &prev);
        assert!(
            obs.iter()
                .all(|o| !matches!(o.kind, ObservationKind::MemoryPressureTrend(_))),
            "zero mem_total should not produce memory pressure"
        );
    }

    // ── Performance and allocation notes ─────────────────────────────────

    #[test]
    #[ignore]
    fn timing_observe_scale() {
        let count = 1000;
        let procs: Vec<ProcessInfo> = (0..count)
            .map(|i| {
                make_proc(
                    i,
                    &format!("proc-{i}"),
                    (i as f32 * 0.1) % 100.0,
                    i as u64 * 1_000_000,
                )
            })
            .collect();
        let samples = make_samples(procs.clone(), 16_000_000_000);
        let prev = PrevState {
            processes: procs,
            ..PrevState::default()
        };

        let start = Instant::now();
        for _ in 0..1000 {
            let _ = observe(&samples, &prev);
        }
        let elapsed = start.elapsed();
        let per_call = elapsed / 1000;

        println!("observe() x1000 with {count} processes: {elapsed:?} ({per_call:?}/call)");
    }
}
