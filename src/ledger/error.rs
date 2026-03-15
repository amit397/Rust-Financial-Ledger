/// Errors that can occur during ledger operations.
///
/// Each variant represents a specific invariant violation. All monetary amounts
/// are stored as `i64` in cents — Display formats them as human-readable dollars.
///
/// # Design Notes (m06-error-handling)
///
/// This enum uses manual `Display` instead of `thiserror` because the prompt
/// forbids adding dependencies beyond the listed set. The pattern is identical:
/// each variant gets a human-readable message. `std::error::Error` is implemented
/// with a blanket `source()` returning `None` since these are leaf errors.
#[derive(Debug)]
pub enum LedgerError {
    /// Transaction entries do not sum to zero (double-entry violation).
    Unbalanced { actual_sum: i64 },

    /// Account does not have enough funds to cover the debit.
    InsufficientFunds {
        account: String,
        available: i64,
        requested: i64,
    },

    /// Referenced account does not exist in the ledger.
    AccountNotFound { name: String },

    /// Checked arithmetic returned `None` — integer overflow detected.
    Overflow,

    /// An entry has a zero amount, which is never valid in double-entry accounting.
    InvalidAmount,

    /// Attempted to create an account that already exists.
    DuplicateAccount { name: String },
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::Unbalanced { actual_sum } => {
                let dollars = actual_sum / 100;
                let cents = (actual_sum % 100).abs();
                write!(
                    f,
                    "Transaction is unbalanced: entries sum to ${dollars}.{cents:02} instead of $0.00"
                )
            }
            LedgerError::InsufficientFunds {
                account,
                available,
                requested,
            } => {
                let avail_dollars = available / 100;
                let avail_cents = (available % 100).abs();
                let req_dollars = requested / 100;
                let req_cents = (requested % 100).abs();
                write!(
                    f,
                    "Insufficient funds: {account} has ${avail_dollars}.{avail_cents:02}, \
                     requested ${req_dollars}.{req_cents:02}"
                )
            }
            LedgerError::AccountNotFound { name } => {
                write!(f, "Account not found: {name}")
            }
            LedgerError::Overflow => {
                write!(f, "Arithmetic overflow: operation would exceed i64 bounds")
            }
            LedgerError::InvalidAmount => {
                write!(f, "Invalid amount: entry amount must not be zero")
            }
            LedgerError::DuplicateAccount { name } => {
                write!(f, "Duplicate account: {name} already exists")
            }
        }
    }
}

impl std::error::Error for LedgerError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_unbalanced_positive() {
        let err = LedgerError::Unbalanced { actual_sum: 1050 };
        assert_eq!(
            err.to_string(),
            "Transaction is unbalanced: entries sum to $10.50 instead of $0.00"
        );
    }

    #[test]
    fn display_unbalanced_negative() {
        let err = LedgerError::Unbalanced { actual_sum: -350 };
        assert_eq!(
            err.to_string(),
            "Transaction is unbalanced: entries sum to $-3.50 instead of $0.00"
        );
    }

    #[test]
    fn display_insufficient_funds() {
        let err = LedgerError::InsufficientFunds {
            account: "Savings".to_string(),
            available: 5000,
            requested: 20000,
        };
        assert_eq!(
            err.to_string(),
            "Insufficient funds: Savings has $50.00, requested $200.00"
        );
    }

    #[test]
    fn display_account_not_found() {
        let err = LedgerError::AccountNotFound {
            name: "Phantom".to_string(),
        };
        assert_eq!(err.to_string(), "Account not found: Phantom");
    }

    #[test]
    fn display_overflow() {
        let err = LedgerError::Overflow;
        assert_eq!(
            err.to_string(),
            "Arithmetic overflow: operation would exceed i64 bounds"
        );
    }

    #[test]
    fn display_invalid_amount() {
        let err = LedgerError::InvalidAmount;
        assert_eq!(
            err.to_string(),
            "Invalid amount: entry amount must not be zero"
        );
    }

    #[test]
    fn display_duplicate_account() {
        let err = LedgerError::DuplicateAccount {
            name: "Checking".to_string(),
        };
        assert_eq!(err.to_string(), "Duplicate account: Checking already exists");
    }

    #[test]
    fn error_trait_is_implemented() {
        let err: &dyn std::error::Error = &LedgerError::Overflow;
        // source() should return None — these are leaf errors
        assert!(err.source().is_none());
    }
}
