use std::collections::HashMap;
use std::time::Instant;
use crate::agent::Agent;
use crate::ledger::{Ledger, Transaction};

#[derive(Debug, Clone, serde::Serialize)]
pub struct EvalResult {
    pub id: usize,
    pub category: String,
    pub input: String,
    pub expected_valid: bool,
    pub agent_error: Option<String>,
    pub ledger_committed: bool,
    pub ledger_error: Option<String>,
    pub latency_ms: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AccuracyReport {
    pub by_category: HashMap<String, CategoryReport>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryReport {
    pub total: usize,
    pub parse_success: usize,
    pub ledger_committed: usize,
    pub ledger_correctly_rejected: usize,
    pub false_negatives: usize,
    pub false_positives: usize,
    pub p50_latency_ms: f64,
    pub p95_latency_ms: f64,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct CorpusEntry {
    pub id: usize,
    pub category: String,
    pub input: String,
    pub expected_valid: bool,
}

/// Load a corpus JSON file from disk.
pub fn load_corpus(path: &str) -> Vec<CorpusEntry> {
    match std::fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .unwrap_or_else(|e| panic!("Malformed corpus JSON in {}: {}", path, e)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => panic!("Failed to read corpus file {}: {}", path, e),
    }
}

/// Load all corpus files from a directory.
pub fn load_corpus_dir(dir: &str) -> Vec<CorpusEntry> {
    let mut entries = Vec::new();
    let path = std::path::Path::new(dir);
    if !path.exists() {
        return entries;
    }
    let mut files: Vec<_> = std::fs::read_dir(path)
        .expect("Failed to read corpus directory")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort_by_key(|e| e.path());
    for file in files {
        let file_entries = load_corpus(file.path().to_str().unwrap());
        entries.extend(file_entries);
    }
    entries
}

/// Run all corpus entries through the agent and ledger pipeline.
pub fn run_eval(corpus: &[CorpusEntry], agent: &dyn Agent, ledger: &mut Ledger) -> Vec<EvalResult> {
    corpus
        .iter()
        .map(|entry| {
            let start = Instant::now();
            let (agent_error, ledger_committed, ledger_error) = match agent.propose(&entry.input) {
                Err(e) => (Some(format!("{:?}", e)), false, None),
                Ok(proposal) => match Transaction::new(proposal.description, proposal.entries) {
                    Err(e) => (None, false, Some(format!("{}", e))),
                    Ok(tx) => match ledger.apply(tx) {
                        Ok(()) => (None, true, None),
                        Err(e) => (None, false, Some(format!("{}", e))),
                    },
                },
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
            }
        })
        .collect()
}

/// Compute per-category accuracy metrics from evaluation results.
pub fn compute_accuracy(results: &[EvalResult]) -> AccuracyReport {
    let mut groups: HashMap<String, Vec<&EvalResult>> = HashMap::new();
    for r in results {
        groups.entry(r.category.clone()).or_default().push(r);
    }

    let mut by_category = HashMap::new();
    for (category, group) in groups {
        let total = group.len();
        let parse_success = group.iter().filter(|r| r.agent_error.is_none()).count();
        let ledger_committed = group.iter().filter(|r| r.ledger_committed).count();

        // Correctly rejected: expected invalid AND not committed
        let ledger_correctly_rejected = group
            .iter()
            .filter(|r| !r.expected_valid && !r.ledger_committed)
            .count();

        // False negatives: expected valid, agent succeeded, but ledger rejected
        // This is a BUG in the ledger — report separately
        let false_negatives = group
            .iter()
            .filter(|r| r.expected_valid && r.agent_error.is_none() && !r.ledger_committed)
            .count();

        // False positives: expected invalid but ledger committed
        // This is a BUG in Transaction::new or Ledger::apply — report separately
        let false_positives = group
            .iter()
            .filter(|r| !r.expected_valid && r.ledger_committed)
            .count();

        // Latency percentiles
        let mut latencies: Vec<f64> = group.iter().map(|r| r.latency_ms).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile(&latencies, 0.50);
        let p95 = percentile(&latencies, 0.95);

        by_category.insert(
            category,
            CategoryReport {
                total,
                parse_success,
                ledger_committed,
                ledger_correctly_rejected,
                false_negatives,
                false_positives,
                p50_latency_ms: p50,
                p95_latency_ms: p95,
            },
        );
    }

    AccuracyReport { by_category }
}

fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 * pct) as usize).min(sorted.len() - 1);
    sorted[idx]
}

/// Print a formatted evaluation report.
pub fn print_eval_report(report: &AccuracyReport, results: &[EvalResult]) {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║           LedgerGuard Evaluation Report                          ║");
    println!("║       Agent accuracy vs. Ledger correctness                      ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    println!(
        "  Total queries evaluated: {}\n",
        results.len()
    );

    // Sort categories for consistent output
    let mut categories: Vec<_> = report.by_category.keys().collect();
    categories.sort();

    println!(
        "  {:<14} {:>6} {:>8} {:>10} {:>10} {:>8} {:>8} {:>8} {:>8}",
        "Category", "Total", "Parsed", "Parse%", "Committed", "Commit%", "FalseN", "FalseP", "p50ms"
    );
    println!("  ─────────────────────────────────────────────────────────────────────────────────────");

    for cat in &categories {
        let c = &report.by_category[*cat];
        let parse_pct = if c.total > 0 {
            c.parse_success as f64 / c.total as f64 * 100.0
        } else {
            0.0
        };
        let commit_pct = if c.total > 0 {
            c.ledger_committed as f64 / c.total as f64 * 100.0
        } else {
            0.0
        };
        println!(
            "  {:<14} {:>6} {:>8} {:>9.1}% {:>10} {:>7.1}% {:>8} {:>8} {:>7.3}",
            cat,
            c.total,
            c.parse_success,
            parse_pct,
            c.ledger_committed,
            commit_pct,
            c.false_negatives,
            c.false_positives,
            c.p50_latency_ms
        );
    }

    println!("\n═══ Ledger Correctness ═══\n");

    let total_fn: usize = report.by_category.values().map(|c| c.false_negatives).sum();
    let total_fp: usize = report.by_category.values().map(|c| c.false_positives).sum();

    if total_fn == 0 && total_fp == 0 {
        println!("  ✅ Zero false negatives (valid tx rejected by ledger)");
        println!("  ✅ Zero false positives (invalid tx accepted by ledger)");
        println!("  The ledger's 100% true-positive and true-negative rates are confirmed.");
    } else {
        if total_fn > 0 {
            println!(
                "  ❌ {} false negative(s) — valid transactions rejected by ledger (BUG)",
                total_fn
            );
        }
        if total_fp > 0 {
            println!(
                "  ❌ {} false positive(s) — invalid transactions accepted by ledger (BUG)",
                total_fp
            );
        }
    }

    // Show individual false positives/negatives for debugging
    if total_fp > 0 || total_fn > 0 {
        println!();
        for r in results {
            if !r.expected_valid && r.ledger_committed {
                println!("    FP id={}: \"{}\"", r.id, &r.input[..r.input.len().min(60)]);
            }
            if r.expected_valid && r.agent_error.is_none() && !r.ledger_committed {
                println!("    FN id={}: \"{}\" err={}", r.id, &r.input[..r.input.len().min(60)], r.ledger_error.as_deref().unwrap_or("?"));
            }
        }
    }

    println!("\n═══ What This Means ═══\n");
    println!("  Agent accuracy is a measured number, reported honestly.");
    println!("  The ledger's job is 100% true-positive and 100% true-negative rates.");
    println!("  Any false negative or false positive is a correctness bug, not a trade-off.\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_corpus_missing_file() {
        let entries = load_corpus("/nonexistent/path.json");
        assert!(entries.is_empty());
    }

    #[test]
    fn test_compute_accuracy_empty() {
        let report = compute_accuracy(&[]);
        assert!(report.by_category.is_empty());
    }

    #[test]
    fn test_compute_accuracy_basic() {
        let results = vec![
            EvalResult {
                id: 1,
                category: "templated".into(),
                input: "test".into(),
                expected_valid: true,
                agent_error: None,
                ledger_committed: true,
                ledger_error: None,
                latency_ms: 0.5,
            },
            EvalResult {
                id: 2,
                category: "templated".into(),
                input: "test2".into(),
                expected_valid: false,
                agent_error: None,
                ledger_committed: false,
                ledger_error: Some("rejected".into()),
                latency_ms: 0.3,
            },
        ];
        let report = compute_accuracy(&results);
        let cat = &report.by_category["templated"];
        assert_eq!(cat.total, 2);
        assert_eq!(cat.parse_success, 2);
        assert_eq!(cat.ledger_committed, 1);
        assert_eq!(cat.ledger_correctly_rejected, 1);
        assert_eq!(cat.false_negatives, 0);
        assert_eq!(cat.false_positives, 0);
    }

    #[test]
    fn test_percentile() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        // idx = (10 * 0.5) = 5 -> data[5] = 6.0
        assert_eq!(percentile(&data, 0.5), 6.0);
        // idx = (10 * 0.95) = 9 -> data[9] = 10.0
        assert_eq!(percentile(&data, 0.95), 10.0);
        assert_eq!(percentile(&[], 0.5), 0.0);
    }
}
