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
}

/// Direction of a recent change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Surge,
    Drop,
    New,
    Exited,
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

// ── Threshold constants ─────────────────────────────────────────────────────

/// Dominant-process composite score below which the observation is suppressed.
const DOMINANT_MIN_SCORE: f64 = 0.05;
/// Number of dominant-process observations to emit at most.
const DOMINANT_MAX_COUNT: usize = 3;

/// Minimum absolute CPU delta (percentage points) to trigger a RecentChange.
const CHANGE_MIN_DELTA: f32 = 5.0;
/// Minimum CPU of a new process to emit a RecentChange::New observation.
const CHANGE_NEW_MIN_CPU: f32 = 1.0;
/// Number of recent-change observations to emit at most.
const CHANGE_MAX_COUNT: usize = 3;

/// Maximum total observations returned by [`observe()`].
const MAX_OBSERVATIONS: usize = 9;

// ── Priority thresholds ─────────────────────────────────────────────────────

const PRIORITY_CRITICAL_SCORE: f64 = 0.8;
const PRIORITY_HIGH_SCORE: f64 = 0.4;
const PRIORITY_MEDIUM_SCORE: f64 = 0.15;

const PRIORITY_CRITICAL_DELTA: f32 = 50.0;
const PRIORITY_HIGH_DELTA: f32 = 20.0;
const PRIORITY_MEDIUM_DELTA: f32 = 10.0;

// ── Public API ──────────────────────────────────────────────────────────────

/// Compute all observations from the current sample and previous process data.
///
/// Returns observations sorted by priority descending, limited to
/// [`MAX_OBSERVATIONS`]. On the first frame (empty `prev_processes`),
/// change detection is skipped since there is no baseline.
pub fn observe(current: &Samples, prev_processes: &[ProcessInfo]) -> Vec<Observation> {
    let mut results: Vec<Observation> = Vec::with_capacity(MAX_OBSERVATIONS);

    observe_dominant_processes(&mut results, current);
    observe_recent_changes(&mut results, current, prev_processes);

    results.sort_by_key(|item| std::cmp::Reverse(item.priority));
    results.truncate(MAX_OBSERVATIONS);
    results
}

// ── Observation functions ───────────────────────────────────────────────────

fn observe_dominant_processes(results: &mut Vec<Observation>, current: &Samples) {
    let max_mem = current.mem_total.max(1);
    let mut procs: Vec<&ProcessInfo> = current.processes.iter().collect();
    procs.sort_unstable_by(|a, b| {
        let score_a = composite_score(a, max_mem);
        let score_b = composite_score(b, max_mem);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for p in procs.iter().take(DOMINANT_MAX_COUNT) {
        let score = composite_score(p, max_mem);
        if score < DOMINANT_MIN_SCORE {
            break;
        }
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

fn observe_recent_changes(results: &mut Vec<Observation>, current: &Samples, prev: &[ProcessInfo]) {
    // Skip change detection on the first frame when there is no baseline.
    if prev.is_empty() || current.processes.is_empty() {
        return;
    }

    let prev_cpu: HashMap<u32, f32> = prev.iter().map(|p| (p.pid, p.cpu)).collect();
    let mut deltas: Vec<(f32, &ProcessInfo, f32, f32)> = Vec::new();

    for p in &current.processes {
        let prev_val = prev_cpu.get(&p.pid).copied().unwrap_or(0.0);
        let delta = p.cpu - prev_val;
        if delta.abs() >= CHANGE_MIN_DELTA {
            deltas.push((delta.abs(), p, prev_val, p.cpu));
        }
    }

    let curr_pids: HashSet<u32> = current.processes.iter().map(|p| p.pid).collect();
    for p in prev {
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

// ── Scoring helpers ─────────────────────────────────────────────────────────

/// Composite score (0.0–~2.0) combining CPU and memory usage.
fn composite_score(p: &ProcessInfo, max_mem: u64) -> f64 {
    let cpu_norm = (p.cpu as f64) / 100.0;
    let mem_norm = (p.memory as f64) / max_mem as f64;
    cpu_norm + mem_norm
}

fn dominant_priority(score: f64) -> ObservationPriority {
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

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let obs = observe(&samples, &[]);
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
        let obs = observe(&samples, &[]);
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert!(dominant.is_empty(), "low-score processes filtered out");
    }

    #[test]
    fn dominant_empty_processes() {
        let samples = make_samples(vec![], 4_000_000_000);
        let obs = observe(&samples, &[]);
        let dominant: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::DominantProcess { .. }))
            .collect();
        assert!(dominant.is_empty());
    }

    // ── RecentChange ─────────────────────────────────────────────────────

    #[test]
    fn change_detects_surge() {
        let prev = vec![make_proc(1, "firefox", 5.0, 100_000_000)];
        let curr = vec![make_proc(1, "firefox", 55.0, 100_000_000)];
        let samples = make_samples(curr, 4_000_000_000);
        let obs = observe(&samples, &prev);
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
        let obs = observe(&samples, &prev);
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
        let obs = observe(&samples, &prev);
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
        let obs = observe(&samples, &prev);
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
        let obs = observe(&samples, &[]);
        let changes: Vec<&Observation> = obs
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::RecentChange { .. }))
            .collect();
        assert!(changes.is_empty(), "no change detection on first frame");
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
        let obs = observe(&samples, &prev);
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
        let obs = observe(&samples, &prev);
        assert!(
            obs.len() <= MAX_OBSERVATIONS,
            "capped at {MAX_OBSERVATIONS}"
        );
    }

    #[test]
    fn idle_system_returns_empty() {
        let samples = make_samples(vec![], 4_000_000_000);
        let obs = observe(&samples, &[]);
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
    fn change_priority_assignments() {
        assert_eq!(change_priority(60.0), ObservationPriority::Critical);
        assert_eq!(change_priority(30.0), ObservationPriority::High);
        assert_eq!(change_priority(15.0), ObservationPriority::Medium);
        assert_eq!(change_priority(5.0), ObservationPriority::Low);
    }
}
