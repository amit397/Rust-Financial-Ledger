// ============================================================================
// Ledger — Stateful Validation Engine (`src/ledger/ledger.rs`)
// ============================================================================
//
// The `Ledger` struct is the heart of LedgerGuard. It maintains:
//
//   1. An **event log** (`Vec<Transaction>`) — the authoritative record of
//      every transaction that has ever been applied. This is the source of
//      truth that gets persisted to disk.
//
//   2. A **balance cache** (`HashMap<String, i64>`) — a derived view that
//      gives O(1) balance lookups. This is rebuilt from the event log on
//      startup and updated incrementally on each `apply()`.
//
// WHY TWO SOURCES OF TRUTH?
// ─────────────────────────
// The event log is append-only and easy to persist. But querying "what is
// the balance of account X?" would require replaying every transaction
// (O(n)). The balance cache makes this O(1), which matters for real-time
// validation (checking sufficient funds before committing).
//
// The risk of two sources of truth is divergence. We mitigate this with:
// - Property-based tests verifying cache-replay consistency
// - Replay on load (rebuilding cache from the event log)
//
// VALIDATION FLOW:
// ────────────────
// 1. User types "transfer $50 from Checking to Savings"
// 2. Agent produces a JSON proposal
// 3. `Transaction::new()` validates construction-time invariants (balanced, etc.)
// 4. `Ledger::apply()` validates stateful invariants (funds, account existence)
// 5. If step 3 or 4 fails → structured error returned, nothing mutated
// 6. If both pass → transaction committed, balance cache updated
//
// This two-layer defense is what makes the system bulletproof:
// - Layer 1 (`Transaction::new`) catches structural errors
// - Layer 2 (`Ledger::apply`) catches contextual errors
// ============================================================================

use std::collections::HashMap;

use super::error::LedgerError;
use super::types::Transaction;

/// The core financial ledger with stateful validation.
///
/// A `Ledger` enforces both construction-time invariants (via `Transaction::new`)
/// and stateful invariants (via `Ledger::apply`). Together, they guarantee that
/// no invalid transaction can ever be committed.
///
/// # Thread Safety
///
/// `Ledger` is `Send` (can be moved between threads) because all its fields
/// own their data. It is not `Sync` (not safe to share between threads without
/// a mutex) because `apply()` mutates internal state. For a concurrent system,
/// wrap it in `Arc<Mutex<Ledger>>`.
#[derive(Debug, Clone)]
pub struct Ledger {
    /// The append-only event log — every transaction ever committed.
    ///
    /// This is the authoritative record. The balance cache is derived from
    /// this. On save, this is what gets serialized to disk. On load, this
    /// is what gets replayed to rebuild the cache.
    transactions: Vec<Transaction>,

    /// O(1) balance lookup cache.
    ///
    /// Maps account names to their current balance in cents. Updated
    /// atomically (all-or-nothing) when a transaction is applied.
    ///
    /// Invariant: for every account, `balances[account]` equals the sum
    /// of all entry amounts for that account across all transactions.
    balances: HashMap<String, i64>,
}

impl Ledger {
    /// Create a new, empty ledger.
    ///
    /// The ledger starts with no accounts and no transactions. Accounts
    /// must be explicitly created with [`Ledger::create_account`] before
    /// they can be debited. (Credits to new accounts are rejected too —
    /// all accounts must be pre-registered.)
    ///
    /// # Example
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let ledger = Ledger::new();
    /// assert_eq!(ledger.transaction_count(), 0);
    /// ```
    pub fn new() -> Ledger {
        Ledger {
            transactions: Vec::new(),
            balances: HashMap::new(),
        }
    }

    /// Register a new account with a zero balance.
    ///
    /// Accounts must be created before they can participate in transactions.
    /// This explicit registration step prevents typos from silently creating
    /// new accounts (e.g., "Checkng" instead of "Checking").
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::AccountAlreadyExists` if the name is taken.
    ///
    /// # Example
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let mut ledger = Ledger::new();
    /// ledger.create_account("Checking".to_string()).unwrap();
    ///
    /// // Duplicate name is rejected
    /// let err = ledger.create_account("Checking".to_string());
    /// assert!(err.is_err());
    /// ```
    pub fn create_account(&mut self, name: String) -> Result<(), LedgerError> {
        // `entry` API on HashMap: if the key doesn't exist, we insert 0.
        // If it does exist, we return an error.
        if self.balances.contains_key(&name) {
            return Err(LedgerError::AccountAlreadyExists {
                account: name,
            });
        }

        // Insert with zero balance. The `insert` method returns the old
        // value if the key existed, but we already checked above.
        self.balances.insert(name, 0);
        Ok(())
    }

    /// Apply a validated transaction to the ledger.
    ///
    /// This is the second layer of defense. `Transaction::new` already
    /// checked that the entries are structurally valid (balanced, non-empty,
    /// no overflow). `apply` checks the **stateful** invariants:
    ///
    /// 1. **Account existence** — every account referenced must exist
    /// 2. **Sufficient funds** — debited accounts must have enough balance
    /// 3. **Overflow safety** — balance updates use `checked_add`/`checked_sub`
    ///
    /// If any check fails, the ledger is **unchanged** (all-or-nothing).
    /// The transaction is only committed if every check passes.
    ///
    /// # All-or-Nothing Semantics
    ///
    /// We first validate ALL entries against current balances, computing
    /// what the new balances would be. Only after all entries pass do we
    /// actually update the balance cache and append to the event log.
    /// This prevents partial commits (e.g., debiting account A but failing
    /// on account B, leaving the ledger in an inconsistent state).
    ///
    /// # Arguments
    ///
    /// * `transaction` — A pre-validated `Transaction` (from `Transaction::new`)
    ///
    /// # Errors
    ///
    /// * `LedgerError::AccountNotFound` — an account doesn't exist
    /// * `LedgerError::InsufficientFunds` — a debit exceeds available balance
    /// * `LedgerError::Overflow` — balance update would overflow i64
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ledger_guard::ledger::{Ledger, Transaction, Entry};
    ///
    /// let mut ledger = Ledger::new();
    /// ledger.create_account("Checking".to_string()).unwrap();
    /// ledger.create_account("Savings".to_string()).unwrap();
    ///
    /// // Accounts need to be funded before transfers.
    /// // Assuming Checking has sufficient funds:
    /// let tx = Transaction::new(
    ///     "Transfer funds".to_string(),
    ///     vec![
    ///         Entry { account_id: "Checking".to_string(), amount: -5000 },
    ///         Entry { account_id: "Savings".to_string(),  amount:  5000 },
    ///     ],
    /// ).unwrap();
    /// // ledger.apply(tx).unwrap();
    /// ```
    pub fn apply(&mut self, transaction: Transaction) -> Result<(), LedgerError> {
        // ── Phase 1: Validate all entries against current state ──────
        //
        // We compute what the new balances WOULD be, without actually
        // changing anything. This gives us all-or-nothing semantics.
        //
        // `pending_updates` maps account_id → new_balance after this
        // transaction. We build this incrementally and check constraints
        // at each step.
        let mut pending_updates: HashMap<String, i64> = HashMap::new();

        for entry in &transaction.entries {
            // Look up the current balance (or the pending balance if
            // this account was already touched by an earlier entry in
            // the same transaction).
            let current_balance = if let Some(&pending) = pending_updates.get(&entry.account_id) {
                // This account was already modified by an earlier entry
                // in this same transaction. Use the pending value.
                pending
            } else if let Some(&existing) = self.balances.get(&entry.account_id) {
                // Account exists in the ledger, use its current balance.
                existing
            } else {
                // Account doesn't exist at all.
                return Err(LedgerError::AccountNotFound {
                    account: entry.account_id.clone(),
                });
            };

            // ── Compute new balance with overflow check ─────────────
            //
            // `checked_add` returns `None` if the result would overflow
            // i64. This is critical: without it, a carefully crafted
            // transaction could wrap a balance from positive to negative
            // (or vice versa), corrupting the ledger.
            let new_balance = current_balance
                .checked_add(entry.amount)
                .ok_or(LedgerError::Overflow)?;

            // ── Sufficient funds check ──────────────────────────────
            //
            // If the new balance would be negative, the account doesn't
            // have enough funds to cover this debit. Note: we check
            // `new_balance < 0`, not `entry.amount < 0`, because a
            // credit could still push a negative pending balance positive.
            if new_balance < 0 {
                return Err(LedgerError::InsufficientFunds {
                    account: entry.account_id.clone(),
                    available: current_balance,
                    // Report the debit amount as a positive number for readability
                    requested: -entry.amount,
                });
            }

            // Record the pending balance for this account
            pending_updates.insert(entry.account_id.clone(), new_balance);
        }

        // ── Phase 2: Commit ─────────────────────────────────────────
        //
        // All checks passed. Now we atomically update the balance cache
        // and append the transaction to the event log.
        //
        // This is safe because:
        // - We validated everything in Phase 1
        // - No fallible operations happen in Phase 2
        // - If the process crashes here, the transaction was never persisted
        //   (persistence is handled separately by the `save` function)

        for (account_id, new_balance) in pending_updates {
            // `insert` replaces the old value. We know the key exists
            // because Phase 1 verified it.
            self.balances.insert(account_id, new_balance);
        }

        self.transactions.push(transaction);

        Ok(())
    }

    /// Get the current balance of an account, in cents.
    ///
    /// This is an O(1) lookup against the balance cache — no need to
    /// replay the entire transaction history.
    ///
    /// # Errors
    ///
    /// Returns `LedgerError::AccountNotFound` if the account doesn't exist.
    pub fn balance(&self, account: &str) -> Result<i64, LedgerError> {
        self.balances
            .get(account)
            .copied() // Convert &i64 to i64 (i64 implements Copy)
            .ok_or_else(|| LedgerError::AccountNotFound {
                account: account.to_string(),
            })
    }

    /// Get a reference to the full transaction history (event log).
    ///
    /// Returns transactions in the order they were applied. This is the
    /// authoritative record that gets persisted to disk.
    pub fn history(&self) -> &[Transaction] {
        &self.transactions
    }

    /// Get all accounts and their current balances.
    ///
    /// Returns a sorted vector of `(account_name, balance_in_cents)` tuples.
    /// Sorted alphabetically by account name for consistent display.
    pub fn accounts(&self) -> Vec<(String, i64)> {
        let mut accounts: Vec<(String, i64)> = self
            .balances
            .iter()
            .map(|(name, &balance)| (name.clone(), balance))
            .collect();

        // Sort alphabetically for deterministic output
        accounts.sort_by(|a, b| a.0.cmp(&b.0));
        accounts
    }

    /// Get the number of committed transactions.
    pub fn transaction_count(&self) -> usize {
        self.transactions.len()
    }

    /// Check if an account exists in the ledger.
    pub fn account_exists(&self, name: &str) -> bool {
        self.balances.contains_key(name)
    }

    /// Get the internal transactions list (used by persistence layer).
    ///
    /// This returns a reference to the raw transaction vector. The
    /// persistence layer uses this to serialize the event log.
    pub fn get_transactions(&self) -> &Vec<Transaction> {
        &self.transactions
    }

    /// Get the internal accounts map (used by persistence layer).
    ///
    /// Returns account names registered in the ledger (for save/restore).
    pub fn get_account_names(&self) -> Vec<String> {
        self.balances.keys().cloned().collect()
    }
}

// ============================================================================
// Default implementation
// ============================================================================

impl Default for Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Unit Tests — Ledger Stateful Validation
// ============================================================================
//
// These tests verify the stateful invariants enforced by `Ledger::apply`.
// Combined with the Transaction::new tests, they provide 100% coverage
// of all error paths in the system.

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::Entry;

    /// Helper: create a ledger with pre-funded accounts for testing.
    ///
    /// This directly seeds balances via private field access (valid in tests).
    /// In production, money enters through double-entry transactions.
    /// For tests, direct seeding avoids the bootstrapping chicken-and-egg
    /// problem (you need money to transfer, but you need to transfer to get money).
    fn setup_ledger() -> Ledger {
        let mut ledger = Ledger::new();
        ledger.create_account("Checking".to_string()).unwrap();
        ledger.create_account("Savings".to_string()).unwrap();
        ledger.create_account("External".to_string()).unwrap();

        // Seed initial balances directly for testing
        ledger.balances.insert("Checking".to_string(), 10000);  // $100.00
        ledger.balances.insert("Savings".to_string(), 5000);    // $50.00
        ledger.balances.insert("External".to_string(), 100000); // $1,000.00

        ledger
    }

    // ── Account Creation ────────────────────────────────────────────

    #[test]
    fn test_create_account() {
        let mut ledger = Ledger::new();
        assert!(ledger.create_account("Checking".to_string()).is_ok());
        assert_eq!(ledger.balance("Checking").unwrap(), 0);
    }

    #[test]
    fn test_duplicate_account_rejected() {
        let mut ledger = Ledger::new();
        ledger.create_account("Checking".to_string()).unwrap();

        let err = ledger.create_account("Checking".to_string());
        assert!(matches!(err, Err(LedgerError::AccountAlreadyExists { .. })));
    }

    // ── Successful Apply ────────────────────────────────────────────

    #[test]
    fn test_apply_valid_transfer() {
        let mut ledger = setup_ledger();

        // Transfer $30 from Checking ($100) to Savings ($50)
        let tx = Transaction::new(
            "Transfer".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -3000 },
                Entry { account_id: "Savings".to_string(), amount: 3000 },
            ],
        ).unwrap();

        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking").unwrap(), 7000);  // $100 - $30 = $70
        assert_eq!(ledger.balance("Savings").unwrap(), 8000);   // $50 + $30 = $80
    }

    #[test]
    fn test_apply_exact_balance_transfer() {
        let mut ledger = setup_ledger();

        // Transfer exactly $100 from Checking (empties it to $0)
        let tx = Transaction::new(
            "Drain checking".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -10000 },
                Entry { account_id: "Savings".to_string(), amount: 10000 },
            ],
        ).unwrap();

        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking").unwrap(), 0);
        assert_eq!(ledger.balance("Savings").unwrap(), 15000); // $50 + $100 = $150
    }

    #[test]
    fn test_apply_three_way_transaction() {
        let mut ledger = setup_ledger();

        // Three-way split: Checking pays, Savings and External receive
        let tx = Transaction::new(
            "Split payment".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -5000 },
                Entry { account_id: "Savings".to_string(), amount: 3000 },
                Entry { account_id: "External".to_string(), amount: 2000 },
            ],
        ).unwrap();

        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking").unwrap(), 5000);  // $100 - $50
        assert_eq!(ledger.balance("Savings").unwrap(), 8000);   // $50 + $30
        assert_eq!(ledger.balance("External").unwrap(), 102000); // $1000 + $20
    }

    // ── Error Paths ─────────────────────────────────────────────────

    #[test]
    fn test_apply_nonexistent_account_rejected() {
        let mut ledger = setup_ledger();

        let tx = Transaction::new(
            "Bad transfer".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -1000 },
                Entry { account_id: "Nonexistent".to_string(), amount: 1000 },
            ],
        ).unwrap();

        let result = ledger.apply(tx);
        assert!(matches!(result, Err(LedgerError::AccountNotFound { .. })));

        // Verify ledger was NOT modified (all-or-nothing)
        assert_eq!(ledger.balance("Checking").unwrap(), 10000);
    }

    #[test]
    fn test_apply_insufficient_funds_rejected() {
        let mut ledger = setup_ledger();

        // Try to transfer $200 from Checking (which only has $100)
        let tx = Transaction::new(
            "Overdraft attempt".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -20000 },
                Entry { account_id: "Savings".to_string(), amount: 20000 },
            ],
        ).unwrap();

        let result = ledger.apply(tx);
        match result {
            Err(LedgerError::InsufficientFunds { account, available, requested }) => {
                assert_eq!(account, "Checking");
                assert_eq!(available, 10000);  // $100
                assert_eq!(requested, 20000);  // $200
            }
            other => panic!("Expected InsufficientFunds, got: {:?}", other),
        }

        // Verify ledger was NOT modified (all-or-nothing)
        assert_eq!(ledger.balance("Checking").unwrap(), 10000);
        assert_eq!(ledger.balance("Savings").unwrap(), 5000);
    }

    #[test]
    fn test_apply_all_or_nothing_on_second_entry_failure() {
        let mut ledger = setup_ledger();

        // First entry succeeds (Checking has $100), but second entry fails
        // (Savings only has $50, trying to debit $80)
        let tx = Transaction::new(
            "Multi-fail".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: 3000 }, // credit $30
                Entry { account_id: "Savings".to_string(), amount: -8000 }, // debit $80 (only $50!)
                Entry { account_id: "External".to_string(), amount: 5000 }, // credit $50
            ],
        ).unwrap();

        let result = ledger.apply(tx);
        assert!(matches!(result, Err(LedgerError::InsufficientFunds { .. })));

        // CRITICAL: even though Checking's entry was valid, it should NOT
        // have been applied because a later entry failed.
        assert_eq!(ledger.balance("Checking").unwrap(), 10000);
        assert_eq!(ledger.balance("Savings").unwrap(), 5000);
        assert_eq!(ledger.balance("External").unwrap(), 100000);
    }

    #[test]
    fn test_balance_nonexistent_account() {
        let ledger = Ledger::new();
        assert!(matches!(
            ledger.balance("Ghost"),
            Err(LedgerError::AccountNotFound { .. })
        ));
    }

    // ── History & Accounts ──────────────────────────────────────────

    #[test]
    fn test_history_tracks_all_transactions() {
        let mut ledger = setup_ledger();

        let tx1 = Transaction::new(
            "First".to_string(),
            vec![
                Entry { account_id: "Checking".to_string(), amount: -1000 },
                Entry { account_id: "Savings".to_string(), amount: 1000 },
            ],
        ).unwrap();

        let tx2 = Transaction::new(
            "Second".to_string(),
            vec![
                Entry { account_id: "Savings".to_string(), amount: -500 },
                Entry { account_id: "Checking".to_string(), amount: 500 },
            ],
        ).unwrap();

        ledger.apply(tx1).unwrap();
        ledger.apply(tx2).unwrap();

        assert_eq!(ledger.history().len(), 2);
        assert_eq!(ledger.history()[0].description, "First");
        assert_eq!(ledger.history()[1].description, "Second");
    }

    #[test]
    fn test_accounts_returns_sorted_list() {
        let mut ledger = Ledger::new();
        ledger.create_account("Zebra".to_string()).unwrap();
        ledger.create_account("Alpha".to_string()).unwrap();
        ledger.create_account("Middle".to_string()).unwrap();

        let accounts = ledger.accounts();
        assert_eq!(accounts[0].0, "Alpha");
        assert_eq!(accounts[1].0, "Middle");
        assert_eq!(accounts[2].0, "Zebra");
    }

    // ── Replay Consistency ──────────────────────────────────────────

    #[test]
    fn test_replay_produces_identical_balances() {
        // Build up a ledger with several transactions
        let mut ledger = setup_ledger();

        let transactions = vec![
            Transaction::new(
                "T1".to_string(),
                vec![
                    Entry { account_id: "Checking".to_string(), amount: -2000 },
                    Entry { account_id: "Savings".to_string(), amount: 2000 },
                ],
            ).unwrap(),
            Transaction::new(
                "T2".to_string(),
                vec![
                    Entry { account_id: "Savings".to_string(), amount: -1000 },
                    Entry { account_id: "External".to_string(), amount: 1000 },
                ],
            ).unwrap(),
            Transaction::new(
                "T3".to_string(),
                vec![
                    Entry { account_id: "External".to_string(), amount: -500 },
                    Entry { account_id: "Checking".to_string(), amount: 500 },
                ],
            ).unwrap(),
        ];

        for tx in &transactions {
            ledger.apply(tx.clone()).unwrap();
        }

        // Record final balances
        let final_balances = ledger.accounts();

        // Replay: create a fresh ledger and apply same transactions
        let mut replay_ledger = Ledger::new();
        replay_ledger.create_account("Checking".to_string()).unwrap();
        replay_ledger.create_account("Savings".to_string()).unwrap();
        replay_ledger.create_account("External".to_string()).unwrap();
        // Set same initial balances
        replay_ledger.balances.insert("Checking".to_string(), 10000);
        replay_ledger.balances.insert("Savings".to_string(), 5000);
        replay_ledger.balances.insert("External".to_string(), 100000);

        for tx in transactions {
            replay_ledger.apply(tx).unwrap();
        }

        let replay_balances = replay_ledger.accounts();

        // Balances must be identical
        assert_eq!(final_balances, replay_balances);
    }
}
