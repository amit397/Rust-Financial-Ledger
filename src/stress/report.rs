use crate::ledger::Ledger;
use super::metrics::MetricsCollector;

#[derive(Debug)]
pub enum VerificationResult {
    Pass(String),
    Fail(String),
}

pub struct StressTestReport {
    pub duration_secs: f64,
    pub agent_count: usize,
    pub agent_names: Vec<String>,
    pub total_proposals: u64,
    pub total_committed: u64,
    pub total_rejected: u64,
    pub rejected_insufficient: u64,
    pub rejected_not_found: u64,
    pub rejected_unbalanced: u64,
    pub rejected_overflow: u64,
    pub rejected_construction: u64,
    pub rejected_parse: u64,
    pub total_panics: u64,
    pub throughput_per_sec: f64,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub p99_latency_ms: Option<f64>,
    pub contention_pct: f64,
    pub per_agent: Vec<PerAgentReport>,
}

pub struct PerAgentReport {
    pub name: String,
    pub proposed: u64,
    pub committed: u64,
    pub rejected: u64,
}

pub fn print_report(report: &StressTestReport, invariants: &[VerificationResult]) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║           LedgerGuard Concurrent Stress Test                     ║");
    println!("║       Proving safety under multi-agent contention                ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    println!("  Configuration:");
    let agents_str = report.agent_names.join(", ");
    println!("    Agents:    {} ({})", report.agent_count, agents_str);
    println!("    Duration:  {:.2}s", report.duration_secs);
    println!("    Threads:   {} OS threads (true parallelism)\n", report.agent_count);

    println!("═══ Results ═══\n");
    println!("  Total proposals:     {:>8}", report.total_proposals);
    let commit_pct = if report.total_proposals > 0 { (report.total_committed as f64 / report.total_proposals as f64) * 100.0 } else { 0.0 };
    let reject_pct = if report.total_proposals > 0 { (report.total_rejected as f64 / report.total_proposals as f64) * 100.0 } else { 0.0 };
    println!("  Committed:           {:>8}  ({:>5.1}%)", report.total_committed, commit_pct);
    println!("  Rejected:            {:>8}  ({:>5.1}%)\n", report.total_rejected, reject_pct);

    println!("  Rejection Breakdown:");
    let fmt_pct = |val| if report.total_rejected > 0 { (val as f64 / report.total_rejected as f64) * 100.0 } else { 0.0 };
    println!("    Insufficient funds:    {:>8}  ({:>5.1}% of rejections)", report.rejected_insufficient, fmt_pct(report.rejected_insufficient));
    println!("    Account not found:     {:>8}  ({:>5.1}%)", report.rejected_not_found, fmt_pct(report.rejected_not_found));
    println!("    Unbalanced entries:    {:>8}  ({:>5.1}%)", report.rejected_unbalanced, fmt_pct(report.rejected_unbalanced));
    println!("    Overflow:              {:>8}  ({:>5.1}%)", report.rejected_overflow, fmt_pct(report.rejected_overflow));
    println!("    Construction failure:  {:>8}  ({:>5.1}%)", report.rejected_construction, fmt_pct(report.rejected_construction));
    println!("    Parse failure:         {:>8}  ({:>5.1}%)\n", report.rejected_parse, fmt_pct(report.rejected_parse));

    println!("  Performance:");
    println!("    Throughput:           {:.0} proposals/sec", report.throughput_per_sec);
    println!("    Latency p50:          {} ms", report.p50_latency_ms.map_or("N/A".to_string(), |v| format!("{:.2}", v)));
    println!("    Latency p95:          {} ms", report.p95_latency_ms.map_or("N/A".to_string(), |v| format!("{:.2}", v)));
    println!("    Latency p99:          {} ms", report.p99_latency_ms.map_or("N/A".to_string(), |v| format!("{:.2}", v)));
    println!("    Lock contention:      {:.1}% of wall time", report.contention_pct);
    println!("    Panics caught:        {} (via catch_unwind)\n", report.total_panics);

    println!("═══ Per-Agent Breakdown ═══\n");
    println!("  {:<18} {:>10} {:>12} {:>11} {:>9}", "Agent", "Proposed", "Committed", "Rejected", "Commit%");
    println!("  ─────────────────────────────────────────────────────────────");
    for a in &report.per_agent {
        let pct = if a.proposed > 0 { (a.committed as f64 / a.proposed as f64) * 100.0 } else { 0.0 };
        println!("  {:<18} {:>10} {:>12} {:>11} {:>8.1}%", a.name, a.proposed, a.committed, a.rejected, pct);
    }
    println!("\n═══ Invariant Verification ═══\n");
    for inv in invariants {
        match inv {
            VerificationResult::Pass(msg) => println!("  ✅ {}", msg),
            VerificationResult::Fail(msg) => println!("  ❌ {}", msg),
        }
    }

    let passed = invariants.iter().all(|r| matches!(r, VerificationResult::Pass(_)));
    if passed {
        println!("\n═══ What This Proves ═══\n");
        println!("  {} unreliable agents submitted {} proposals over {:.2} seconds.", report.agent_count, report.total_proposals, report.duration_secs);
        println!("  The ledger committed {} valid transactions and rejected {} invalid ones.", report.total_committed, report.total_rejected);
        println!("  Zero invariant violations. Zero data races.");
        println!("  Rust's type system made the concurrency proof free.\n");
    } else {
        println!("\n═══ INVARIANT VIOLATION DETECTED ═══");
        println!("  The ledger state is corrupted or inconsistent.");
    }
}

pub fn verify_invariants(ledger: &Ledger, metrics: &MetricsCollector) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Check 1 — Non-negative balances
    let mut neg = false;
    for (acc, bal) in ledger.accounts() {
        if acc != "External" && bal < 0 {
            results.push(VerificationResult::Fail(format!("Account {} has negative balance: {}", acc, bal)));
            neg = true;
        }
    }
    if !neg {
        results.push(VerificationResult::Pass("All non-External balances ≥ 0".to_string()));
    }

    // UNDERSTANDING CHECK — answer this before reading the implementation:
    //
    // Why does Transaction::new's zero-sum invariant GUARANTEE that the sum
    // of all account balances is always zero after any number of commits?
    //
    // Write the inductive proof here (4 sentences):
    //   Base case: Empty ledger has sum 0.
    //   Inductive step: New transaction has zero-sum entries. Checked add mutates balance.
    //   Therefore: Total sum remains unchanged (sum + 0 = sum).
    //   Implication for this check: It must hold for the entire lifetime of the process.

    // Check 2 — Conservation of money
    let sum: i64 = ledger.accounts().into_iter().map(|(_, b)| b).sum();
    if sum == 0 {
        results.push(VerificationResult::Pass("Sum of all balances = 0 (double-entry conservation)".to_string()));
    } else {
        results.push(VerificationResult::Fail(format!("Conservation violation: sum = {}", sum)));
    }

    // Check 3 — Transaction count consistency
    // Ledger includes initial funding transactions from StressTest::new.
    // Count those by finding the first stress-test tx (initial txs have lower IDs).
    let initial_txs = ledger.history().iter()
        .take_while(|tx| tx.description.starts_with("Initial funding"))
        .count() as u64;
    let stress_count = ledger.transaction_count() as u64 - initial_txs;
    let metrics_commit = metrics.total_committed();
    if stress_count == metrics_commit {
        results.push(VerificationResult::Pass("Transaction count matches committed count".to_string()));
    } else {
        results.push(VerificationResult::Fail(format!("Transaction count mismatch: ledger {} (stress only), metrics {}", stress_count, metrics_commit)));
    }

    // Check 4 — Replay consistency
    let mut replay = Ledger::new();
    for (acc, _) in ledger.accounts() {
        let _ = replay.create_account(&acc);
    }
    for tx in ledger.history() {
        if let Err(e) = replay.apply(tx.clone()) {
            results.push(VerificationResult::Fail(format!("Replay failed on tx {}: {}", tx.id, e)));
            return results;
        }
    }
    
    let original = ledger.accounts();
    let replayed = replay.accounts();
    if original == replayed {
        results.push(VerificationResult::Pass("Replay consistency verified".to_string()));
    } else {
        results.push(VerificationResult::Fail("Replay consistency failed: mismatch in accounts".to_string()));
    }

    results
}
