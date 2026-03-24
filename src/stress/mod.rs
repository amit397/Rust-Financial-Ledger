pub mod metrics;
pub mod report;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use crate::ledger::{Ledger, Transaction};
use crate::agent::Agent;
use metrics::MetricsCollector;
use report::{StressTestReport, VerificationResult, print_report, verify_invariants};

pub struct AgentConfig {
    // +Send bound is ONLY here, not on the Agent trait itself.
    // This keeps the Agent trait unchanged for single-threaded consumers (CLI).
    // Only the stress test needs Send (threads). The CLI doesn't — it's single-threaded.
    // This is the principle of least constraint.
    pub agent: Box<dyn Agent + Send>,
    pub name: String,
}

pub struct StressTestResult {
    pub report: StressTestReport,
    pub invariant_results: Vec<VerificationResult>,
    pub all_invariants_passed: bool,
}

pub struct StressTest {
    pub ledger: Arc<Mutex<Ledger>>,
    pub metrics: Arc<MetricsCollector>,
    pub agents: Vec<AgentConfig>,
    pub duration: Duration,
    pub stop_signal: Arc<AtomicBool>,
}

impl StressTest {
    pub fn new(agent_configs: Vec<AgentConfig>, duration: Duration) -> Self {
        // Create ledger with 8 accounts
        let mut ledger = Ledger::new();
        for name in &["Checking","Savings","External","Revenue","Expenses","Investments","Payroll","Escrow"] {
            ledger.create_account(name).unwrap();
        }
        // Fund accounts with initial deposits from External
        // (ValidAgent needs money to transfer; OverdraftAgent needs SOMETHING to overdraft against)
        let accounts_to_fund = [("Checking", 100_000i64), ("Savings", 50_000), ("Revenue", 200_000),
                                 ("Investments", 75_000), ("Payroll", 150_000), ("Escrow", 25_000)];
        for (account, amount_cents) in &accounts_to_fund {
            let entries = vec![
                crate::ledger::Entry { account: crate::ledger::AccountId("External".into()), amount: -amount_cents },
                crate::ledger::Entry { account: crate::ledger::AccountId(account.to_string()), amount: *amount_cents },
            ];
            let tx = Transaction::new(format!("Initial funding: {}", account), entries).unwrap();
            ledger.apply(tx).unwrap();
        }
        let agent_count = agent_configs.len();
        let agent_names: Vec<String> = agent_configs.iter().map(|c| c.name.clone()).collect();
        Self {
            ledger: Arc::new(Mutex::new(ledger)),
            metrics: Arc::new(MetricsCollector::new(agent_count, agent_names)),
            agents: agent_configs,
            duration,
            stop_signal: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn run(self) -> StressTestResult {
        // UNIMPLEMENTED — Developer task (the Arc<Mutex<T>> concurrency pattern)
        //
        // ═══ PART A: Arc<Mutex<Ledger>> — What it is and why ═══
        //
        //   You already have: self.ledger: Arc<Mutex<Ledger>>
        //
        //   ARC (Atomic Reference Count):
        //     Rust's ownership model allows only ONE owner per value. Multiple threads
        //     need to own the same Ledger. Arc solves this by heap-allocating the value
        //     and handing each thread a reference-counted handle.
        //     Arc::clone(&self.ledger) does NOT copy the Ledger data.
        //     It increments an atomic counter and returns a new handle to the SAME allocation.
        //     When the LAST Arc drops (all threads done), the Ledger is freed.
        //
        //   MUTEX:
        //     Enforces mutual exclusion — only one thread can access the inner Ledger at a time.
        //     .lock() blocks until no other thread holds the lock, then returns a MutexGuard.
        //     When the MutexGuard drops (end of scope), the lock is released automatically.
        //     Rust's type system enforces this: you CANNOT access the inner value without
        //     calling .lock(). If you try, it's a compile error — not a runtime race.
        //
        //   WHY NOT JUST PASS &mut self.ledger TO EACH THREAD?
        //     Rust doesn't allow multiple &mut references simultaneously — that's the whole
        //     point of the borrow checker. move || { } closures require owned values, not
        //     references that might outlive the spawner. Arc is the ownership solution.
        //     Try removing the Arc::clone and passing self.ledger directly — the error
        //     message will teach you more than this comment.
        //
        // ═══ PART B: std::thread::spawn vs tokio ═══
        //
        //   Chaos agents are CPU-bound: RNG + arithmetic + mutex acquisition.
        //   tokio is for I/O-bound work (waiting on network, disk, timers).
        //   CPU-bound tasks in tokio block the entire async executor thread pool —
        //   all other tasks stall. OS threads give each agent a real CPU core.
        //   True parallelism = more realistic stress = better demo numbers.
        //
        // ═══ PART C: AtomicBool stop signal vs Mutex<bool> ═══
        //
        //   Every loop iteration of every thread checks stop_signal.load(Ordering::Relaxed).
        //   Mutex<bool> would require a lock acquisition per iteration — thousands/sec per thread.
        //   AtomicBool.load compiles to a single MOV instruction (read from cache line).
        //   Zero OS involvement, zero contention. The right tool for a simple flag.
        //
        // ═══ IMPLEMENTATION OUTLINE ═══
        //
        //   STEP 1 — Register Ctrl+C handler:
        //     let stop_for_ctrlc = Arc::clone(&self.stop_signal);
        //     ctrlc::set_handler(move || {
        //         stop_for_ctrlc.store(true, Ordering::Relaxed);
        //         eprintln!("\n⏹  Ctrl+C — stopping agents and printing report...");
        //     }).expect("Failed to register Ctrl+C handler");
        //
        //   STEP 2 — Spawn one OS thread per agent:
        //     let mut handles = Vec::new();
        //     for (agent_id, config) in self.agents.into_iter().enumerate() {
        //         let ledger  = Arc::clone(&self.ledger);   // O(1) — just increments counter
        //         let metrics = Arc::clone(&self.metrics);
        //         let stop    = Arc::clone(&self.stop_signal);
        //         let handle  = std::thread::spawn(move || {
        //             agent_thread(ledger, config.agent, metrics, stop, agent_id);
        //         });
        //         handles.push(handle);
        //     }
        //
        //   STEP 3 — Wait for duration, then signal stop:
        //     std::thread::sleep(self.duration);
        //     self.stop_signal.store(true, Ordering::Relaxed);
        //
        //   STEP 4 — Join all threads (wait for in-flight proposals to complete):
        //     for h in handles { h.join().ok(); }
        //     // join() BEFORE computing the report — threads may still be writing metrics
        //
        //   STEP 5 — Run invariant checks and compile report:
        //     let ledger_guard = self.ledger.lock().unwrap_or_else(|p| p.into_inner());
        //     let invariants = verify_invariants(&ledger_guard, &self.metrics);
        //     let report = self.metrics.compute_report();
        //     print_report(&report, &invariants);
        //     let passed = invariants.iter().all(|r| matches!(r, VerificationResult::Pass(_)));
        //     StressTestResult { report, invariant_results: invariants, all_invariants_passed: passed }
        //
        // ═══ QUESTIONS TO ANSWER AS COMMENTS BEFORE IMPLEMENTING ═══
        //   Q1: Why Arc::clone instead of moving self.ledger into the first thread?
        //   Q2: Why join() threads BEFORE reading metrics, not after printing the report?
        //   Q3: What state is a proposal in when stop_signal fires mid-execution?
        //       Is the ledger left in a consistent state? Why?
        todo!("StressTest::run — implement Arc::clone thread spawning (read all 3 parts above)")
    }
}

fn agent_thread(
    _ledger: Arc<Mutex<Ledger>>,
    _agent: Box<dyn Agent + Send>,
    _metrics: Arc<MetricsCollector>,
    _stop_signal: Arc<AtomicBool>,
    _agent_id: usize,
) {
    // UNIMPLEMENTED — Developer task (catch_unwind + mutex poison recovery)
    //
    // ═══ WHAT IS MUTEX POISONING? ═══
    //
    //   When a thread panics while HOLDING a Mutex lock, Rust marks the Mutex as
    //   "poisoned." All subsequent .lock() calls return Err(PoisonError<T>) instead
    //   of Ok(MutexGuard<T>). This is Rust's defensive signal: the guarded data MAY
    //   be in an inconsistent state because a thread panicked mid-mutation.
    //
    //   In most systems: treat poisoning as fatal. Here: we can safely recover.
    //
    // ═══ WHY SAFE TO RECOVER HERE ═══
    //
    //   Ledger::apply has two phases (you implemented this in Phase 1):
    //     Phase 1: Validation — read-only, NO mutation.
    //     Phase 2: Commit — mutation, but all operations are infallible (no panics possible).
    //   A panic in Phase 1 leaves the Ledger UNMODIFIED. The data is consistent.
    //   A panic in Phase 2 is impossible by construction.
    //   Therefore: .into_inner() on a PoisonError is safe here — the Ledger is valid.
    //
    //   This is WHY you designed Ledger::apply with two phases in Phase 1. It wasn't
    //   just about rollback simplicity — it was to make Phase 4 poison recovery provably safe.
    //
    // ═══ WHY catch_unwind AROUND EACH PROPOSAL? ═══
    //
    //   OverflowAgent sends amounts near i64::MAX. Some code path might panic
    //   (unwrap, index out of bounds, etc.). Without catch_unwind, that panic propagates
    //   up the thread stack, poisons the Mutex, and every subsequent .lock() fails.
    //   One bad proposal from one agent kills the entire stress test.
    //
    //   catch_unwind intercepts the panic before it reaches the thread boundary.
    //   We record a panic counter, continue the loop. One broken proposal = one metric
    //   increment, not a cascade failure.
    //
    // ═══ WHY AssertUnwindSafe? ═══
    //
    //   catch_unwind requires its closure to be UnwindSafe. Rust marks types with
    //   interior mutability (Arc<Mutex<T>>, RefCell<T>) as NOT UnwindSafe by default,
    //   because catching a panic that mutated them could leave them inconsistent.
    //   AssertUnwindSafe is our explicit assertion: "We've verified the invariants hold
    //   even after a panic" — and we have (see the two-phase proof above).
    //
    // ═══ BEFORE IMPLEMENTING: write this test first ═══
    //
    //   In #[cfg(test)] below, implement test_poison_recovery:
    //   1. Create Arc<Mutex<i32>> with value 42
    //   2. Spawn a thread: lock it, then panic!()
    //   3. join() that thread (it will return Err)
    //   4. On main thread: .lock().unwrap_or_else(|p| p.into_inner())
    //   5. Assert the value is still 42 (panic didn't corrupt it)
    //   Run this test. Understand it. Then implement agent_thread.
    //
    // ═══ IMPLEMENTATION OUTLINE ═══
    //
    //   while !stop_signal.load(Ordering::Relaxed) {
    //       let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    //           let start = std::time::Instant::now();
    //
    //           match agent.propose("") {
    //               Ok(proposal) => {
    //                   match Transaction::new(proposal.description, proposal.entries) {
    //                       Ok(tx) => {
    //                           let lock_start = std::time::Instant::now();
    //                           let mut guard = ledger.lock()
    //                               .unwrap_or_else(|p| p.into_inner()); // poison recovery
    //                           metrics.record_lock_wait(agent_id, lock_start.elapsed());
    //                           match guard.apply(tx) {
    //                               Ok(()) => metrics.record_commit(agent_id),
    //                               Err(e) => metrics.record_rejection(agent_id, &e),
    //                           }
    //                       }
    //                       Err(e) => metrics.record_construction_failure(agent_id, &e),
    //                   }
    //               }
    //               Err(e) => metrics.record_parse_failure(agent_id, &e),
    //           }
    //
    //           metrics.record_latency(agent_id, start.elapsed());
    //           metrics.increment_total(agent_id);
    //       }));
    //
    //       if result.is_err() {
    //           metrics.record_panic(agent_id);
    //       }
    //   }
    todo!("agent_thread — implement catch_unwind + poison recovery loop (read all sections above)")
}

#[cfg(test)]
mod tests {
    // IMPLEMENT THIS FIRST before agent_thread:
    // test_poison_recovery — described in the agent_thread guide above
    //
    // test_stress_completes — 2-second stress test with 2 agents, assert no panic
}
