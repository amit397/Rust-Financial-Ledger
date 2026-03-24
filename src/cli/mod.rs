use crate::agent::{Agent, AgentError};
use crate::ledger::LedgerError;
use crate::ledger::{Ledger, Transaction};
use rustyline::{DefaultEditor, error::ReadlineError};

pub struct Cli {
    ledger: Ledger,
    agent: Box<dyn Agent>,
    data_path: String,
}

impl Cli {
    pub fn new(agent: Box<dyn Agent>, data_path: &str, default_accounts: &[String]) -> Self {
        let mut ledger = Ledger::new();
        
        for acc in default_accounts {
            match ledger.create_account(acc) {
                Ok(()) | Err(LedgerError::DuplicateAccount { .. }) => {}
                Err(e) => eprintln!("Warning: failed to create account '{}': {}", acc, e),
            }
        }

        match crate::persistence::load(data_path) {
            Ok(history) => {
                for tx in history {
                    if let Err(e) = ledger.apply(tx) {
                        eprintln!("Failed to load transaction: {}", e);
                    }
                }
            }
            Err(e) => eprintln!("Warning: Failed to load previous ledger data: {}", e),
        }
        
        Self {
            ledger,
            agent,
            data_path: data_path.to_string(),
        }
    }

    pub fn run(&mut self) {
        println!("LedgerGuard — AI-Safe Financial Ledger");
        println!("Type a transaction in natural language, or 'help' for commands.");
        let mut accounts_str = String::new();
        for (acc, bal) in self.ledger.accounts() {
            accounts_str.push_str(&format!("{} (${:.2}), ", acc, bal as f64 / 100.0));
        }
        println!("Accounts: {}", accounts_str.trim_end_matches(", "));

        let mut rl = DefaultEditor::new().unwrap();
        loop {
            let readline = rl.readline(">> ");
            match readline {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    let _ = rl.add_history_entry(trimmed);

                    match trimmed {
                        "quit" | "exit" => {
                            if let Err(e) = crate::persistence::save(&self.data_path, self.ledger.history()) {
                                eprintln!("Failed to save ledger: {}", e);
                            }
                            break;
                        }
                        "help" => {
                            println!("Commands:");
                            println!("  <natural language>  Process a transaction");
                            println!("  balance <account>   Show account balance");
                            println!("  accounts            List all accounts");
                            println!("  history             Show last 10 transactions");
                            println!("  help                Show this message");
                            println!("  quit / exit         Save and exit");
                        }
                        "accounts" => {
                            for (acc, bal) in self.ledger.accounts() {
                                println!("{:15} | ${:.2}", acc, bal as f64 / 100.0);
                            }
                        }
                        "history" => {
                            let history = self.ledger.history();
                            let start = if history.len() > 10 { history.len() - 10 } else { 0 };
                            for tx in &history[start..] {
                                println!("[{}] {}", tx.id, tx.description);
                                for entry in &tx.entries {
                                    println!("  {:15} | {}", entry.account.0, format_amount(entry.amount));
                                }
                            }
                        }
                        input if input.starts_with("balance ") => {
                            let account_query = input.trim_start_matches("balance ").trim().to_lowercase();
                            let mut found = false;
                            for (acc, bal) in self.ledger.accounts() {
                                if acc.to_lowercase() == account_query {
                                    println!("${:.2}", bal as f64 / 100.0);
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                println!("Account not found");
                            }
                        }
                        input => {
                            self.process_transaction(input);
                        }
                    }
                }
                Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                    if let Err(e) = crate::persistence::save(&self.data_path, self.ledger.history()) {
                        eprintln!("Failed to save ledger: {}", e);
                    }
                    break;
                }
                Err(err) => {
                    eprintln!("Readline error: {:?}", err);
                    if let Err(e) = crate::persistence::save(&self.data_path, self.ledger.history()) {
                        eprintln!("Failed to save ledger: {}", e);
                    }
                    break;
                }
            }
        }
    }

    fn process_transaction(&mut self, input: &str) {
        match self.agent.propose(input) {
            Ok(proposal) => {
                match Transaction::new(proposal.description.clone(), proposal.entries.clone()) {
                    Ok(tx) => {
                        match self.ledger.apply(tx) {
                            Ok(()) => {
                                println!("✅ Transaction committed\n   {}", proposal.description);
                            }
                            Err(crate::ledger::LedgerError::InsufficientFunds { account, available, requested }) => {
                                println!("❌ Transaction rejected: Insufficient funds");
                                println!("   Account:   {}", account);
                                println!("   Available: {}", format_amount(available));
                                println!("   Requested: {}", format_amount(requested));
                            }
                            Err(e) => {
                                println!("❌ Transaction rejected: {}", e);
                            }
                        }
                    }
                    Err(e) => println!("❌ Invalid proposal: {:?}", e),
                }
            }
            Err(AgentError::ParseFailure(_)) => println!("Model produced invalid output. Please rephrase."),
            Err(AgentError::Timeout) => println!("Model took too long. Please try again."),
            Err(AgentError::InferenceFailed(_)) => println!("Model failed unexpectedly. Please try again."),
        }
    }
}

fn format_amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    format!("{}${:.2}", sign, cents.abs() as f64 / 100.0)
}
