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
       // 1. Set up Ctrl+C handler to allow early exit
    let stop_for_ctrlc = Arc::clone(&self.stop_signal);
    ctrlc::set_handler(move || {
        stop_for_ctrlc.store(true, Ordering::Relaxed);
        eprintln!("\n⏹  Ctrl+C — stopping agents and printing report...");
    }).ok(); // .ok() — may fail if a handler is already registered (e.g. multiple tests)

    // 2. Spawn agent threads using Arc handles
    let mut handles = Vec::new();
    for (agent_id, config) in self.agents.into_iter().enumerate() {
        let ledger  = Arc::clone(&self.ledger);
        let metrics = Arc::clone(&self.metrics);
        let stop    = Arc::clone(&self.stop_signal);
        
        let handle  = std::thread::spawn(move || {
            agent_thread(ledger, config.agent, metrics, stop, agent_id);
        });
        handles.push(handle);
    }

    // 3. Let the test run for the specified duration
    std::thread::sleep(self.duration);
    
    // 4. Signal all threads to stop and wait for them to finish
    self.stop_signal.store(true, Ordering::Relaxed);
    for h in handles {
        h.join().ok(); // Join acts as a memory barrier for metrics
    }

    // 5. Finalize the ledger and print the results
    let ledger_guard = self.ledger.lock().unwrap_or_else(|p| p.into_inner());
    let invariants = verify_invariants(&ledger_guard, &self.metrics);
    let report = self.metrics.compute_report();
    
    print_report(&report, &invariants);
    
    let passed = invariants.iter().all(|r| matches!(r, VerificationResult::Pass(_)));
    StressTestResult { report, invariant_results: invariants, all_invariants_passed: passed }
}
}

fn agent_thread(
    ledger: Arc<Mutex<Ledger>>,
    agent: Box<dyn Agent + Send>,
    metrics: Arc<MetricsCollector>,
    stop_signal: Arc<AtomicBool>,
    agent_id: usize,
) {
    while !stop_signal.load(Ordering::Relaxed) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let start = std::time::Instant::now();

            match agent.propose("") {
                Ok(proposal) => {
                    match Transaction::new(proposal.description, proposal.entries) {
                        Ok(tx) => {
                            let lock_start = std::time::Instant::now();
                            let mut guard = ledger.lock()
                                .unwrap_or_else(|p| p.into_inner());

                            metrics.record_lock_wait(agent_id, lock_start.elapsed());

                            match guard.apply(tx) {
                                Ok(()) => metrics.record_commit(agent_id),
                                Err(e) => metrics.record_rejection(agent_id, &e),
                            }
                        }
                        Err(e) => metrics.record_construction_failure(agent_id, &e),
                    }
                }
                Err(e) => metrics.record_parse_failure(agent_id, &e),
            }

            metrics.record_latency(agent_id, start.elapsed());
            metrics.increment_total(agent_id);
        }));

        if result.is_err() {
            metrics.record_panic(agent_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_poison_recovery() {
        let data = Arc::new(Mutex::new(42));
        let data_clone = Arc::clone(&data);

        let handle = std::thread::spawn(move || {
            let _guard = data_clone.lock().unwrap();
            panic!("intentional panic while holding lock");
        });

        let _ = handle.join(); // thread panicked — Mutex is now poisoned

        // Recover via into_inner — data must still be intact
        let guard = data.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(*guard, 42, "Mutex data corrupted after poison recovery");
    }

    #[test]
    fn test_stress_completes() {
        let accounts: Vec<String> = vec!["Checking","Savings","External","Revenue","Expenses","Investments","Payroll","Escrow"]
            .into_iter().map(String::from).collect();

        let configs = vec![
            AgentConfig {
                agent: Box::new(crate::agent::chaos::ValidAgent::new(accounts.clone())),
                name: "ValidAgent".into(),
            },
            AgentConfig {
                agent: Box::new(crate::agent::chaos::ChaosAgent::new(accounts.clone())),
                name: "ChaosAgent".into(),
            },
        ];

        let result = StressTest::new(configs, Duration::from_secs(2)).run();
        assert!(result.report.total_proposals > 0, "No proposals processed");
        assert!(result.all_invariants_passed, "Invariants failed: {:?}", result.invariant_results);
    }
}
