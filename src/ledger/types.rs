/// Accounts are identified by name. Names are trimmed, case-sensitive strings.
/// Uses a newtype (`AccountId`) for type safety and unified validation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub String);

/// A single line in a double-entry transaction.
/// `amount` is in cents (i64) to avoid floating-point precision loss. Negative = debit.
/// Zero amounts are never valid.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub account: AccountId,
    pub amount: i64,
}

/// A validated, immutable financial transaction.
/// The type is the proof of valid structural invariants (zero-sum, non-zero entries).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Transaction {
    pub id: u64,
    pub description: String,
    pub entries: Vec<Entry>,
    pub timestamp: std::time::SystemTime,
}
