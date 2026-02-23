// ============================================================================
// LedgerError — Typed Error Enum (`src/ledger/error.rs`)
// ============================================================================
//
// Every invariant violation that the ledger can detect is encoded as a variant
// of this enum. Using `thiserror` gives us two things for free:
//
//   1. `impl Display` — human-readable error messages via #[error("...")]
//   2. `impl std::error::Error` — compatibility with Rust's error ecosystem
//
// WHY AN ENUM INSTEAD OF STRINGS?
// ────────────────────────────────
// Returning `Result<_, String>` (as the old WASM code did) makes it impossible
// for callers to programmatically handle different error cases. With a typed
// enum, the CLI can pattern-match on the error and display context-specific
// help messages. The benchmark system can categorize rejection reasons. And
// the compiler ensures we handle every case.
//
// DESIGN PRINCIPLE: If the ledger ever returns `Ok(...)`, the transaction is
// guaranteed valid. If it returns `Err(LedgerError::_)`, the error message
// tells you exactly what went wrong and gives you the data to fix it.
// ============================================================================

use thiserror::Error;

/// Every way a transaction or ledger operation can fail.
///
/// Each variant carries the data needed to produce a helpful error message.
/// The `#[error("...")]` attribute auto-generates `Display` implementations
/// so these can be printed directly to the user.
#[derive(Debug, Clone, Error)]
pub enum LedgerError {
    // ── Construction-time invariants (checked in Transaction::new) ──

    /// The sum of all entry amounts is not zero.
    ///
    /// Double-entry bookkeeping requires that every transaction balances:
    /// debits must equal credits. If `sum` is positive, more money was
    /// credited than debited (or vice versa for negative).
    #[error("Transaction unbalanced: entries sum to {sum} cents (must be 0)")]
    Unbalanced {
        /// The actual sum of all entries, in cents.
        sum: i64,
    },

    /// No entries were provided.
    ///
    /// A transaction with zero entries is meaningless — it moves no money
    /// between any accounts. We reject it at construction time rather than
    /// silently accepting a no-op.
    #[error("Transaction cannot have zero entries")]
    EmptyTransaction,

    /// An entry has an amount of zero cents.
    ///
    /// A zero-amount entry is a no-op that adds noise without moving money.
    /// We reject it to keep the event log clean and meaningful.
    #[error("Entry for account '{account}' has zero amount (every entry must move money)")]
    InvalidAmount {
        /// The account that had a zero-amount entry.
        account: String,
    },

    /// Arithmetic overflow detected during balance computation.
    ///
    /// All monetary arithmetic uses `checked_add` / `checked_sub`. If any
    /// operation would exceed `i64::MAX` or go below `i64::MIN`, we return
    /// this error instead of silently wrapping (which would corrupt balances).
    ///
    /// For context: `i64::MAX` in cents = $92,233,720,368,547,758.07
    /// That's 92 quadrillion dollars — enough for any single-currency ledger.
    #[error("Arithmetic overflow: transaction amounts exceed i64 bounds")]
    Overflow,

    // ── Stateful invariants (checked in Ledger::apply) ──────────────

    /// A debit targets an account that doesn't exist in the ledger.
    ///
    /// Credits to new accounts are allowed (they auto-create the account),
    /// but debits from non-existent accounts are always an error — you can't
    /// take money from an account that hasn't been created yet.
    #[error("Account '{account}' not found (cannot debit a non-existent account)")]
    AccountNotFound {
        /// The account name that was referenced but doesn't exist.
        account: String,
    },

    /// A debit would cause an account's balance to go negative.
    ///
    /// The ledger enforces a simple invariant: no account can have a
    /// negative balance. This prevents overdrafts and double-spending.
    #[error(
        "Insufficient funds: '{account}' has {available} cents, \
         but {requested} cents were requested"
    )]
    InsufficientFunds {
        /// The account being debited.
        account: String,
        /// The current balance of the account, in cents.
        available: i64,
        /// The (positive) amount that was requested for withdrawal.
        requested: i64,
    },

    /// An account with this name already exists.
    ///
    /// Returned by `Ledger::create_account` when the name is already taken.
    #[error("Account '{account}' already exists")]
    AccountAlreadyExists {
        /// The duplicate account name.
        account: String,
    },

    // ── Persistence errors ─────────────────────────────────────────

    /// An I/O error occurred during save or load.
    ///
    /// This wraps `std::io::Error` so callers can inspect the underlying
    /// OS error (permission denied, disk full, etc.).
    #[error("Persistence I/O error: {message}")]
    IoError {
        /// Human-readable description of what went wrong.
        message: String,
    },

    /// The saved data file is corrupted or contains invalid data.
    ///
    /// This can happen if someone manually edits the JSON file, or if a
    /// crash occurred before atomic write was implemented.
    #[error("Corrupted data file: {message}")]
    CorruptedData {
        /// Description of the corruption.
        message: String,
    },
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that each error variant produces a readable, informative message.
    /// This matters because these messages are displayed directly to users.
    #[test]
    fn test_error_display_messages() {
        // Unbalanced
        let err = LedgerError::Unbalanced { sum: 500 };
        assert!(err.to_string().contains("500 cents"));

        // EmptyTransaction
        let err = LedgerError::EmptyTransaction;
        assert!(err.to_string().contains("zero entries"));

        // InvalidAmount
        let err = LedgerError::InvalidAmount {
            account: "Checking".to_string(),
        };
        assert!(err.to_string().contains("Checking"));
        assert!(err.to_string().contains("zero amount"));

        // Overflow
        let err = LedgerError::Overflow;
        assert!(err.to_string().contains("overflow"));

        // AccountNotFound
        let err = LedgerError::AccountNotFound {
            account: "Savings".to_string(),
        };
        assert!(err.to_string().contains("Savings"));

        // InsufficientFunds
        let err = LedgerError::InsufficientFunds {
            account: "Checking".to_string(),
            available: 5000,
            requested: 10000,
        };
        assert!(err.to_string().contains("5000"));
        assert!(err.to_string().contains("10000"));

        // AccountAlreadyExists
        let err = LedgerError::AccountAlreadyExists {
            account: "Savings".to_string(),
        };
        assert!(err.to_string().contains("already exists"));

        // IoError
        let err = LedgerError::IoError {
            message: "disk full".to_string(),
        };
        assert!(err.to_string().contains("disk full"));

        // CorruptedData
        let err = LedgerError::CorruptedData {
            message: "invalid JSON".to_string(),
        };
        assert!(err.to_string().contains("invalid JSON"));
    }
}
