// ============================================================================
// Ledger Module — Public API (`src/ledger/mod.rs`)
// ============================================================================
//
// This is the module root for the ledger core. It declares the three sub-modules
// and re-exports their key types so consumers can write:
//
//   use ledger_guard::ledger::{Ledger, Transaction, Entry, LedgerError};
//
// instead of reaching into sub-modules directly.
// ============================================================================

/// Error types for every invariant violation the ledger can detect.
pub mod error;

/// Core data structures: `Entry` and `Transaction`.
pub mod types;

/// The stateful `Ledger` with balance cache and event log.
#[allow(clippy::module_inception)]
pub mod ledger;

// ─── Re-exports ────────────────────────────────────────────────────
// These bring the key types up to the `ledger` module level so users
// don't need to know about the internal file layout.
pub use error::LedgerError;
pub use types::{Entry, Transaction};
pub use ledger::Ledger;
