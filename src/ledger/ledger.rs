use std::collections::HashMap;

use super::error::LedgerError;
use super::types::{AccountId, Entry, Transaction};

/// The core financial ledger.
///
/// Maintains a running balance cache (`HashMap<String, i64>`) and an append-only
/// transaction history (`Vec<Transaction>`). The cache gives O(1) balance lookups;
/// the history enables replay and audit.
///
/// # Two-layer defense model
///
/// 1. **`Transaction::new`** — construction-time invariants (zero-sum, no zero amounts,
///    no overflow). If you hold a `Transaction`, it is structurally valid.
/// 2. **`Ledger::apply`** — stateful validation (account existence, sufficient funds).
///    If `apply` returns `Ok`, the transaction is both structurally and contextually valid.
pub struct Ledger {
    accounts: HashMap<String, i64>,
    history: Vec<Transaction>,
    next_id: u64,
}

impl Transaction {
    /// Construct a validated transaction from a description and entries.
    ///
    /// This is the FIRST LINE OF DEFENSE. Construction-time invariants:
    ///   1. Entries must not be empty.
    ///   2. No entry may have amount == 0 (InvalidAmount).
    ///   3. The sum of all entry amounts must equal exactly zero (Unbalanced).
    ///      Use checked_add to accumulate the sum — if any addition overflows, return Overflow.
    ///
    /// # Why Result instead of panic?
    ///
    /// Panicking on invalid input means the program crashes when an agent sends garbage.
    /// Returning Result means the caller decides what to do — log it, report it, retry.
    /// Panics are for programmer errors (bugs). Invalid agent input is an expected runtime
    /// condition, not a bug. This distinction matters in production systems.
    ///
    /// In Rust, the convention is:
    /// - `panic!` / `unwrap()` / `expect()` → invariant violations, programmer bugs
    /// - `Result<T, E>` → expected failures that callers should handle
    ///
    /// An AI agent sending `{"entries": []}` is not a bug in our code — it's bad input
    /// from an untrusted source. We return `Err`, not crash.
    ///
    /// # Why not typestate (DraftTransaction → ValidatedTransaction)?
    ///
    /// Typestate is justified when many consumers might skip validation steps.
    /// This project has two consumers (agent and CLI). Typestate would add significant
    /// complexity for no safety gain. The constructor IS the typestate — you cannot hold
    /// a Transaction that skipped validation. The type itself is the proof of validity.
    ///
    /// Consider: with typestate, you'd need `DraftTransaction`, `ValidatedTransaction`,
    /// and conversion methods between them. That's three types instead of one, with
    /// generic parameters or phantom types to enforce ordering. For two consumers,
    /// that complexity doesn't pay for itself.
    ///
    /// # Why `checked_add` instead of plain addition?
    ///
    /// `i64` can overflow. Plain addition wraps silently in release mode (Rust uses
    /// two's complement wrapping for release builds, panics in debug builds). A malicious
    /// agent sending `i64::MAX` and `-i64::MAX + 1` would appear balanced under plain
    /// addition but isn't — the sum would wrap around.
    ///
    /// `checked_add` returns `Option<i64>`:
    /// - `Some(result)` if the addition is safe
    /// - `None` if it would overflow
    ///
    /// We map `None` → `LedgerError::Overflow`. This is Rust's idiomatic approach:
    /// make the error case explicit, not silent. Other options exist (`saturating_add`,
    /// `wrapping_add`) but `checked_add` is the right choice here because we want to
    /// REJECT the transaction, not silently cap or wrap the value.
    ///
    /// # Implementation steps
    ///
    /// ```text
    /// 1. If entries.is_empty(), return Err(LedgerError::Unbalanced { actual_sum: 0 }).
    ///    — An empty transaction is meaningless. We use Unbalanced rather than a new
    ///      variant because it IS unbalanced (sum of nothing is 0, but we require
    ///      at least one entry to make that sum meaningful).
    ///
    /// 2. Iterate entries. For each entry:
    ///    if entry.amount == 0, return Err(LedgerError::InvalidAmount).
    ///    — Zero-amount entries are accounting noise. They don't debit or credit
    ///      anything. Rejecting them keeps the ledger clean and prevents agents
    ///      from padding transactions with meaningless entries.
    ///
    /// 3. Accumulate sum using checked arithmetic:
    ///    let sum = entries.iter().try_fold(0i64, |acc, e| acc.checked_add(e.amount));
    ///    — try_fold is like fold, but short-circuits on None (overflow).
    ///    — If sum is None → return Err(LedgerError::Overflow)
    ///    — If sum is Some(s) and s != 0 → return Err(LedgerError::Unbalanced { actual_sum: s })
    ///
    /// 4. All checks passed. Return Ok(Transaction {
    ///        id: 0,           // Placeholder — Ledger::apply assigns the real id
    ///        description,
    ///        entries,
    ///        timestamp: std::time::SystemTime::now(),
    ///    })
    /// ```
    pub fn new(description: String, entries: Vec<Entry>) -> Result<Self, LedgerError> {
        if entries.is_empty() { return Err(LedgerError::Unbalanced {actual_sum: 0}); }
        
        let mut sum: i64 = 0;
        for e in entries.iter() {
            if e.amount == 0 { return Err(LedgerError::InvalidAmount); }
            
            sum = sum.checked_add(e.amount).ok_or(LedgerError::Overflow)?;
        }

        if sum != 0 { return Err(LedgerError::Unbalanced {actual_sum: sum}) }

        Ok(Self {
            id: 0,
            description, 
            entries,
            timestamp: std::time::SystemTime::now(),
        })

    }
}

impl Ledger {
    /// Creates an empty ledger with no accounts and no history.
    ///
    /// The `next_id` counter starts at 1 so the first committed transaction
    /// gets `id = 1`, making it easy to distinguish "never committed" (id 0)
    /// from "first real transaction" (id 1).
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let ledger = Ledger::new();
    /// assert_eq!(ledger.transaction_count(), 0);
    /// assert!(ledger.accounts().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            history: Vec::new(),
            next_id: 1,
        }
    }

    /// Creates a new account with a zero balance.
    ///
    /// Account names are trimmed of leading/trailing whitespace. Empty names
    /// (after trimming) are rejected as `AccountNotFound` — a name that is
    /// nothing refers to nothing.
    ///
    /// # Errors
    ///
    /// - `LedgerError::AccountNotFound` — if the trimmed name is empty.
    /// - `LedgerError::DuplicateAccount` — if an account with this name already exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let mut ledger = Ledger::new();
    /// ledger.create_account("Checking").unwrap();
    /// assert_eq!(ledger.balance("Checking"), Some(0));
    /// ```
    pub fn create_account(&mut self, name: &str) -> Result<(), LedgerError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(LedgerError::AccountNotFound {
                name: String::new(),
            });
        }
        if self.accounts.contains_key(trimmed) {
            return Err(LedgerError::DuplicateAccount {
                name: trimmed.to_string(),
            });
        }
        self.accounts.insert(trimmed.to_string(), 0);
        Ok(())
    }

    /// Returns the current balance of an account, or `None` if the account
    /// does not exist.
    ///
    /// # Why `Option` instead of `Result`?
    ///
    /// "Account might not exist" is normal absence, not an error condition.
    /// The caller can pattern-match on `Some`/`None` without unwrapping an
    /// error type. This follows Rust convention: `HashMap::get` returns
    /// `Option`, and so does our wrapper.
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let mut ledger = Ledger::new();
    /// assert_eq!(ledger.balance("Nonexistent"), None);
    ///
    /// ledger.create_account("Savings").unwrap();
    /// assert_eq!(ledger.balance("Savings"), Some(0));
    /// ```
    pub fn balance(&self, name: &str) -> Option<i64> {
        self.accounts.get(name).copied()
    }

    /// Returns all accounts and their balances, sorted alphabetically by name.
    ///
    /// Deterministic output is critical for replay consistency tests — if two
    /// ledgers that processed the same transactions in the same order produce
    /// different `accounts()` output, something is wrong.
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let mut ledger = Ledger::new();
    /// ledger.create_account("Zebra").unwrap();
    /// ledger.create_account("Alpha").unwrap();
    ///
    /// let accounts = ledger.accounts();
    /// assert_eq!(accounts[0].0, "Alpha");
    /// assert_eq!(accounts[1].0, "Zebra");
    /// ```
    pub fn accounts(&self) -> Vec<(String, i64)> {
        let mut sorted: Vec<(String, i64)> = self
            .accounts
            .iter()
            .map(|(name, &balance)| (name.clone(), balance))
            .collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        sorted
    }

    /// Returns an immutable slice of all committed transactions, in order.
    ///
    /// This is the append-only event log. Combined with `accounts()`, it forms
    /// the dual representation: the history is the source of truth, the balance
    /// cache is the optimization. Property-based tests will verify that replaying
    /// `history()` from scratch produces the same `accounts()` output.
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let ledger = Ledger::new();
    /// assert!(ledger.history().is_empty());
    /// ```
    pub fn history(&self) -> &[Transaction] {
        &self.history
    }

    /// Returns the total number of committed transactions.
    ///
    /// This is always equal to `self.history().len()`. Having a dedicated method
    /// makes intent clearer at call sites and avoids exposing the full history
    /// just to count entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ledger_guard::ledger::Ledger;
    ///
    /// let ledger = Ledger::new();
    /// assert_eq!(ledger.transaction_count(), 0);
    /// ```
    pub fn transaction_count(&self) -> usize {
        self.history.len()
    }

    /// Apply a validated transaction to the ledger.
    ///
    /// This is the SECOND LINE OF DEFENSE — stateful validation.
    /// `Transaction::new` already proved the entries are structurally balanced.
    /// `Ledger::apply` checks whether the transaction is valid GIVEN CURRENT STATE.
    ///
    /// # Two-phase design — critical for concurrent correctness
    ///
    /// ## Phase 1 — Validation (read-only, NO mutation)
    ///
    /// For each entry in `tx.entries`:
    ///
    /// a. Verify the account exists in `self.accounts`. If not → `AccountNotFound`.
    ///    Use `self.accounts.get(&entry.account.0)` — if it returns `None`, the
    ///    account was never created with `create_account()`.
    ///
    /// b. For each debit entry (`amount < 0`): verify `current_balance + amount >= 0`,
    ///    UNLESS the account name is `"External"` (External can go negative — it
    ///    represents the outside world, not an internal account with a real constraint).
    ///
    ///    If balance would go negative → `InsufficientFunds { account, available, requested }`.
    ///    Note: `requested` should be the **absolute value** of the debit amount
    ///    (i.e., `amount.abs()`), because the error message says "requested $X" —
    ///    negative dollars in a user-facing message is confusing.
    ///
    /// ## Phase 2 — Commit (mutation, MUST be infallible)
    ///
    /// If and only if ALL validations in Phase 1 passed:
    ///
    /// a. Assign `self.next_id` to `tx.id`, then increment `self.next_id`.
    ///
    /// b. For each entry: update the balance:
    ///    ```text
    ///    let balance = self.accounts.get_mut(&entry.account.0).unwrap();
    ///    *balance = balance.checked_add(entry.amount)
    ///        .expect("balance overflow in commit phase — invariant violation");
    ///    ```
    ///    The `unwrap()` and `expect()` here are correct — if we reach Phase 2,
    ///    we already verified the account exists (Phase 1a) and the arithmetic
    ///    is safe (Transaction::new checked the sum). A panic here would indicate
    ///    a bug in our validation logic, not bad user input. This is exactly when
    ///    `expect()` is appropriate: "this should be unreachable; if it fires,
    ///    our code has a bug."
    ///
    /// c. Push `tx` into `self.history`.
    ///
    /// # Why two phases?
    ///
    /// If you interleave validation and mutation (validate entry 1, apply entry 1,
    /// validate entry 2 — fails), you've partially mutated the ledger and must
    /// roll back. Rollback logic is complex, error-prone, and hard to test
    /// exhaustively. Two phases means: either ALL validations pass and ALL
    /// mutations apply, or NO mutations happen. Atomic from the caller's perspective.
    ///
    /// Under concurrent load (`Arc<Mutex<Ledger>>` in Phase 4), this means a thread
    /// that panics in Phase 1 leaves the ledger completely unmodified. Mutex poison
    /// recovery is safe precisely because Phase 1 never mutates. This design
    /// decision in Phase 1 enables the safety guarantee in Phase 4.
    ///
    /// # Implementation
    ///
    /// ```text
    /// // Phase 1: Validation — collect the first error, return it
    /// for entry in &tx.entries {
    ///     let balance = self.accounts.get(&entry.account.0)
    ///         .ok_or_else(|| LedgerError::AccountNotFound {
    ///             name: entry.account.0.clone(),
    ///         })?;
    ///
    ///     if entry.amount < 0 && entry.account.0 != "External" {
    ///         if *balance + entry.amount < 0 {
    ///             return Err(LedgerError::InsufficientFunds {
    ///                 account: entry.account.0.clone(),
    ///                 available: *balance,
    ///                 requested: entry.amount.abs(),
    ///             });
    ///         }
    ///     }
    /// }
    ///
    /// // Phase 2: Commit — infallible after Phase 1
    /// tx.id = self.next_id;
    /// self.next_id += 1;
    ///
    /// for entry in &tx.entries {
    ///     let balance = self.accounts.get_mut(&entry.account.0).unwrap();
    ///     *balance = balance.checked_add(entry.amount)
    ///         .expect("balance overflow in commit phase — invariant violation");
    /// }
    ///
    /// self.history.push(tx);
    /// Ok(())
    /// ```
    pub fn apply(&mut self, mut tx: Transaction) -> Result<(), LedgerError> {
        todo!("Ledger::apply — implement two-phase validation + commit")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Helper: build entries quickly
    // -----------------------------------------------------------------------

    fn entry(account: &str, amount: i64) -> Entry {
        Entry {
            account: AccountId(account.to_string()),
            amount,
        }
    }

    /// Helper to create a ledger with two standard accounts ("Checking", "Savings")
    /// each funded with the specified balance via "External".
    fn ledger_with_accounts(checking_balance: i64, savings_balance: i64) -> Ledger {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("Checking").unwrap();
        ledger.create_account("Savings").unwrap();

        if checking_balance != 0 {
            let tx = Transaction::new(
                "Fund Checking".into(),
                vec![
                    entry("External", -checking_balance),
                    entry("Checking", checking_balance),
                ],
            )
            .unwrap();
            ledger.apply(tx).unwrap();
        }

        if savings_balance != 0 {
            let tx = Transaction::new(
                "Fund Savings".into(),
                vec![
                    entry("External", -savings_balance),
                    entry("Savings", savings_balance),
                ],
            )
            .unwrap();
            ledger.apply(tx).unwrap();
        }

        ledger
    }

    // =======================================================================
    // Transaction::new — construction-time invariants
    // =======================================================================

    #[test]
    fn tx_empty_entries_is_unbalanced() {
        let result = Transaction::new("Empty".into(), vec![]);
        match result {
            Err(LedgerError::Unbalanced { actual_sum: 0 }) => {}
            other => panic!("Expected Unbalanced {{ actual_sum: 0 }}, got {other:?}"),
        }
    }

    #[test]
    fn tx_zero_amount_entry_is_invalid() {
        let result = Transaction::new("Zero".into(), vec![entry("A", 0)]);
        match result {
            Err(LedgerError::InvalidAmount) => {}
            other => panic!("Expected InvalidAmount, got {other:?}"),
        }
    }

    #[test]
    fn tx_zero_amount_among_valid_entries_is_invalid() {
        let result = Transaction::new(
            "Mixed".into(),
            vec![entry("A", 100), entry("B", 0), entry("C", -100)],
        );
        match result {
            Err(LedgerError::InvalidAmount) => {}
            other => panic!("Expected InvalidAmount, got {other:?}"),
        }
    }

    #[test]
    fn tx_overflow_is_detected() {
        let result = Transaction::new(
            "Overflow".into(),
            vec![entry("A", i64::MAX), entry("B", 1)],
        );
        match result {
            Err(LedgerError::Overflow) => {}
            other => panic!("Expected Overflow, got {other:?}"),
        }
    }

    #[test]
    fn tx_underflow_is_detected() {
        let result = Transaction::new(
            "Underflow".into(),
            vec![entry("A", i64::MIN), entry("B", -1)],
        );
        match result {
            Err(LedgerError::Overflow) => {}
            other => panic!("Expected Overflow, got {other:?}"),
        }
    }

    #[test]
    fn tx_unbalanced_positive_sum() {
        let result = Transaction::new(
            "Unbalanced+".into(),
            vec![entry("A", 100), entry("B", -50)],
        );
        match result {
            Err(LedgerError::Unbalanced { actual_sum: 50 }) => {}
            other => panic!("Expected Unbalanced {{ actual_sum: 50 }}, got {other:?}"),
        }
    }

    #[test]
    fn tx_unbalanced_negative_sum() {
        let result = Transaction::new(
            "Unbalanced-".into(),
            vec![entry("A", -100), entry("B", 30)],
        );
        match result {
            Err(LedgerError::Unbalanced { actual_sum: -70 }) => {}
            other => panic!("Expected Unbalanced {{ actual_sum: -70 }}, got {other:?}"),
        }
    }

    #[test]
    fn tx_valid_two_entry_transfer() {
        let tx = Transaction::new(
            "Transfer".into(),
            vec![entry("A", 500), entry("B", -500)],
        )
        .unwrap();
        assert_eq!(tx.entries.len(), 2);
        assert_eq!(tx.id, 0); // Placeholder before Ledger::apply
        assert_eq!(tx.description, "Transfer");
    }

    #[test]
    fn tx_valid_multi_entry() {
        // Three entries summing to zero: +100, +200, -300
        let tx = Transaction::new(
            "Multi".into(),
            vec![entry("A", 100), entry("B", 200), entry("C", -300)],
        )
        .unwrap();
        assert_eq!(tx.entries.len(), 3);
    }

    #[test]
    fn tx_valid_large_balanced_amounts() {
        // Large but not overflowing: i64::MAX/2 and -(i64::MAX/2)
        let half_max = i64::MAX / 2;
        let tx = Transaction::new(
            "Big".into(),
            vec![entry("A", half_max), entry("B", -half_max)],
        )
        .unwrap();
        assert_eq!(tx.entries.len(), 2);
    }

    #[test]
    fn tx_single_entry_nonzero_is_unbalanced() {
        let result = Transaction::new("Solo".into(), vec![entry("A", 42)]);
        match result {
            Err(LedgerError::Unbalanced { actual_sum: 42 }) => {}
            other => panic!("Expected Unbalanced {{ actual_sum: 42 }}, got {other:?}"),
        }
    }

    // =======================================================================
    // Ledger::new
    // =======================================================================

    #[test]
    fn new_ledger_is_empty() {
        let ledger = Ledger::new();
        assert_eq!(ledger.transaction_count(), 0);
        assert!(ledger.accounts().is_empty());
        assert!(ledger.history().is_empty());
    }

    // =======================================================================
    // Ledger::create_account
    // =======================================================================

    #[test]
    fn create_account_success() {
        let mut ledger = Ledger::new();
        assert!(ledger.create_account("Checking").is_ok());
        assert_eq!(ledger.balance("Checking"), Some(0));
    }

    #[test]
    fn create_account_trims_whitespace() {
        let mut ledger = Ledger::new();
        ledger.create_account("  Savings  ").unwrap();
        assert_eq!(ledger.balance("Savings"), Some(0));
    }

    #[test]
    fn create_account_empty_name_rejected() {
        let mut ledger = Ledger::new();
        let result = ledger.create_account("   ");
        match result {
            Err(LedgerError::AccountNotFound { name }) if name.is_empty() => {}
            other => panic!("Expected AccountNotFound with empty name, got {other:?}"),
        }
    }

    #[test]
    fn create_account_duplicate_rejected() {
        let mut ledger = Ledger::new();
        ledger.create_account("Checking").unwrap();
        let result = ledger.create_account("Checking");
        match result {
            Err(LedgerError::DuplicateAccount { name }) if name == "Checking" => {}
            other => panic!("Expected DuplicateAccount, got {other:?}"),
        }
    }

    #[test]
    fn create_account_case_sensitive() {
        let mut ledger = Ledger::new();
        ledger.create_account("checking").unwrap();
        // "Checking" (capital C) is different from "checking" (lowercase c)
        assert!(ledger.create_account("Checking").is_ok());
    }

    // =======================================================================
    // Ledger::balance
    // =======================================================================

    #[test]
    fn balance_nonexistent_is_none() {
        let ledger = Ledger::new();
        assert_eq!(ledger.balance("Ghost"), None);
    }

    #[test]
    fn balance_new_account_is_zero() {
        let mut ledger = Ledger::new();
        ledger.create_account("Test").unwrap();
        assert_eq!(ledger.balance("Test"), Some(0));
    }

    // =======================================================================
    // Ledger::accounts — sorted output
    // =======================================================================

    #[test]
    fn accounts_sorted_alphabetically() {
        let mut ledger = Ledger::new();
        ledger.create_account("Zebra").unwrap();
        ledger.create_account("Alpha").unwrap();
        ledger.create_account("Middle").unwrap();

        let accts = ledger.accounts();
        assert_eq!(accts.len(), 3);
        assert_eq!(accts[0].0, "Alpha");
        assert_eq!(accts[1].0, "Middle");
        assert_eq!(accts[2].0, "Zebra");
    }

    #[test]
    fn accounts_returns_current_balances() {
        let ledger = ledger_with_accounts(10000, 5000);
        let accts = ledger.accounts();
        // Sorted: Checking, External, Savings
        let checking = accts.iter().find(|(n, _)| n == "Checking").unwrap();
        let savings = accts.iter().find(|(n, _)| n == "Savings").unwrap();
        assert_eq!(checking.1, 10000);
        assert_eq!(savings.1, 5000);
    }

    // =======================================================================
    // Ledger::history and transaction_count
    // =======================================================================

    #[test]
    fn history_empty_initially() {
        let ledger = Ledger::new();
        assert!(ledger.history().is_empty());
    }

    #[test]
    fn transaction_count_matches_history_len() {
        let ledger = ledger_with_accounts(10000, 5000);
        assert_eq!(ledger.transaction_count(), ledger.history().len());
    }

    // =======================================================================
    // Ledger::apply — stateful validation
    // =======================================================================

    #[test]
    fn apply_nonexistent_account_is_account_not_found() {
        let mut ledger = Ledger::new();
        ledger.create_account("A").unwrap();
        // "B" does not exist
        let tx = Transaction::new(
            "Ghost".into(),
            vec![entry("A", -100), entry("B", 100)],
        )
        .unwrap();
        match ledger.apply(tx) {
            Err(LedgerError::AccountNotFound { name }) if name == "B" => {}
            other => panic!("Expected AccountNotFound for B, got {other:?}"),
        }
    }

    #[test]
    fn apply_insufficient_funds() {
        let mut ledger = ledger_with_accounts(5000, 0);
        // Checking has $50.00, try to debit $100.00
        let tx = Transaction::new(
            "Overdraft".into(),
            vec![entry("Checking", -10000), entry("Savings", 10000)],
        )
        .unwrap();
        match ledger.apply(tx) {
            Err(LedgerError::InsufficientFunds {
                account,
                available,
                requested,
            }) => {
                assert_eq!(account, "Checking");
                assert_eq!(available, 5000);
                assert_eq!(requested, 10000);
            }
            other => panic!("Expected InsufficientFunds, got {other:?}"),
        }
    }

    #[test]
    fn apply_debit_exact_balance_succeeds() {
        let mut ledger = ledger_with_accounts(5000, 0);
        // Debit exactly $50.00 from Checking — should succeed (balance becomes 0)
        let tx = Transaction::new(
            "Exact".into(),
            vec![entry("Checking", -5000), entry("Savings", 5000)],
        )
        .unwrap();
        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking"), Some(0));
        assert_eq!(ledger.balance("Savings"), Some(5000));
    }

    #[test]
    fn apply_cannot_go_negative_by_one_cent() {
        let mut ledger = ledger_with_accounts(5000, 0);
        // Try to debit $50.01 — one cent more than available
        let tx = Transaction::new(
            "OneCentOver".into(),
            vec![entry("Checking", -5001), entry("Savings", 5001)],
        )
        .unwrap();
        match ledger.apply(tx) {
            Err(LedgerError::InsufficientFunds { .. }) => {}
            other => panic!("Expected InsufficientFunds, got {other:?}"),
        }
    }

    #[test]
    fn apply_external_can_go_negative() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("Checking").unwrap();

        // External starts at 0, debiting makes it negative — should be allowed
        let tx = Transaction::new(
            "Fund".into(),
            vec![entry("External", -100000), entry("Checking", 100000)],
        )
        .unwrap();
        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("External"), Some(-100000));
        assert_eq!(ledger.balance("Checking"), Some(100000));
    }

    #[test]
    fn apply_external_can_go_deeply_negative() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("Vault").unwrap();

        // Send a million dollars into Vault from External
        let tx = Transaction::new(
            "BigFund".into(),
            vec![entry("External", -100_000_000), entry("Vault", 100_000_000)],
        )
        .unwrap();
        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("External"), Some(-100_000_000));
    }

    #[test]
    fn apply_updates_balances() {
        let mut ledger = ledger_with_accounts(10000, 5000);
        // Transfer $30.00 from Checking to Savings
        let tx = Transaction::new(
            "Transfer".into(),
            vec![entry("Checking", -3000), entry("Savings", 3000)],
        )
        .unwrap();
        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking"), Some(7000));
        assert_eq!(ledger.balance("Savings"), Some(8000));
    }

    #[test]
    fn apply_increments_history_length() {
        let mut ledger = ledger_with_accounts(10000, 0);
        let before = ledger.transaction_count();
        let tx = Transaction::new(
            "Another".into(),
            vec![entry("Checking", -1000), entry("Savings", 1000)],
        )
        .unwrap();
        ledger.apply(tx).unwrap();
        assert_eq!(ledger.transaction_count(), before + 1);
    }

    #[test]
    fn apply_assigns_sequential_ids() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("A").unwrap();

        let tx1 = Transaction::new(
            "First".into(),
            vec![entry("External", -100), entry("A", 100)],
        )
        .unwrap();
        ledger.apply(tx1).unwrap();

        let tx2 = Transaction::new(
            "Second".into(),
            vec![entry("External", -200), entry("A", 200)],
        )
        .unwrap();
        ledger.apply(tx2).unwrap();

        let history = ledger.history();
        assert_eq!(history[0].id, 1);
        assert_eq!(history[1].id, 2);
    }

    #[test]
    fn apply_failed_tx_does_not_mutate_ledger() {
        let mut ledger = ledger_with_accounts(5000, 0);
        let balance_before = ledger.balance("Checking").unwrap();
        let count_before = ledger.transaction_count();

        // This should fail — insufficient funds
        let tx = Transaction::new(
            "Fail".into(),
            vec![entry("Checking", -99999), entry("Savings", 99999)],
        )
        .unwrap();
        let _ = ledger.apply(tx);

        // Ledger is unchanged
        assert_eq!(ledger.balance("Checking").unwrap(), balance_before);
        assert_eq!(ledger.transaction_count(), count_before);
    }

    #[test]
    fn apply_multi_entry_transaction() {
        let mut ledger = ledger_with_accounts(10000, 5000);
        // Three-way split: Checking -$60, Savings -$40, External +$100
        let tx = Transaction::new(
            "Split".into(),
            vec![
                entry("Checking", -6000),
                entry("Savings", -4000),
                entry("External", 10000),
            ],
        )
        .unwrap();
        assert!(ledger.apply(tx).is_ok());
        assert_eq!(ledger.balance("Checking"), Some(4000));
        assert_eq!(ledger.balance("Savings"), Some(1000));
    }

    #[test]
    fn apply_preserves_transaction_description() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("A").unwrap();

        let tx = Transaction::new(
            "Payroll deposit".into(),
            vec![entry("External", -500000), entry("A", 500000)],
        )
        .unwrap();
        ledger.apply(tx).unwrap();

        assert_eq!(ledger.history()[0].description, "Payroll deposit");
    }

    #[test]
    fn apply_transaction_has_timestamp() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("A").unwrap();

        let before = std::time::SystemTime::now();
        let tx = Transaction::new(
            "Timed".into(),
            vec![entry("External", -100), entry("A", 100)],
        )
        .unwrap();
        ledger.apply(tx).unwrap();
        let after = std::time::SystemTime::now();

        let ts = ledger.history()[0].timestamp;
        assert!(ts >= before && ts <= after);
    }

    // =======================================================================
    // Conservation / consistency checks
    // =======================================================================

    #[test]
    fn sum_of_all_balances_is_zero_after_transactions() {
        let mut ledger = Ledger::new();
        ledger.create_account("External").unwrap();
        ledger.create_account("A").unwrap();
        ledger.create_account("B").unwrap();
        ledger.create_account("C").unwrap();

        // Fund A and B from External
        let tx1 = Transaction::new(
            "Fund A".into(),
            vec![entry("External", -50000), entry("A", 50000)],
        )
        .unwrap();
        ledger.apply(tx1).unwrap();

        let tx2 = Transaction::new(
            "Fund B".into(),
            vec![entry("External", -30000), entry("B", 30000)],
        )
        .unwrap();
        ledger.apply(tx2).unwrap();

        // Transfer from A to C
        let tx3 = Transaction::new(
            "A to C".into(),
            vec![entry("A", -10000), entry("C", 10000)],
        )
        .unwrap();
        ledger.apply(tx3).unwrap();

        // Sum of all balances must be zero (conservation of money)
        let total: i64 = ledger.accounts().iter().map(|(_, b)| b).sum();
        assert_eq!(total, 0, "Conservation of money violated");
    }

    #[test]
    fn transaction_count_always_matches_history_len() {
        let ledger = ledger_with_accounts(10000, 5000);
        // ledger_with_accounts applies 2 transactions (fund Checking + fund Savings)
        assert_eq!(ledger.transaction_count(), 2);
        assert_eq!(ledger.history().len(), 2);
        assert_eq!(ledger.transaction_count(), ledger.history().len());
    }

    #[test]
    fn multiple_operations_leave_ledger_consistent() {
        let mut ledger = ledger_with_accounts(100000, 50000);

        // Series of transfers
        for i in 0..10 {
            let tx = Transaction::new(
                format!("Transfer {i}"),
                vec![entry("Checking", -1000), entry("Savings", 1000)],
            )
            .unwrap();
            ledger.apply(tx).unwrap();
        }

        assert_eq!(ledger.balance("Checking"), Some(90000));
        assert_eq!(ledger.balance("Savings"), Some(60000));
        // 2 initial funding txs + 10 transfers
        assert_eq!(ledger.transaction_count(), 12);

        // Conservation check
        let total: i64 = ledger.accounts().iter().map(|(_, b)| b).sum();
        assert_eq!(total, 0);
    }
}
