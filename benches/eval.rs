//! Benchmark evaluation harness.
//! Run: cargo run --bin eval -- --corpus benches/corpus/ --mock
//! (Requires building a separate binary — add to Cargo.toml if needed)

use std::time::Instant;
use ledger_guard::{agent::{Agent, AgentProposal, AgentError}, ledger::{Ledger, Transaction}};

#[derive(Debug)]
pub struct EvalResult {
    pub id: usize,
    pub category: String,
    pub input: String,
    pub expected_valid: bool,
    pub agent_error: Option<String>,
    pub ledger_committed: bool,
    pub ledger_error: Option<String>,
    pub latency_ms: f64,
    pub required_retry: bool,
}

#[derive(Debug)]
pub struct AccuracyReport {
    pub by_category: std::collections::HashMap<String, CategoryReport>,
}

#[derive(Debug)]
pub struct CategoryReport {
    pub total: usize,
    pub parse_success_first_attempt: usize,
    pub parse_success_after_retry: usize,
    pub ledger_committed: usize,
    pub ledger_correctly_rejected: usize,
    pub false_negatives: usize,
    pub false_positives: usize,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
}

/// Run all corpus entries through the agent and ledger pipeline.
pub fn run_eval(corpus: &[CorpusEntry], agent: &dyn Agent, ledger: &mut Ledger) -> Vec<EvalResult> {
    corpus.iter().enumerate().map(|(i, entry)| {
        let start = Instant::now();
        let (agent_error, ledger_committed, ledger_error, required_retry) =
            match agent.propose(&entry.input) {
                Err(e) => (Some(format!("{:?}", e)), false, None, false),
                Ok(proposal) => {
                    match Transaction::new(proposal.description, proposal.entries) {
                        Err(e) => (None, false, Some(format!("{:?}", e)), false),
                        Ok(tx) => match ledger.apply(tx) {
                            Ok(()) => (None, true, None, false),
                            Err(e) => (None, false, Some(format!("{:?}", e)), false),
                        }
                    }
                }
            };
        EvalResult {
            id: entry.id,
            category: entry.category.clone(),
            input: entry.input.clone(),
            expected_valid: entry.expected_valid,
            agent_error,
            ledger_committed,
            ledger_error,
            latency_ms: start.elapsed().as_secs_f64() * 1000.0,
            required_retry,
        }
    }).collect()
}

/// UNIMPLEMENTED — Developer task
///
/// This function takes raw EvalResults and computes the AccuracyReport.
///
/// METRICS TO COMPUTE PER CATEGORY:
///
///   parse_success_first_attempt: results where agent_error is None (counted without retry)
///   parse_success_after_retry: same, but agent was called twice for failures on first attempt
///   ledger_committed: results where ledger_committed == true
///   ledger_correctly_rejected: expected_valid == false AND ledger_committed == false
///   false_negatives: expected_valid == true AND agent succeeded AND ledger_committed == false
///     → THIS IS A BUG in Ledger::apply. Report it as such, not as an accuracy number.
///   false_positives: expected_valid == false AND ledger_committed == true
///     → THIS IS A BUG in Transaction::new or Ledger::apply. Report as a bug.
///
///   p50/p95 latency: sort latency_ms values per category, index at 50th/95th percentile.
///
/// IMPORTANT: The ledger's job is 100% true-positive and 100% true-negative rates.
///   Any false_negative or false_positive is a correctness bug, NOT an accuracy trade-off.
///   Report bugs separately from accuracy numbers. Don't average them together.
///   The agent's accuracy is a measured number, reported honestly — no target threshold.
///   If paraphrased parse rate is 62%, that's the number. Publish it.
///
/// IMPLEMENTATION OUTLINE:
///   Group results by category using a HashMap.
///   For each group, iterate results computing each metric.
///   Sort latencies separately for percentile computation.
///   Return AccuracyReport { by_category }.
pub fn compute_accuracy(results: &[EvalResult]) -> AccuracyReport {
    todo!("compute_accuracy — implement per-category metrics (see guide above)")
}

#[derive(serde::Deserialize)]
pub struct CorpusEntry {
    pub id: usize,
    pub category: String,
    pub input: String,
    pub expected_valid: bool,
}

/// Load a corpus JSON file from disk.
///
/// IMPLEMENTATION:
///   1. Read the file to a string — return empty Vec if file not found.
///   2. Parse with serde_json::from_str::<Vec<CorpusEntry>>.
///   3. Return the parsed entries, or panic with a clear error on malformed JSON.
pub fn load_corpus(path: &str) -> Vec<CorpusEntry> {
    todo!("load_corpus — read JSON file and deserialize into Vec<CorpusEntry>")
}
