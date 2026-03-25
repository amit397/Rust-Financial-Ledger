use std::time::Duration;
use ledger_guard::{agent::chaos::*, stress::{StressTest, AgentConfig}};
use ledger_guard::stress::report::VerificationResult;

#[test]
fn stress_test_3_seconds_all_invariants_pass() {
    let accounts = vec!["Checking","Savings","External","Revenue","Expenses","Investments","Payroll","Escrow"]
        .into_iter().map(String::from).collect::<Vec<_>>();

    let configs = vec![
        AgentConfig { agent: Box::new(ValidAgent::new(accounts.clone())),    name: "ValidAgent".into() },
        AgentConfig { agent: Box::new(OverdraftAgent::new(accounts.clone())), name: "OverdraftAgent".into() },
        AgentConfig { agent: Box::new(ChaosAgent::new(accounts.clone())),    name: "ChaosAgent".into() },
    ];

    let result = StressTest::new(configs, Duration::from_secs(3)).run();

    // At least some proposals were processed
    assert!(result.report.total_proposals > 0,
            "No proposals processed — stress test likely panicked on startup");

    // Accounting identity: committed + rejected = total (minus any in-flight at shutdown)
    let accounted = result.report.total_committed + result.report.total_rejected;
    assert!(accounted <= result.report.total_proposals,
            "More accounted than proposed: committed+rejected > total");
    assert!(result.report.total_proposals <= accounted + 3,
            "Too many unaccounted proposals (more than agent count)");

    // All invariants must pass
    assert!(result.all_invariants_passed,
            "Invariant failure: {:?}", result.invariant_results);

    // ValidAgent should commit at a high rate (>80%)
    let valid_agent = result.report.per_agent.iter().find(|a| a.name == "ValidAgent").unwrap();
    if valid_agent.proposed > 100 {
        let commit_rate = valid_agent.committed as f64 / valid_agent.proposed as f64;
        assert!(commit_rate > 0.8,
                "ValidAgent commit rate too low: {:.1}%", commit_rate * 100.0);
    }

    // NOTE: OverdraftAgent commit rate is NOT asserted here — in concurrent tests,
    // ValidAgent deposits inflate balances above overdraft thresholds. Correctness
    // is proven by the 4 invariant checks above (non-negativity, conservation,
    // count consistency, replay).
}

#[test]
fn stress_test_zero_data_races() {
    let accounts = vec!["Checking","Savings","External"]
        .into_iter().map(String::from).collect::<Vec<_>>();
    let configs = vec![
        AgentConfig { agent: Box::new(ValidAgent::new(accounts.clone())), name: "V1".into() },
        AgentConfig { agent: Box::new(ValidAgent::new(accounts.clone())), name: "V2".into() },
    ];
    let result = StressTest::new(configs, Duration::from_secs(1)).run();
    assert!(result.all_invariants_passed);
}
