use ledger_guard::agent::mock::MockAgent;
use ledger_guard::eval::{
    compute_accuracy, load_corpus_dir, print_eval_report, run_eval,
};
use ledger_guard::ledger::{AccountId, Entry, Ledger, Transaction};

const ACCOUNTS: &[&str] = &[
    "Checking",
    "Savings",
    "External",
    "Revenue",
    "Expenses",
    "Investments",
    "Payroll",
    "Escrow",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let corpus_dir = args
        .iter()
        .position(|a| a == "--corpus")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("benches/corpus");

    println!("Loading corpus from: {}", corpus_dir);
    let corpus = load_corpus_dir(corpus_dir);
    if corpus.is_empty() {
        eprintln!("No corpus entries found in {}. Create JSON files there first.", corpus_dir);
        std::process::exit(1);
    }
    println!("Loaded {} corpus entries.\n", corpus.len());

    let agent = MockAgent {
        known_accounts: ACCOUNTS.iter().map(|s| s.to_string()).collect(),
    };

    let mut ledger = Ledger::new();
    for name in ACCOUNTS {
        ledger.create_account(name).unwrap();
    }
    // Fund accounts so valid transfers can succeed
    let funding = [
        ("Checking", 1_000_000i64),
        ("Savings", 500_000),
        ("Revenue", 500_000),
        ("Expenses", 200_000),
        ("Investments", 300_000),
        ("Payroll", 400_000),
        ("Escrow", 100_000),
    ];
    for (account, amount) in &funding {
        let tx = Transaction::new(
            format!("Fund {}", account),
            vec![
                Entry {
                    account: AccountId("External".into()),
                    amount: -amount,
                },
                Entry {
                    account: AccountId(account.to_string()),
                    amount: *amount,
                },
            ],
        )
        .unwrap();
        ledger.apply(tx).unwrap();
    }

    let results = run_eval(&corpus, &agent, &mut ledger);
    let report = compute_accuracy(&results);
    print_eval_report(&report, &results);

    // Export results as JSON for graph generation
    let json = serde_json::to_string_pretty(&results).unwrap();
    let _ = std::fs::write("output/eval_results.json", &json);
    let report_json = serde_json::to_string_pretty(&report).unwrap();
    let _ = std::fs::write("output/eval_report.json", &report_json);
    println!("\nResults written to output/eval_results.json and output/eval_report.json");
}
