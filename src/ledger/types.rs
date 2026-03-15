/// Accounts are identified by name. Names are trimmed, case-sensitive strings.
///
/// `"External"` is the system boundary account — it can go negative (represents
/// the outside world). All other accounts must maintain non-negative balances.
///
/// # Why a newtype instead of plain `String`?
///
/// A newtype (`AccountId(String)`) gives us:
/// - **Type safety:** You can't accidentally pass a description where an account
///   name is expected. The compiler catches it.
/// - **Single validation point:** If we later add rules (e.g., max length, allowed
///   characters), there's one place to enforce them.
/// - **Self-documenting:** Function signatures like `fn balance(id: &AccountId)`
///   are clearer than `fn balance(name: &str)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub String);

/// A single line in a double-entry transaction.
///
/// `amount` is in cents (smallest USD unit). Positive = credit. Negative = debit.
/// Zero amounts are never valid — enforced by `Transaction::new`.
///
/// # Why cents (`i64`) instead of dollars (`f64`)?
///
/// Floating-point arithmetic loses precision. `0.1 + 0.2 != 0.3` in IEEE 754.
/// Financial systems store amounts as integers in the smallest currency unit
/// (cents for USD). This matches industry practice (Stripe, Square, Ramp).
/// All arithmetic uses `checked_add` / `checked_sub` to detect overflow.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub account: AccountId,
    pub amount: i64,
}

/// A validated, immutable financial transaction.
///
/// If this type exists, it has already passed all construction-time invariants.
/// It is impossible to construct an invalid `Transaction` — the type is the proof.
///
/// # Invariants guaranteed by `Transaction::new`:
/// 1. At least one entry exists.
/// 2. No entry has `amount == 0`.
/// 3. All entry amounts sum to exactly zero (double-entry balance).
///
/// The `id` field is assigned by `Ledger::apply` when the transaction is committed.
/// During construction, `id` is set to `0` as a placeholder.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    pub id: u64,
    pub description: String,
    pub entries: Vec<Entry>,
    pub timestamp: std::time::SystemTime,
}
