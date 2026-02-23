// ============================================================================
// Main Entry Point (`src/main.rs`)
// ============================================================================
//
// This is the binary entry point for LedgerGuard. It handles:
//
//   1. CLI argument parsing (`--mock`, `--data <path>`)
//   2. Loading existing ledger data (if any)
//   3. Setting up default accounts for new ledgers
//   4. Launching the interactive REPL
//
// WHY `std::env::args` INSTEAD OF `clap`?
// ───────────────────────────────────────
// `clap` is the standard Rust CLI argument parser, but it's a heavy
// dependency (~50 sub-crates). For just two flags (`--mock` and `--data`),
// manual parsing with `std::env::args()` is simpler and keeps compile
// times fast. If more flags are needed later, `clap` can be added.
//
// STARTUP FLOW:
// ─────────────
// 1. Parse args → determine agent type and data path
// 2. Check if data file exists
//    - Yes → load and replay (re-validates everything)
//    - No  → create fresh ledger with default accounts
// 3. Create the appropriate agent (mock or future LLM)
// 4. Launch the REPL
// 5. On exit → save state to disk (atomic write)
// ============================================================================

use ledger_guard::agent::mock::MockAgent;
use ledger_guard::cli::Cli;
use ledger_guard::ledger::Ledger;
use ledger_guard::persistence;

fn main() {
    // ── Parse command-line arguments ────────────────────────────────
    //
    // We use `std::env::args()` for zero-dependency arg parsing.
    // `args()` returns an iterator of String where:
    //   - args[0] = the program name (e.g., "ledger-guard.exe")
    //   - args[1..] = the user's arguments
    //
    // We collect into a Vec so we can index into it.
    let args: Vec<String> = std::env::args().collect();

    // ── Determine agent mode ────────────────────────────────────────
    //
    // `--mock` flag: use the deterministic regex-based agent.
    // Without this flag, we currently default to mock (since the LLM
    // backend isn't implemented yet). When LLM integration is added,
    // this will become the fallback mode.
    //
    // `.iter().any()` scans all arguments for the flag. This means
    // `--mock` can appear anywhere: `ledger-guard --mock --data x`
    // or `ledger-guard --data x --mock` both work.
    let use_mock = args.iter().any(|a| a == "--mock");

    // ── Determine data file path ────────────────────────────────────
    //
    // `--data <path>` flag: specify where to save/load ledger state.
    // Default: "ledger_data.json" in the current directory.
    //
    // We find the `--data` flag, then take the next argument as the path.
    // `.windows(2)` gives us sliding windows of 2 consecutive args,
    // so we can check for ["--data", "<path>"] pairs.
    let data_path = args
        .windows(2)
        .find(|w| w[0] == "--data")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "ledger_data.json".to_string());

    // ── Check for --help ────────────────────────────────────────────
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("LedgerGuard — A type-safe Rust ledger that catches every AI mistake");
        println!();
        println!("USAGE:");
        println!("  ledger-guard [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("  --mock           Use deterministic mock agent (no model download needed)");
        println!("  --data <path>    Path to ledger data file (default: ledger_data.json)");
        println!("  --help, -h       Show this help message");
        println!();
        println!("EXAMPLES:");
        println!("  ledger-guard --mock                  # Start with mock agent");
        println!("  ledger-guard --mock --data my.json   # Custom data file");
        return;
    }

    // ── Load or create ledger ───────────────────────────────────────
    //
    // If a data file exists, we load it (which re-validates everything
    // by replaying transactions). If not, we create a fresh ledger
    // with a set of default accounts.
    let ledger = if persistence::data_file_exists(&data_path) {
        println!("📂 Loading ledger from '{}'...", data_path);
        match persistence::load(&data_path) {
            Ok(ledger) => {
                println!(
                    "   ✅ Loaded {} accounts, {} transactions",
                    ledger.accounts().len(),
                    ledger.transaction_count()
                );
                ledger
            }
            Err(e) => {
                eprintln!("   ❌ Failed to load '{}': {}", data_path, e);
                eprintln!("   Starting with a fresh ledger instead.");
                create_default_ledger()
            }
        }
    } else {
        println!("📂 No existing data file — starting fresh.");
        create_default_ledger()
    };

    // ── Create the agent ────────────────────────────────────────────
    //
    // Currently only the mock agent is available. When LLM integration
    // is added, this will branch based on `use_mock`:
    //
    //   if use_mock {
    //       Box::new(MockAgent::new())
    //   } else {
    //       Box::new(LlmAgent::new("models/phi-3-mini.gguf")?)
    //   }
    //
    // For now, we always use the mock agent but log differently.
    let agent: Box<dyn ledger_guard::agent::Agent> = if use_mock {
        println!("🤖 Using mock agent (deterministic regex parser)");
        Box::new(MockAgent::new())
    } else {
        // TODO: Replace with LLM agent when ready
        println!("🤖 LLM agent not yet implemented — falling back to mock agent");
        println!("   (Use --mock to suppress this message)");
        Box::new(MockAgent::new())
    };

    // ── Launch the REPL ─────────────────────────────────────────────
    let mut cli = Cli::new(ledger, agent, data_path);

    if let Err(e) = cli.run() {
        eprintln!("\n❌ Fatal error: {}", e);
        std::process::exit(1);
    }
}

/// Create a fresh ledger pre-loaded with useful default accounts.
///
/// These defaults give the user something to work with immediately
/// without having to manually create accounts first. They represent
/// a typical personal finance setup:
///
/// - **Checking** — Primary spending account
/// - **Savings** — Long-term savings
/// - **External** — Represents money flowing in/out of the system
///   (deposits from outside, withdrawals to outside)
///
/// # Why "External"?
///
/// In double-entry bookkeeping, money can't appear from nowhere. When
/// the user says "deposit $100 to Savings", the money has to come FROM
/// somewhere. The "External" account represents the outside world —
/// it's the counterparty for deposits and withdrawals.
fn create_default_ledger() -> Ledger {
    let mut ledger = Ledger::new();

    // Create default accounts
    let defaults = vec![
        "Checking",
        "Savings",
        "External",
    ];

    for name in defaults {
        ledger
            .create_account(name.to_string())
            .expect("Default account creation should never fail");
    }

    println!(
        "   Created {} default accounts: Checking, Savings, External",
        ledger.accounts().len()
    );

    ledger
}
