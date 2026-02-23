// ============================================================================
// LedgerGuard — Library Root (`src/lib.rs`)
// ============================================================================
//
// This is the library entry point. It declares and re-exports the four core
// modules that make up LedgerGuard:
//
//   1. `ledger`      — The safety-critical core: Transaction, Ledger, LedgerError
//   2. `agent`       — Agent trait + MockAgent (deterministic, regex-based)
//   3. `persistence` — Atomic file I/O (temp file → rename)
//   4. `cli`         — Interactive REPL with rustyline
//
// By keeping this file minimal (just module declarations), we maintain a clean
// separation of concerns. Each module is self-contained and independently testable.
// ============================================================================

/// The safety-critical financial ledger core.
///
/// Contains:
/// - [`ledger::LedgerError`] — Typed error enum for every invariant violation
/// - [`ledger::Entry`] — A single debit or credit line
/// - [`ledger::Transaction`] — An immutable, validated set of balanced entries
/// - [`ledger::Ledger`] — The stateful ledger with balance cache and event log
pub mod ledger;

/// Agent abstraction layer.
///
/// Contains:
/// - [`agent::Agent`] — Trait that any agent (LLM or mock) must implement
/// - [`agent::AgentProposal`] — The structured output an agent produces
/// - [`agent::MockAgent`] — A deterministic regex-based agent for testing
pub mod agent;

/// Durable persistence with crash safety.
///
/// Contains:
/// - [`persistence::save`] — Serialize ledger state to JSON with atomic write
/// - [`persistence::load`] — Deserialize and re-validate by replaying transactions
pub mod persistence;

/// Interactive command-line interface.
///
/// Contains:
/// - [`cli::Cli`] — The REPL loop that ties agent, ledger, and display together
pub mod cli;
