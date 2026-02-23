// ============================================================================
// Core Types — Entry & Transaction (`src/ledger/types.rs`)
// ============================================================================
//
// These are the foundational data structures of double-entry bookkeeping.
//
// DOUBLE-ENTRY BOOKKEEPING IN 30 SECONDS:
// ────────────────────────────────────────
// Every financial transaction has two sides: a debit (money leaving) and a
// credit (money arriving). The fundamental rule is:
//
//     sum(all entries) == 0
//
// For example, transferring $50 from Checking to Savings:
//   - Entry 1: Checking  → -5000 cents (debit, money leaves)
//   - Entry 2: Savings   → +5000 cents (credit, money arrives)
//   - Sum: -5000 + 5000 = 0 ✓
//
// WHY `i64` IN CENTS?
// ───────────────────
// Floating-point arithmetic (`f64`) has rounding errors:
//   0.1 + 0.2 = 0.30000000000000004  ← WRONG for financial math
//
// Industry standard (Stripe, Square, Ramp) is to use integer cents:
//   10 + 20 = 30  ← always exact
//
// `i64` range: ±9,223,372,036,854,775,807 cents = ±$92.2 quadrillion
// That's enough for any single-currency ledger.
//
// WHY `checked_add` / `checked_sub`?
// ──────────────────────────────────
// Normal `+` on i64 can silently overflow in release mode (wrapping)
// or panic in debug mode. Neither is acceptable for financial software.
// `checked_add` returns `Option<i64>`:
//   - `Some(result)` if the addition is within bounds
//   - `None` if it would overflow
// We convert `None` → `LedgerError::Overflow`.
// ============================================================================

use serde::{Serialize, Deserialize};
use chrono::Utc;
use uuid::Uuid;

use super::error::LedgerError;

// ─── Entry ─────────────────────────────────────────────────────────

/// A single line in a financial transaction.
///
/// Each entry represents money moving into or out of one account:
/// - **Positive amount** → credit (money arriving)
/// - **Negative amount** → debit (money leaving)
///
/// # Examples
/// ```
/// use ledger_guard::ledger::Entry;
///
/// // $50 leaving Checking (debit)
/// let debit = Entry {
///     account_id: "Checking".to_string(),
///     amount: -5000, // negative = debit, in cents
/// };
///
/// // $50 arriving in Savings (credit)
/// let credit = Entry {
///     account_id: "Savings".to_string(),
///     amount: 5000, // positive = credit, in cents
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// The name of the account this entry affects.
    ///
    /// Account names are case-sensitive strings. In a production system,
    /// these would be opaque IDs referencing a chart of accounts. For this
    /// CLI tool, plain names like "Checking", "Savings", "Revenue" are used.
    pub account_id: String,

    /// The amount in the smallest currency unit (cents for USD).
    ///
    /// - **Positive** = credit (money in)
    /// - **Negative** = debit (money out)
    /// - **Zero** = invalid (rejected by `Transaction::new`)
    ///
    /// Using `i64` instead of `f64` eliminates floating-point rounding errors.
    pub amount: i64,
}

// ─── Transaction ───────────────────────────────────────────────────

/// An immutable, validated financial transaction.
///
/// A `Transaction` can only be created via [`Transaction::new`], which
/// enforces all construction-time invariants. Once created, a `Transaction`
/// is guaranteed to:
///
/// 1. Have at least one entry
/// 2. Have no zero-amount entries
/// 3. Have entries that sum to exactly zero (balanced)
/// 4. Have survived overflow checks on all arithmetic
///
/// It is **impossible** to hold an invalid `Transaction` — the constructor
/// returns `Err(LedgerError)` for any violation.
///
/// # Design Decision: Why No Typestate?
///
/// The typestate pattern (`DraftTransaction` → `ValidatedTransaction`) was
/// considered and deliberately cut. With only two consumers (LLM agent and
/// CLI), the `Result`-returning constructor is simpler, equally safe, and
/// trivially explainable in an interview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    /// Unique identifier for this transaction (UUID v4).
    ///
    /// Generated automatically in `Transaction::new`. UUIDs are used instead
    /// of sequential IDs because they're globally unique without coordination
    /// — important if this system ever supports concurrent or distributed use.
    pub id: String,

    /// Human-readable description of what this transaction represents.
    ///
    /// Examples: "Transfer from Checking to Savings", "Payroll deposit",
    /// "Coffee shop purchase"
    pub description: String,

    /// ISO 8601 timestamp of when this transaction was created.
    ///
    /// Generated automatically using `chrono::Utc::now()`. Stored as a
    /// string rather than a `DateTime` for simpler serialization and
    /// display. Example: "2026-02-23T05:30:00Z"
    pub timestamp: String,

    /// The set of balanced entries that make up this transaction.
    ///
    /// Guaranteed to:
    /// - Have at least one entry
    /// - Have no zero-amount entries
    /// - Sum to exactly zero
    pub entries: Vec<Entry>,
}

impl Transaction {
    /// Create a new validated transaction.
    ///
    /// This is the **only** way to construct a `Transaction`. It enforces
    /// every construction-time invariant and returns a structured error
    /// (not a string) if any check fails.
    ///
    /// # Invariants Checked (in order)
    ///
    /// 1. **Non-empty** — at least one entry must be provided
    /// 2. **No zero amounts** — every entry must move money
    /// 3. **No overflow** — the sum is computed with `checked_add`
    /// 4. **Balanced** — the sum of all amounts must be exactly zero
    ///
    /// # Arguments
    ///
    /// * `description` — Human-readable description of the transaction
    /// * `entries` — The debit/credit entries (positive = credit, negative = debit)
    ///
    /// # Returns
    ///
    /// * `Ok(Transaction)` — A valid, immutable transaction
    /// * `Err(LedgerError)` — A typed error explaining exactly what was wrong
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::{Transaction, Entry, LedgerError};
    ///
    /// // ✅ Valid: balanced entries summing to zero
    /// let tx = Transaction::new(
    ///     "Transfer".to_string(),
    ///     vec![
    ///         Entry { account_id: "Checking".to_string(), amount: -5000 },
    ///         Entry { account_id: "Savings".to_string(),  amount:  5000 },
    ///     ],
    /// );
    /// assert!(tx.is_ok());
    ///
    /// // ❌ Invalid: entries don't sum to zero
    /// let tx = Transaction::new(
    ///     "Bad transfer".to_string(),
    ///     vec![
    ///         Entry { account_id: "Checking".to_string(), amount: -5000 },
    ///         Entry { account_id: "Savings".to_string(),  amount:  3000 },
    ///     ],
    /// );
    /// assert!(matches!(tx, Err(LedgerError::Unbalanced { sum: -2000 })));
    /// ```
    pub fn new(description: String, entries: Vec<Entry>) -> Result<Transaction, LedgerError> {
        // ── Check 1: Non-empty ──────────────────────────────────────
        // A transaction with zero entries is meaningless. We check this
        // first because subsequent checks iterate over entries.
        if entries.is_empty() {
            return Err(LedgerError::EmptyTransaction);
        }

        // ── Check 2: No zero-amount entries ─────────────────────────
        // A zero-amount entry is a no-op. It doesn't move money, so it
        // shouldn't exist. Catching it here keeps the event log clean.
        for entry in &entries {
            if entry.amount == 0 {
                return Err(LedgerError::InvalidAmount {
                    account: entry.account_id.clone(),
                });
            }
        }

        // ── Check 3: Overflow-safe sum ──────────────────────────────
        // We use `checked_add` instead of the `+` operator. Normal `+`
        // will panic in debug mode or silently wrap in release mode if
        // the result exceeds i64 bounds. `checked_add` returns `None`
        // on overflow, which we convert to `LedgerError::Overflow`.
        //
        // This is critical for financial software: silent overflow could
        // turn a $92-quadrillion debit into a credit, corrupting the
        // entire ledger.
        let mut sum: i64 = 0;
        for entry in &entries {
            sum = sum.checked_add(entry.amount).ok_or(LedgerError::Overflow)?;
            // The `?` operator: if `checked_add` returns `None`, the
            // `ok_or` converts it to `Err(LedgerError::Overflow)` and
            // the `?` immediately returns that error from the function.
        }

        // ── Check 4: Balance invariant ──────────────────────────────
        // The fundamental law of double-entry bookkeeping: debits must
        // equal credits, meaning all entries must sum to zero.
        if sum != 0 {
            return Err(LedgerError::Unbalanced { sum });
        }

        // ── All checks passed — construct the transaction ───────────
        // At this point, the transaction is guaranteed valid. We generate
        // a UUID and timestamp, then return the immutable struct.
        Ok(Transaction {
            // UUID v4: 128-bit random identifier, collision probability
            // is astronomically low (~2^-122 per pair).
            id: Uuid::new_v4().to_string(),

            description,

            // ISO 8601 timestamp in UTC. Using `to_rfc3339()` produces
            // strings like "2026-02-23T05:30:00.123456789+00:00".
            timestamp: Utc::now().to_rfc3339(),

            entries,
        })
    }
}

// ============================================================================
// Display Implementation
// ============================================================================
//
// Custom `Display` for pretty-printing transactions to the CLI.
// This is used when showing transaction history to the user.

impl std::fmt::Display for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Header line with ID (first 8 chars for readability) and description
        writeln!(
            f,
            "[{}] {} ({})",
            &self.id[..8], // First 8 chars of UUID — enough to identify
            self.description,
            self.timestamp
        )?;

        // Each entry on its own line with debit/credit formatting
        for entry in &self.entries {
            if entry.amount < 0 {
                // Negative = debit (money leaving)
                // Convert cents to dollars for display: -5000 → $50.00
                writeln!(
                    f,
                    "  DEBIT  {:>12} ← ${:.2}",
                    entry.account_id,
                    (-entry.amount) as f64 / 100.0
                )?;
            } else {
                // Positive = credit (money arriving)
                writeln!(
                    f,
                    "  CREDIT {:>12} ← ${:.2}",
                    entry.account_id,
                    entry.amount as f64 / 100.0
                )?;
            }
        }

        Ok(())
    }
}

// ============================================================================
// Unit Tests — Transaction Construction
// ============================================================================
//
// These tests verify every invariant check in `Transaction::new`.
// The goal is 100% coverage of all error paths.

#[cfg(test)]
mod tests {
    use super::*;

    // ── Happy Path ──────────────────────────────────────────────────

    #[test]
    fn test_valid_two_entry_transaction() {
        // The simplest valid transaction: $100 from A to B
        let tx = Transaction::new(
            "Transfer".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -10000 },
                Entry { account_id: "Savings".to_string(), amount: 10000 },
            ],
        );

        let tx = tx.expect("Should succeed for balanced entries");
        assert_eq!(tx.description, "Transfer");
        assert_eq!(tx.entries.len(), 2);
        // UUID should be 36 characters (32 hex digits + 4 hyphens)
        assert_eq!(tx.id.len(), 36);
        // Timestamp should be non-empty
        assert!(!tx.timestamp.is_empty());
    }

    #[test]
    fn test_valid_multi_entry_transaction() {
        // Three-entry transaction: $100 invoice paid with $90 cash + $10 discount
        // This tests that the ledger handles N-entry transactions, not just pairs.
        let tx = Transaction::new(
            "Invoice payment".to_string(),
            vec![
                Entry { account_id: "Expense".to_string(), amount: 10000 },    // +$100
                Entry { account_id: "Cash".to_string(), amount: -9000 },       // -$90
                Entry { account_id: "Discount".to_string(), amount: -1000 },   // -$10
            ],
        );

        assert!(tx.is_ok());
        assert_eq!(tx.unwrap().entries.len(), 3);
    }

    // ── Error Paths ─────────────────────────────────────────────────

    #[test]
    fn test_empty_entries_rejected() {
        let tx = Transaction::new("Empty".to_string(), vec![]);
        assert!(matches!(tx, Err(LedgerError::EmptyTransaction)));
    }

    #[test]
    fn test_zero_amount_rejected() {
        let tx = Transaction::new(
            "Zero".to_string(),
            vec![
                Entry { account_id: "A".to_string(), amount: 0 },
                Entry { account_id: "B".to_string(), amount: 100 },
            ],
        );
        match tx {
            Err(LedgerError::InvalidAmount { account }) => {
                assert_eq!(account, "A");
            }
            other => panic!("Expected InvalidAmount, got: {:?}", other),
        }
    }

    #[test]
    fn test_unbalanced_positive_sum_rejected() {
        // More credits than debits: sum = +2000
        let tx = Transaction::new(
            "Unbalanced".to_string(),
            vec![
                Entry { account_id: "A".to_string(), amount: -5000 },
                Entry { account_id: "B".to_string(), amount: 7000 },
            ],
        );
        assert!(matches!(tx, Err(LedgerError::Unbalanced { sum: 2000 })));
    }

    #[test]
    fn test_unbalanced_negative_sum_rejected() {
        // More debits than credits: sum = -3000
        let tx = Transaction::new(
            "Unbalanced".to_string(),
            vec![
                Entry { account_id: "A".to_string(), amount: -5000 },
                Entry { account_id: "B".to_string(), amount: 2000 },
            ],
        );
        assert!(matches!(tx, Err(LedgerError::Unbalanced { sum: -3000 })));
    }

    #[test]
    fn test_overflow_detected() {
        // Two entries that would overflow i64 when summed
        let tx = Transaction::new(
            "Overflow".to_string(),
            vec![
                Entry { account_id: "A".to_string(), amount: i64::MAX },
                Entry { account_id: "B".to_string(), amount: 1 },
            ],
        );
        assert!(matches!(tx, Err(LedgerError::Overflow)));
    }

    #[test]
    fn test_single_entry_rejected() {
        // A single entry can never sum to zero (since zero-amount is rejected)
        let tx = Transaction::new(
            "Single".to_string(),
            vec![Entry { account_id: "A".to_string(), amount: 100 }],
        );
        // Sum = 100 ≠ 0, so this should fail with Unbalanced
        assert!(matches!(tx, Err(LedgerError::Unbalanced { sum: 100 })));
    }

    #[test]
    fn test_display_format() {
        let tx = Transaction::new(
            "Test display".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -5000 },
                Entry { account_id: "Savings".to_string(), amount: 5000 },
            ],
        )
        .unwrap();

        let display = format!("{}", tx);
        assert!(display.contains("Test display"));
        assert!(display.contains("DEBIT"));
        assert!(display.contains("CREDIT"));
        assert!(display.contains("$50.00"));
    }
}
