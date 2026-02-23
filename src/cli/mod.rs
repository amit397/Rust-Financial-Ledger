// ============================================================================
// CLI Module — Interactive REPL (`src/cli/mod.rs`)
// ============================================================================
//
// This module ties everything together into an interactive command-line
// experience. It's the user-facing interface that:
//
//   1. Reads input using `rustyline` (readline-style line editor with history)
//   2. Routes commands (balance, history, accounts, help, quit)
//   3. Sends natural language input to the agent
//   4. Displays agent proposals → ledger verdicts (✅ or ❌)
//   5. Saves ledger state to disk on exit
//
// SEPARATION OF CONCERNS:
// ──────────────────────
// The CLI module knows about display formatting and command routing, but
// it does NOT contain any business logic. All validation is delegated to
// the ledger module. This means:
// - The CLI can't accidentally bypass invariant checks
// - The ledger can be used without the CLI (e.g., in tests, benchmarks)
// - The agent can be swapped (mock ↔ LLM) without touching the CLI
//
// ERROR DISPLAY:
// ─────────────
// When the ledger rejects a transaction, the CLI displays the structured
// error message (from `LedgerError::Display`). This gives the user enough
// context to fix their input. For example:
//
//   ❌ Insufficient funds: 'Checking' has 5000 cents, but 10000 cents were requested
//
// vs. the unhelpful alternative:
//
//   ❌ Transaction failed
// ============================================================================

use crate::agent::{Agent, AgentError};
use crate::ledger::{Ledger, Transaction, LedgerError};
use crate::persistence;

use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

/// The interactive CLI application.
///
/// Owns the ledger, agent, and REPL editor. Created once at startup
/// and runs until the user types "quit" or sends Ctrl+C/Ctrl+D.
pub struct Cli {
    /// The financial ledger (source of truth for all balances)
    ledger: Ledger,

    /// The agent that parses natural language into proposals
    /// (either MockAgent or a future LLM agent)
    ///
    /// `Box<dyn Agent>` is a trait object — it allows dynamic dispatch
    /// at runtime based on the `--mock` flag. The performance cost is
    /// negligible (one vtable lookup per user input).
    agent: Box<dyn Agent>,

    /// Path to the persistence file
    data_path: String,
}

impl Cli {
    /// Create a new CLI application.
    ///
    /// # Arguments
    ///
    /// * `ledger` — The ledger to operate on (may be loaded from disk or fresh)
    /// * `agent` — The agent to use for parsing (mock or LLM)
    /// * `data_path` — Where to save ledger state on exit
    pub fn new(ledger: Ledger, agent: Box<dyn Agent>, data_path: String) -> Cli {
        Cli {
            ledger,
            agent,
            data_path,
        }
    }

    /// Run the interactive REPL loop.
    ///
    /// This is the main event loop. It:
    /// 1. Prints the welcome banner
    /// 2. Reads a line of input
    /// 3. Routes to the appropriate handler
    /// 4. Repeats until "quit", Ctrl+C, or Ctrl+D
    /// 5. Saves ledger state on exit
    ///
    /// # Why `rustyline`?
    ///
    /// Plain `stdin.read_line()` works but provides a poor user experience:
    /// - No line editing (can't use arrow keys to fix typos)
    /// - No command history (can't press Up to repeat a command)
    /// - No signal handling (Ctrl+C crashes the process)
    ///
    /// `rustyline` gives us all of these for free, making the REPL feel
    /// like a proper shell.
    pub fn run(&mut self) -> Result<(), LedgerError> {
        // ── Create the readline editor ──────────────────────────────
        let mut rl = DefaultEditor::new().map_err(|e| LedgerError::IoError {
            message: format!("Failed to initialize readline: {}", e),
        })?;

        // ── Print welcome banner ────────────────────────────────────
        self.print_banner();

        // ── Main REPL loop ──────────────────────────────────────────
        loop {
            // Read a line with the "ledger> " prompt.
            // `readline` handles:
            //   - Line editing (arrow keys, backspace, etc.)
            //   - History (Up/Down arrow to recall previous inputs)
            //   - Signal handling (Ctrl+C → Interrupted, Ctrl+D → Eof)
            match rl.readline("ledger> ") {
                Ok(line) => {
                    let trimmed = line.trim().to_string();
                    if trimmed.is_empty() {
                        continue;
                    }

                    // Add to history so the user can recall it with Up arrow
                    let _ = rl.add_history_entry(&trimmed);

                    // Route the command to the appropriate handler
                    self.handle_input(&trimmed);
                }

                // Ctrl+C: cancel current line (don't exit)
                Err(ReadlineError::Interrupted) => {
                    println!("(Ctrl+C — type 'quit' to exit)");
                }

                // Ctrl+D: end of input (exit gracefully)
                Err(ReadlineError::Eof) => {
                    println!("\nGoodbye!");
                    break;
                }

                // Other readline errors (rare)
                Err(err) => {
                    eprintln!("Readline error: {}", err);
                    break;
                }
            }
        }

        // ── Save on exit ────────────────────────────────────────────
        self.save_state()?;

        Ok(())
    }

    /// Route user input to the appropriate handler.
    ///
    /// Built-in commands are checked first (case-insensitive). If the
    /// input doesn't match any command, it's sent to the agent for
    /// natural language parsing.
    fn handle_input(&mut self, input: &str) {
        // Split the input into command + arguments
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let command = parts[0].to_lowercase();

        match command.as_str() {
            // ── Built-in commands ───────────────────────────────────
            "quit" | "exit" | "q" => {
                println!("\nSaving ledger state...");
                match self.save_state() {
                    Ok(()) => println!("✅ Saved to '{}'", self.data_path),
                    Err(e) => eprintln!("❌ Save failed: {}", e),
                }
                println!("Goodbye!");
                std::process::exit(0);
            }

            "help" | "h" | "?" => {
                self.print_help();
            }

            "balance" | "bal" | "b" => {
                if parts.len() < 2 {
                    println!("Usage: balance <account_name>");
                    println!("  Example: balance Checking");
                } else {
                    self.handle_balance(parts[1].trim());
                }
            }

            "history" | "hist" => {
                self.handle_history();
            }

            "accounts" | "accts" | "a" => {
                self.handle_accounts();
            }

            "create" => {
                if parts.len() < 2 {
                    println!("Usage: create <account_name>");
                    println!("  Example: create Savings");
                } else {
                    self.handle_create_account(parts[1].trim());
                }
            }

            "save" => {
                match self.save_state() {
                    Ok(()) => println!("✅ Saved ledger to '{}'", self.data_path),
                    Err(e) => eprintln!("❌ Save failed: {}", e),
                }
            }

            // ── Not a built-in command → send to agent ──────────────
            _ => {
                self.handle_agent_input(input);
            }
        }
    }

    /// Send natural language input to the agent and process the proposal.
    ///
    /// This is where the core pipeline executes:
    /// 1. Agent parses input → `AgentProposal` (or error)
    /// 2. `Transaction::new` validates proposal → `Transaction` (or error)
    /// 3. `Ledger::apply` validates against state → committed (or error)
    ///
    /// Each step can fail independently, and the CLI displays appropriate
    /// messages for each failure type.
    fn handle_agent_input(&mut self, input: &str) {
        // ── Step 1: Agent parses natural language ────────────────────
        println!("\n🤖 Agent: Parsing input...");

        let proposal = match self.agent.propose(input) {
            Ok(proposal) => proposal,
            Err(AgentError::EmptyInput) => {
                println!("❌ Empty input. Type a financial command or 'help' for options.");
                return;
            }
            Err(AgentError::ParseFailure(msg)) => {
                println!("❌ Agent could not parse: {}", msg);
                return;
            }
        };

        // ── Display the proposal ────────────────────────────────────
        println!("📋 Proposal: {}", proposal.description);
        for entry in &proposal.entries {
            if entry.amount < 0 {
                println!(
                    "   DEBIT  {} ← ${:.2}",
                    entry.account_id,
                    (-entry.amount) as f64 / 100.0
                );
            } else {
                println!(
                    "   CREDIT {} ← ${:.2}",
                    entry.account_id,
                    entry.amount as f64 / 100.0
                );
            }
        }

        // ── Step 2: Validate construction-time invariants ───────────
        //
        // `Transaction::new` checks: non-empty, no zeros, overflow, balanced.
        // If the agent produced garbage, this catches it.
        let transaction = match Transaction::new(proposal.description, proposal.entries) {
            Ok(tx) => tx,
            Err(e) => {
                println!("\n❌ REJECTED (construction): {}", e);
                println!("   The agent's proposal violated a structural invariant.");
                return;
            }
        };

        // ── Step 3: Validate stateful invariants ────────────────────
        //
        // `Ledger::apply` checks: account existence, sufficient funds, overflow.
        // Even if the transaction is structurally valid, it might not be
        // valid in the current ledger state.
        match self.ledger.apply(transaction) {
            Ok(()) => {
                println!("\n✅ COMMITTED — ledger updated successfully.");

                // Show updated balances for affected accounts
                let affected: Vec<String> = self
                    .ledger
                    .history()
                    .last()
                    .map(|tx| {
                        tx.entries
                            .iter()
                            .map(|e| e.account_id.clone())
                            .collect()
                    })
                    .unwrap_or_default();

                for account in affected {
                    if let Ok(balance) = self.ledger.balance(&account) {
                        println!(
                            "   {} balance: ${:.2}",
                            account,
                            balance as f64 / 100.0
                        );
                    }
                }
            }
            Err(e) => {
                println!("\n❌ REJECTED (stateful): {}", e);
                println!("   The transaction was structurally valid but violated a ledger constraint.");
            }
        }
    }

    /// Display the balance of a specific account.
    fn handle_balance(&self, account: &str) {
        match self.ledger.balance(account) {
            Ok(balance) => {
                println!(
                    "{}: ${:.2}",
                    account,
                    balance as f64 / 100.0
                );
            }
            Err(LedgerError::AccountNotFound { .. }) => {
                println!("❌ Account '{}' not found.", account);
                println!("   Use 'accounts' to see all registered accounts.");
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    /// Display the full transaction history.
    fn handle_history(&self) {
        let history = self.ledger.history();

        if history.is_empty() {
            println!("No transactions yet.");
            return;
        }

        println!("═══ Transaction History ({} transactions) ═══\n", history.len());
        for (i, tx) in history.iter().enumerate() {
            println!("{}. {}", i + 1, tx);
        }
    }

    /// Display all accounts and their balances.
    fn handle_accounts(&self) {
        let accounts = self.ledger.accounts();

        if accounts.is_empty() {
            println!("No accounts registered yet.");
            println!("Use 'create <name>' to add an account.");
            return;
        }

        println!("═══ Accounts ═══\n");
        println!("  {:<20} {:>12}", "Account", "Balance");
        println!("  {:<20} {:>12}", "───────", "───────");

        let mut total: i64 = 0;
        for (name, balance) in &accounts {
            println!(
                "  {:<20} {:>12}",
                name,
                format!("${:.2}", *balance as f64 / 100.0)
            );
            total += balance;
        }

        println!("  {:<20} {:>12}", "───────", "───────");
        println!(
            "  {:<20} {:>12}",
            "TOTAL",
            format!("${:.2}", total as f64 / 100.0)
        );
    }

    /// Create a new account.
    fn handle_create_account(&mut self, name: &str) {
        match self.ledger.create_account(name.to_string()) {
            Ok(()) => println!("✅ Account '{}' created with $0.00 balance.", name),
            Err(LedgerError::AccountAlreadyExists { .. }) => {
                println!("❌ Account '{}' already exists.", name);
            }
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    /// Save the current ledger state to disk.
    fn save_state(&self) -> Result<(), LedgerError> {
        persistence::save(&self.ledger, &self.data_path)
    }

    /// Print the welcome banner with usage information.
    fn print_banner(&self) {
        println!("╔══════════════════════════════════════════════════════════════╗");
        println!("║                    LedgerGuard v0.1.0                       ║");
        println!("║    A type-safe Rust ledger that catches every AI mistake    ║");
        println!("╚══════════════════════════════════════════════════════════════╝");
        println!();
        println!("  Agent: {}", self.agent.name());
        println!("  Data:  {}", self.data_path);
        println!("  Accounts: {}", self.ledger.accounts().len());
        println!("  Transactions: {}", self.ledger.transaction_count());
        println!();
        println!("  Type 'help' for commands, or enter a financial transaction.");
        println!("  Example: \"transfer $50 from Checking to Savings\"");
        println!();
    }

    /// Print the help message listing all available commands.
    fn print_help(&self) {
        println!();
        println!("═══ Commands ═══");
        println!();
        println!("  Natural Language Transactions:");
        println!("    \"transfer $50 from Checking to Savings\"");
        println!("    \"deposit $100 to Checking\"");
        println!("    \"withdraw $25 from Savings\"");
        println!();
        println!("  Account Management:");
        println!("    create <name>    Create a new account");
        println!("    accounts         List all accounts and balances");
        println!("    balance <name>   Show balance for one account");
        println!();
        println!("  Ledger:");
        println!("    history          Show all transactions");
        println!("    save             Save ledger to disk");
        println!();
        println!("  System:");
        println!("    help             Show this help message");
        println!("    quit             Save and exit");
        println!();
    }
}
