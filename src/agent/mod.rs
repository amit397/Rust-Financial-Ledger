// ============================================================================
// Agent Module — Trait & Mock Agent (`src/agent/mod.rs`)
// ============================================================================
//
// This module defines the Agent abstraction and implements the mock agent.
//
// ARCHITECTURE:
// ────────────
// The `Agent` trait defines a single method: `propose(input) → Result`.
// Any agent — whether a real LLM or a deterministic mock — must implement
// this trait. The CLI doesn't know (or care) which agent it's using.
//
// This is the Strategy pattern: the algorithm (how to parse natural language
// into a transaction proposal) is swapped at runtime based on the `--mock` flag.
//
// WHY A TRAIT?
// ────────────
// Using a trait instead of an enum gives us:
// 1. Open extension — adding a real LLM agent doesn't require modifying existing code
// 2. Clean testing — the mock agent has zero dependencies
// 3. Same pipeline — mock output flows through `Transaction::new` → `Ledger::apply`,
//    proving the safety layer works regardless of input source
//
// THE MOCK AGENT:
// ───────────────
// The mock agent uses regex to parse simple financial commands:
//   - "transfer $50 from Checking to Savings"
//   - "pay $25 from Checking to Vendor"
//   - "deposit $100 to Savings"
//   - "withdraw $75 from Checking"
//
// It's deterministic (same input → same output), which makes testing reliable.
// A real LLM would produce similar JSON proposals but with ~70% accuracy.
// The mock gives us ~100% accuracy on recognized patterns, which lets reviewers
// evaluate the ledger's safety guarantees without needing a 2 GB model download.
// ============================================================================

pub mod mock;

use std::fmt;

use crate::ledger::Entry;

// ─── Agent Error ───────────────────────────────────────────────────

/// Errors that can occur during agent proposal generation.
///
/// These are separate from `LedgerError` because they represent
/// agent failures (parsing, understanding), not ledger failures
/// (insufficient funds, unbalanced entries). The CLI handles them
/// differently — agent errors get "please rephrase" messages,
/// while ledger errors get structured rejection details.
#[derive(Debug, Clone)]
pub enum AgentError {
    /// The input couldn't be parsed into any recognized pattern.
    ///
    /// This is the most common error. The agent tried all its parsing
    /// rules and none matched. For the mock agent, this means the input
    /// didn't match any regex pattern. For a real LLM, this means the
    /// model produced invalid JSON.
    ParseFailure(String),

    /// The input was empty or whitespace-only.
    EmptyInput,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::ParseFailure(msg) => {
                write!(f, "Could not understand input: {}", msg)
            }
            AgentError::EmptyInput => {
                write!(f, "Empty input — please type a command or transaction")
            }
        }
    }
}

// ─── Agent Proposal ────────────────────────────────────────────────

/// A structured proposal from the agent.
///
/// This is what the agent produces after parsing natural language.
/// It contains the raw data needed to construct a `Transaction` via
/// `Transaction::new`. The proposal is **untrusted** — it will be
/// validated by the ledger before being committed.
///
/// Think of this as the "JSON proposal" from the architecture diagram.
/// Whether it came from a real LLM or the mock agent, the ledger
/// treats it identically.
#[derive(Debug, Clone)]
pub struct AgentProposal {
    /// Human-readable description of what this transaction does.
    ///
    /// Examples: "Transfer $50.00 from Checking to Savings"
    pub description: String,

    /// The debit/credit entries to be validated.
    ///
    /// These are passed directly to `Transaction::new` for validation.
    pub entries: Vec<Entry>,
}

// ─── Agent Trait ───────────────────────────────────────────────────

/// The agent interface that any LLM or mock must implement.
///
/// This trait has a single method: take natural language input and
/// produce a structured `AgentProposal`. The proposal is then
/// validated by the ledger.
///
/// # Design: Trait Object vs. Generic
///
/// We use `Box<dyn Agent>` (trait object) in the CLI rather than
/// generics. This is because the agent type is determined at runtime
/// (based on the `--mock` flag), and trait objects allow dynamic
/// dispatch. The performance cost is negligible (one vtable lookup
/// per user input — microseconds vs. the seconds an LLM would take).
pub trait Agent {
    /// Parse natural language input into a transaction proposal.
    ///
    /// # Arguments
    ///
    /// * `input` — The raw user input string
    ///
    /// # Returns
    ///
    /// * `Ok(AgentProposal)` — A parsed proposal ready for ledger validation
    /// * `Err(AgentError)` — The input couldn't be understood
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;

    /// Human-readable name of this agent (for display in the CLI).
    fn name(&self) -> &str;
}
