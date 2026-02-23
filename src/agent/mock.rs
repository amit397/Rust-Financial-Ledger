// ============================================================================
// Mock Agent — Deterministic Regex Parser (`src/agent/mock.rs`)
// ============================================================================
//
// The mock agent is a rule-based parser that handles a subset of natural
// language financial commands. It exists for two reasons:
//
//   1. REVIEWER FRICTION: Not every reviewer has 8 GB of RAM or wants to
//      download a 2 GB model. `cargo run -- --mock` lets them evaluate
//      the entire safety pipeline with zero setup beyond `cargo build`.
//
//   2. PIPELINE VALIDATION: The mock produces proposals through the SAME
//      untrusted-input path as a real LLM. The ledger has zero awareness
//      of whether its input came from a model or the mock. This guarantees
//      the safety demonstration works regardless of model availability.
//
// SUPPORTED PATTERNS:
// ───────────────────
// The mock understands these natural language patterns (case-insensitive):
//
//   Transfer patterns:
//     "transfer $50 from Checking to Savings"
//     "move $50 from Checking to Savings"
//     "pay $50 from Checking to Savings"
//     "send $50 from Checking to Savings"
//
//   Deposit patterns:
//     "deposit $100 to Savings"
//     "add $100 to Savings"
//
//   Withdrawal patterns:
//     "withdraw $75 from Checking"
//     "take $75 from Checking"
//
// All amounts are parsed as dollar values and converted to cents internally.
// The mock handles decimal amounts ("$50.25" → 5025 cents).
//
// WHY REGEX INSTEAD OF AN NLP LIBRARY?
// ─────────────────────────────────────
// The mock agent's purpose is to be simple and dependency-free. A regex
// parser is ~50 lines, has zero runtime dependencies, and is deterministic.
// An NLP library would add complexity without supporting the core thesis
// (which is about the ledger's safety, not the agent's intelligence).
// ============================================================================

use regex::Regex;

use super::{Agent, AgentError, AgentProposal};
use crate::ledger::Entry;

/// A deterministic, regex-based agent for testing and demos.
///
/// Implements the [`Agent`] trait with pattern-matching rules instead of
/// LLM inference. Same interface, same output format, same validation
/// pipeline — but with ~100% parse rate on recognized patterns.
pub struct MockAgent {
    /// Compiled regex for "transfer/move/pay/send $X from A to B"
    transfer_pattern: Regex,

    /// Compiled regex for "deposit/add $X to A"
    deposit_pattern: Regex,

    /// Compiled regex for "withdraw/take $X from A"
    withdraw_pattern: Regex,
}

impl Default for MockAgent {
    fn default() -> Self {
        Self::new()
    }
}

impl MockAgent {
    /// Create a new mock agent with pre-compiled regex patterns.
    ///
    /// Regex compilation is expensive relative to matching, so we compile
    /// once at construction time and reuse the compiled patterns for every
    /// `propose()` call.
    ///
    /// # Regex Breakdown
    ///
    /// The transfer pattern: `(?i)(transfer|move|pay|send)\s+\$(\d+(?:\.\d{1,2})?)\s+from\s+(\w+)\s+to\s+(\w+)`
    ///
    /// - `(?i)` — case-insensitive matching
    /// - `(transfer|move|pay|send)` — capture group 1: the verb
    /// - `\s+` — one or more whitespace characters
    /// - `\$` — literal dollar sign
    /// - `(\d+(?:\.\d{1,2})?)` — capture group 2: dollar amount (optional cents)
    ///   - `\d+` — one or more digits (whole dollars)
    ///   - `(?:\.\d{1,2})?` — optional: a dot followed by 1-2 digits (cents)
    /// - `\s+from\s+` — "from" surrounded by whitespace
    /// - `(\w+)` — capture group 3: source account name (word characters)
    /// - `\s+to\s+` — "to" surrounded by whitespace
    /// - `(\w+)` — capture group 4: destination account name
    pub fn new() -> MockAgent {
        MockAgent {
            transfer_pattern: Regex::new(
                r"(?i)(?:transfer|move|pay|send)\s+\$(\d+(?:\.\d{1,2})?)\s+from\s+(\w+)\s+to\s+(\w+)"
            ).expect("Transfer regex should compile"),

            deposit_pattern: Regex::new(
                r"(?i)(?:deposit|add)\s+\$(\d+(?:\.\d{1,2})?)\s+to\s+(\w+)"
            ).expect("Deposit regex should compile"),

            withdraw_pattern: Regex::new(
                r"(?i)(?:withdraw|take)\s+\$(\d+(?:\.\d{1,2})?)\s+from\s+(\w+)"
            ).expect("Withdraw regex should compile"),
        }
    }

    /// Parse a dollar amount string into cents (i64).
    ///
    /// Examples:
    /// - "50" → 5000
    /// - "50.25" → 5025
    /// - "0.99" → 99
    /// - "1000000" → 100000000
    ///
    /// # Why This Conversion?
    ///
    /// The user types dollar amounts ("$50.25"), but the ledger works in
    /// cents (5025). This function bridges the gap. It's careful to avoid
    /// floating-point: instead of parsing "50.25" as f64 and multiplying
    /// by 100 (which could introduce rounding errors), it splits on the
    /// decimal point and does integer arithmetic.
    fn parse_amount(amount_str: &str) -> Result<i64, AgentError> {
        // Split on decimal point
        let parts: Vec<&str> = amount_str.split('.').collect();

        match parts.len() {
            // No decimal: "50" → 5000 cents
            1 => {
                let dollars: i64 = parts[0].parse().map_err(|_| {
                    AgentError::ParseFailure(format!("Invalid amount: '{}'", amount_str))
                })?;
                Ok(dollars * 100)
            }
            // Has decimal: "50.25" → 50 * 100 + 25 = 5025 cents
            2 => {
                let dollars: i64 = parts[0].parse().map_err(|_| {
                    AgentError::ParseFailure(format!("Invalid dollar part: '{}'", parts[0]))
                })?;

                // Pad cents to always be 2 digits: "5" → "50", "25" → "25"
                let cents_str = format!("{:0<2}", parts[1]);
                let cents: i64 = cents_str[..2].parse().map_err(|_| {
                    AgentError::ParseFailure(format!("Invalid cents part: '{}'", parts[1]))
                })?;

                Ok(dollars * 100 + cents)
            }
            // Multiple decimals: "50.25.30" → error
            _ => Err(AgentError::ParseFailure(
                format!("Invalid amount format: '{}'", amount_str),
            )),
        }
    }
}

impl Agent for MockAgent {
    /// Parse natural language input into a transaction proposal.
    ///
    /// Tries each regex pattern in order:
    /// 1. Transfer: "transfer $X from A to B"
    /// 2. Deposit: "deposit $X to A"
    /// 3. Withdraw: "withdraw $X from A"
    ///
    /// If no pattern matches, returns `AgentError::ParseFailure`.
    ///
    /// # Processing Pipeline
    ///
    /// ```text
    /// User Input → MockAgent::propose() → AgentProposal
    ///                                          │
    ///                                          ▼
    ///                                    Transaction::new()  ← construction invariants
    ///                                          │
    ///                                          ▼
    ///                                    Ledger::apply()     ← stateful invariants
    ///                                          │
    ///                                          ▼
    ///                                    ✅ Committed or ❌ Rejected
    /// ```
    ///
    /// The mock is the first step, but the ledger doesn't trust it.
    /// Even if the mock produces a perfectly parsed proposal, the ledger
    /// still validates everything. This is the whole point of the project.
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError> {
        // ── Reject empty input early ────────────────────────────────
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(AgentError::EmptyInput);
        }

        // ── Try Transfer pattern ────────────────────────────────────
        //
        // "transfer $50 from Checking to Savings"
        //  → Debit Checking $50, Credit Savings $50
        //
        // In double-entry terms:
        //   Entry 1: Checking → -5000 (money leaves)
        //   Entry 2: Savings  → +5000 (money arrives)
        //   Sum: 0 ✓
        if let Some(caps) = self.transfer_pattern.captures(trimmed) {
            let amount = Self::parse_amount(&caps[1])?;
            let from_account = caps[2].to_string();
            let to_account = caps[3].to_string();

            return Ok(AgentProposal {
                description: format!(
                    "Transfer ${:.2} from {} to {}",
                    amount as f64 / 100.0,
                    from_account,
                    to_account
                ),
                entries: vec![
                    Entry {
                        account_id: from_account,
                        amount: -amount, // Debit (money leaves source)
                    },
                    Entry {
                        account_id: to_account,
                        amount, // Credit (money arrives at destination)
                    },
                ],
            });
        }

        // ── Try Deposit pattern ─────────────────────────────────────
        //
        // "deposit $100 to Savings"
        // → Credit Savings $100, Debit External $100
        //
        // In double-entry, money can't appear from nowhere. We use an
        // "External" account to represent money flowing in from outside
        // the system (e.g., a bank transfer, cash deposit).
        if let Some(caps) = self.deposit_pattern.captures(trimmed) {
            let amount = Self::parse_amount(&caps[1])?;
            let to_account = caps[2].to_string();

            return Ok(AgentProposal {
                description: format!(
                    "Deposit ${:.2} to {}",
                    amount as f64 / 100.0,
                    to_account
                ),
                entries: vec![
                    Entry {
                        account_id: "External".to_string(),
                        amount: -amount, // Debit External (money leaves outside world)
                    },
                    Entry {
                        account_id: to_account,
                        amount, // Credit target (money arrives)
                    },
                ],
            });
        }

        // ── Try Withdraw pattern ────────────────────────────────────
        //
        // "withdraw $75 from Checking"
        // → Debit Checking $75, Credit External $75
        //
        // Mirror of deposit: money flows out of an account and into
        // the external world.
        if let Some(caps) = self.withdraw_pattern.captures(trimmed) {
            let amount = Self::parse_amount(&caps[1])?;
            let from_account = caps[2].to_string();

            return Ok(AgentProposal {
                description: format!(
                    "Withdraw ${:.2} from {}",
                    amount as f64 / 100.0,
                    from_account
                ),
                entries: vec![
                    Entry {
                        account_id: from_account,
                        amount: -amount, // Debit source (money leaves)
                    },
                    Entry {
                        account_id: "External".to_string(),
                        amount, // Credit External (money goes to outside world)
                    },
                ],
            });
        }

        // ── No pattern matched ──────────────────────────────────────
        //
        // If we get here, the input didn't match any recognized command.
        // A real LLM would try harder (few-shot examples, chain-of-thought).
        // The mock just reports failure honestly.
        Err(AgentError::ParseFailure(
            "Unrecognized command. Try:\n  \
             • \"transfer $50 from Checking to Savings\"\n  \
             • \"deposit $100 to Savings\"\n  \
             • \"withdraw $25 from Checking\"".to_string()
        ))
    }

    fn name(&self) -> &str {
        "MockAgent (deterministic regex parser)"
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> MockAgent {
        MockAgent::new()
    }

    // ── Transfer patterns ───────────────────────────────────────────

    #[test]
    fn test_transfer_basic() {
        let proposal = agent().propose("transfer $50 from Checking to Savings").unwrap();
        assert_eq!(proposal.entries.len(), 2);
        assert_eq!(proposal.entries[0].account_id, "Checking");
        assert_eq!(proposal.entries[0].amount, -5000); // debit
        assert_eq!(proposal.entries[1].account_id, "Savings");
        assert_eq!(proposal.entries[1].amount, 5000); // credit
    }

    #[test]
    fn test_transfer_with_cents() {
        let proposal = agent().propose("transfer $50.25 from Checking to Savings").unwrap();
        assert_eq!(proposal.entries[0].amount, -5025);
        assert_eq!(proposal.entries[1].amount, 5025);
    }

    #[test]
    fn test_transfer_case_insensitive() {
        let proposal = agent().propose("TRANSFER $100 FROM checking TO savings").unwrap();
        assert_eq!(proposal.entries[0].amount, -10000);
        assert!(proposal.description.contains("100.00"));
    }

    #[test]
    fn test_move_synonym() {
        let proposal = agent().propose("move $25 from Checking to Savings").unwrap();
        assert_eq!(proposal.entries[0].amount, -2500);
    }

    #[test]
    fn test_pay_synonym() {
        let proposal = agent().propose("pay $75 from Checking to Vendor").unwrap();
        assert_eq!(proposal.entries[0].account_id, "Checking");
        assert_eq!(proposal.entries[1].account_id, "Vendor");
    }

    #[test]
    fn test_send_synonym() {
        let proposal = agent().propose("send $10 from Savings to Checking").unwrap();
        assert_eq!(proposal.entries[0].account_id, "Savings");
        assert_eq!(proposal.entries[1].account_id, "Checking");
    }

    // ── Deposit patterns ────────────────────────────────────────────

    #[test]
    fn test_deposit_basic() {
        let proposal = agent().propose("deposit $100 to Savings").unwrap();
        assert_eq!(proposal.entries.len(), 2);
        // External debited (money comes from outside)
        assert_eq!(proposal.entries[0].account_id, "External");
        assert_eq!(proposal.entries[0].amount, -10000);
        // Target credited
        assert_eq!(proposal.entries[1].account_id, "Savings");
        assert_eq!(proposal.entries[1].amount, 10000);
    }

    #[test]
    fn test_add_synonym() {
        let proposal = agent().propose("add $50 to Checking").unwrap();
        assert_eq!(proposal.entries[1].account_id, "Checking");
        assert_eq!(proposal.entries[1].amount, 5000);
    }

    // ── Withdraw patterns ───────────────────────────────────────────

    #[test]
    fn test_withdraw_basic() {
        let proposal = agent().propose("withdraw $75 from Checking").unwrap();
        assert_eq!(proposal.entries.len(), 2);
        // Source debited
        assert_eq!(proposal.entries[0].account_id, "Checking");
        assert_eq!(proposal.entries[0].amount, -7500);
        // External credited (money goes to outside)
        assert_eq!(proposal.entries[1].account_id, "External");
        assert_eq!(proposal.entries[1].amount, 7500);
    }

    #[test]
    fn test_take_synonym() {
        let proposal = agent().propose("take $20 from Savings").unwrap();
        assert_eq!(proposal.entries[0].account_id, "Savings");
        assert_eq!(proposal.entries[0].amount, -2000);
    }

    // ── Error cases ─────────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        let result = agent().propose("");
        assert!(matches!(result, Err(AgentError::EmptyInput)));
    }

    #[test]
    fn test_whitespace_only_input() {
        let result = agent().propose("   \t\n  ");
        assert!(matches!(result, Err(AgentError::EmptyInput)));
    }

    #[test]
    fn test_unrecognized_command() {
        let result = agent().propose("what's the weather?");
        assert!(matches!(result, Err(AgentError::ParseFailure(_))));
    }

    #[test]
    fn test_incomplete_transfer() {
        let result = agent().propose("transfer $50 from Checking");
        assert!(matches!(result, Err(AgentError::ParseFailure(_))));
    }

    // ── Amount parsing edge cases ───────────────────────────────────

    #[test]
    fn test_single_cent_decimal() {
        // "$50.5" should be parsed as $50.50 = 5050 cents
        let proposal = agent().propose("transfer $50.5 from A to B").unwrap();
        assert_eq!(proposal.entries[0].amount, -5050);
    }

    #[test]
    fn test_large_amount() {
        let proposal = agent().propose("transfer $1000000 from A to B").unwrap();
        assert_eq!(proposal.entries[0].amount, -100000000); // $1M in cents
    }

    #[test]
    fn test_one_dollar() {
        let proposal = agent().propose("transfer $1 from A to B").unwrap();
        assert_eq!(proposal.entries[0].amount, -100);
    }

    // ── Agent trait ─────────────────────────────────────────────────

    #[test]
    fn test_agent_name() {
        let mock = MockAgent::new();
        assert!(mock.name().contains("Mock"));
    }

    #[test]
    fn test_agent_trait_object() {
        // Verify MockAgent works as a trait object (Box<dyn Agent>)
        let agent: Box<dyn Agent> = Box::new(MockAgent::new());
        let result = agent.propose("transfer $10 from A to B");
        assert!(result.is_ok());
    }
}
