use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use crate::ledger::LedgerError;
use crate::agent::AgentError;
use super::report::{StressTestReport, PerAgentReport};

pub const RESERVOIR_CAP: usize = 10_000;

pub struct AgentAtomicMetrics {
    pub proposed: AtomicU64,
    pub committed: AtomicU64,
    pub rejected: AtomicU64,
    pub panics: AtomicU64,
}

impl Default for AgentAtomicMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentAtomicMetrics {
    pub fn new() -> Self {
        Self {
            proposed: AtomicU64::new(0),
            committed: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
            panics: AtomicU64::new(0),
        }
    }
}

pub struct MetricsCollector {
    pub total_proposals: AtomicU64,
    pub total_committed: AtomicU64,
    pub total_rejected: AtomicU64,
    pub rejected_insufficient: AtomicU64,
    pub rejected_not_found: AtomicU64,
    pub rejected_unbalanced: AtomicU64,
    pub rejected_overflow: AtomicU64,
    pub rejected_construction: AtomicU64,
    pub rejected_parse: AtomicU64,
    pub total_panics: AtomicU64,
    pub per_agent: Vec<AgentAtomicMetrics>,
    pub latencies: Mutex<Vec<Duration>>,
    pub latency_count: AtomicU64,
    pub lock_wait_ns: Vec<AtomicU64>,
    pub start_time: Instant,
    pub agent_names: Vec<String>,
}

impl MetricsCollector {
    pub fn new(agent_count: usize, agent_names: Vec<String>) -> Self {
        let mut per_agent = Vec::new();
        let mut lock_wait_ns = Vec::new();
        for _ in 0..agent_count {
            per_agent.push(AgentAtomicMetrics::new());
            lock_wait_ns.push(AtomicU64::new(0));
        }
        
        Self {
            total_proposals: AtomicU64::new(0),
            total_committed: AtomicU64::new(0),
            total_rejected: AtomicU64::new(0),
            rejected_insufficient: AtomicU64::new(0),
            rejected_not_found: AtomicU64::new(0),
            rejected_unbalanced: AtomicU64::new(0),
            rejected_overflow: AtomicU64::new(0),
            rejected_construction: AtomicU64::new(0),
            rejected_parse: AtomicU64::new(0),
            total_panics: AtomicU64::new(0),
            per_agent,
            latencies: Mutex::new(Vec::with_capacity(RESERVOIR_CAP)),
            latency_count: AtomicU64::new(0),
            lock_wait_ns,
            start_time: Instant::now(),
            agent_names,
        }
    }

    pub fn record_commit(&self, agent_id: usize) {
        self.total_committed.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].committed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_rejection(&self, agent_id: usize, err: &LedgerError) {
        self.total_rejected.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].rejected.fetch_add(1, Ordering::Relaxed);
        match err {
            LedgerError::InsufficientFunds { .. } => { self.rejected_insufficient.fetch_add(1, Ordering::Relaxed); }
            LedgerError::AccountNotFound { .. } => { self.rejected_not_found.fetch_add(1, Ordering::Relaxed); }
            LedgerError::Unbalanced { .. } => { self.rejected_unbalanced.fetch_add(1, Ordering::Relaxed); }
            LedgerError::Overflow => { self.rejected_overflow.fetch_add(1, Ordering::Relaxed); }
            LedgerError::InvalidAmount | LedgerError::DuplicateAccount { .. } => { self.rejected_construction.fetch_add(1, Ordering::Relaxed); }
        }
    }

    pub fn record_construction_failure(&self, agent_id: usize, _err: &LedgerError) {
        self.total_rejected.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].rejected.fetch_add(1, Ordering::Relaxed);
        self.rejected_construction.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_parse_failure(&self, agent_id: usize, _err: &AgentError) {
        self.total_rejected.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].rejected.fetch_add(1, Ordering::Relaxed);
        self.rejected_parse.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_panic(&self, agent_id: usize) {
        self.total_panics.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].panics.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_total(&self, agent_id: usize) {
        self.total_proposals.fetch_add(1, Ordering::Relaxed);
        self.per_agent[agent_id].proposed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn total_committed(&self) -> u64 {
        self.total_committed.load(Ordering::Relaxed)
    }

    pub fn compute_report(&self) -> StressTestReport {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let total_proposals = self.total_proposals.load(Ordering::Relaxed);
        let throughput_per_sec = total_proposals as f64 / elapsed;

        let mut p50 = None;
        let mut p95 = None;
        let mut p99 = None;
        
        let mut sorted = self.latencies.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if !sorted.is_empty() {
            sorted.sort_unstable();
            let idx = |pct: f64| ((sorted.len() as f64 * pct) as usize).min(sorted.len() - 1);
            p50 = Some(sorted[idx(0.5)].as_secs_f64() * 1000.0);
            p95 = Some(sorted[idx(0.95)].as_secs_f64() * 1000.0);
            p99 = Some(sorted[idx(0.99)].as_secs_f64() * 1000.0);
        }

        let mut sum_wait = 0;
        for wait in &self.lock_wait_ns {
            sum_wait += wait.load(Ordering::Relaxed);
        }
        let elapsed_ns = self.start_time.elapsed().as_nanos() as f64;
        let contention_pct = (sum_wait as f64) / (elapsed_ns * self.per_agent.len() as f64) * 100.0;

        let mut per_agent_report = Vec::new();
        for (i, agent) in self.per_agent.iter().enumerate() {
            per_agent_report.push(PerAgentReport {
                name: self.agent_names[i].clone(),
                proposed: agent.proposed.load(Ordering::Relaxed),
                committed: agent.committed.load(Ordering::Relaxed),
                rejected: agent.rejected.load(Ordering::Relaxed),
            });
        }

        StressTestReport {
            duration_secs: elapsed,
            agent_count: self.per_agent.len(),
            agent_names: self.agent_names.clone(),
            total_proposals,
            total_committed: self.total_committed.load(Ordering::Relaxed),
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            rejected_insufficient: self.rejected_insufficient.load(Ordering::Relaxed),
            rejected_not_found: self.rejected_not_found.load(Ordering::Relaxed),
            rejected_unbalanced: self.rejected_unbalanced.load(Ordering::Relaxed),
            rejected_overflow: self.rejected_overflow.load(Ordering::Relaxed),
            rejected_construction: self.rejected_construction.load(Ordering::Relaxed),
            rejected_parse: self.rejected_parse.load(Ordering::Relaxed),
            total_panics: self.total_panics.load(Ordering::Relaxed),
            throughput_per_sec,
            p50_latency_ms: p50,
            p95_latency_ms: p95,
            p99_latency_ms: p99,
            contention_pct,
            per_agent: per_agent_report,
        }
    }

    pub fn record_lock_wait(&self, agent_id: usize, duration: Duration) {
        self.lock_wait_ns[agent_id].fetch_add(
            duration.as_nanos() as u64,
            Ordering::Relaxed,
        );
    }

    pub fn record_latency(&self, _agent_id: usize, duration: Duration) {
        // UNIMPLEMENTED — Developer task (Algorithm R reservoir sampling)
        //
        // THE PROBLEM WITH Vec::push ON EVERY CALL:
        //   At 5,000 proposals/sec × 10 agents × 30 seconds = 1,500,000 Duration values.
        //   That's ~18 MB of data and the Mutex would be contended on every single call.
        //   At high throughput, lock contention on the latency vec would become the
        //   bottleneck — you'd be spending more time recording metrics than doing work.
        //
        // THE PROBLEM WITH SYSTEMATIC SAMPLING (every Nth):
        //   If you record every 100th latency, you get unbiased results ONLY if the
        //   workload has no periodicity. But OS schedulers give threads CPU time in
        //   bursts. Lock contention waves are periodic. GC pauses are periodic.
        //   If the burst size ≈ N, you always sample the same phase — always the fast
        //   part or always the slow part. Your p95 could be wrong by 3-5x.
        //   This is "aliasing" — your sampling frequency matches the signal frequency.
        //
        // THE SOLUTION: Algorithm R (Vitter, 1985)
        //   Maintain a fixed-size reservoir of K samples (K = RESERVOIR_CAP = 10,000).
        //   After seeing N total observations:
        //     - If N < K: always add to the reservoir (not full yet)
        //     - If N >= K: include observation with probability K/N, by picking
        //       a random index j in [0, N-1]. If j < K, replace reservoir[j].
        //
        // PROOF OF UNBIASEDNESS:
        //   We want: after N observations, each has probability K/N of being in reservoir.
        //   Proof by induction:
        //     Base (N=K): all K observations included. P = K/K = 1. ✓
        //     Step (N→N+1): new observation included with prob K/(N+1). ✓
        //       Each existing observation j survives if: new observation NOT included,
        //       OR included but replaces a different slot.
        //       P(survive) = 1 - K/(N+1) + K/(N+1) × (K-1)/K = N/(N+1)
        //       So P(j in reservoir after N+1) = K/N × N/(N+1) = K/(N+1). ✓
        //   Result: every observation has EQUAL probability of being in the final reservoir,
        //   regardless of WHEN it arrived. First or last — same probability. Unbiased.
        //
        // PERFORMANCE:
        //   Once reservoir is full (~10,000 observations), most calls skip the lock entirely
        //   (probability 1 - K/N approaches 1 as N grows). Near-zero overhead at high load.
        //
        // IMPLEMENTATION:
        //   let count = self.latency_count.fetch_add(1, Ordering::Relaxed);
        //   // count = number of latencies seen BEFORE this one (0-indexed)
        //
        //   if (count as usize) < RESERVOIR_CAP {
        //       // Reservoir not full — always add
        //       self.latencies.lock().unwrap_or_else(|p| p.into_inner()).push(duration);
        //   } else {
        //       // Reservoir full — replace with probability RESERVOIR_CAP / (count + 1)
        //       let j = rand::random::<u64>() % (count + 1);
        //       if (j as usize) < RESERVOIR_CAP {
        //           self.latencies.lock().unwrap_or_else(|p| p.into_inner())[j as usize] = duration;
        //       }
        //       // else: do nothing — no lock acquired, zero cost
        //   }
        let count = self.latency_count.fetch_add(1, Ordering::Relaxed);
        if (count as usize) < RESERVOIR_CAP {
            self.latencies.lock().unwrap_or_else(|p| p.into_inner()).push(duration);
        } else {
            let j = rand::random::<u64>() % (count + 1);

            if (j as usize) < RESERVOIR_CAP {
                self.latencies.lock().unwrap_or_else(|p| p.into_inner())[j as usize] = duration;
            }
        }

    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit_increments() {
        let mc = MetricsCollector::new(1, vec!["A".into()]);
        for _ in 0..1000 {
            mc.record_commit(0);
        }
        assert_eq!(mc.total_committed.load(Ordering::Relaxed), 1000);
        assert_eq!(mc.per_agent[0].committed.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn test_rejection_breakdown() {
        let mc = MetricsCollector::new(1, vec!["A".into()]);

        mc.record_rejection(0, &LedgerError::InsufficientFunds {
            account: "X".into(), available: 0, requested: 1,
        });
        mc.record_rejection(0, &LedgerError::AccountNotFound { name: "X".into() });
        mc.record_rejection(0, &LedgerError::Unbalanced { actual_sum: 1 });
        mc.record_rejection(0, &LedgerError::Overflow);

        assert_eq!(mc.rejected_insufficient.load(Ordering::Relaxed), 1);
        assert_eq!(mc.rejected_not_found.load(Ordering::Relaxed), 1);
        assert_eq!(mc.rejected_unbalanced.load(Ordering::Relaxed), 1);
        assert_eq!(mc.rejected_overflow.load(Ordering::Relaxed), 1);
        assert_eq!(mc.total_rejected.load(Ordering::Relaxed), 4);
    }

    #[test]
    fn test_reservoir_cap() {
        let mc = MetricsCollector::new(1, vec!["A".into()]);
        for _ in 0..20_000 {
            mc.record_latency(0, Duration::from_micros(100));
        }
        let latencies = mc.latencies.lock().unwrap();
        assert_eq!(latencies.len(), RESERVOIR_CAP);
    }

    #[test]
    fn test_per_agent_isolation() {
        let mc = MetricsCollector::new(2, vec!["A".into(), "B".into()]);

        for _ in 0..50 {
            mc.record_commit(0);
            mc.increment_total(0);
        }
        for _ in 0..30 {
            mc.record_commit(1);
            mc.increment_total(1);
        }

        assert_eq!(mc.per_agent[0].committed.load(Ordering::Relaxed), 50);
        assert_eq!(mc.per_agent[0].proposed.load(Ordering::Relaxed), 50);
        assert_eq!(mc.per_agent[1].committed.load(Ordering::Relaxed), 30);
        assert_eq!(mc.per_agent[1].proposed.load(Ordering::Relaxed), 30);
    }
}

