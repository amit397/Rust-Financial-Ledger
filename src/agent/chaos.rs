use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::{Entry, AccountId};

pub struct EmptyAgent;
impl Agent for EmptyAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Empty entries".to_string(),
            entries: vec![],
        })
    }
    fn name(&self) -> &str { "EmptyAgent" }
}

pub struct ZeroAmountAgent;
impl Agent for ZeroAmountAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Zero amount".to_string(),
            entries: vec![
                Entry { account: AccountId("A".to_string()), amount: 0 },
                Entry { account: AccountId("B".to_string()), amount: 0 },
            ],
        })
    }
    fn name(&self) -> &str { "ZeroAmountAgent" }
}

pub struct UnbalancedAgent;
impl Agent for UnbalancedAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Sum != 0".to_string(),
            entries: vec![
                Entry { account: AccountId("A".to_string()), amount: -50 },
                Entry { account: AccountId("B".to_string()), amount: 30 },
            ],
        })
    }
    fn name(&self) -> &str { "UnbalancedAgent" }
}

pub struct OverflowAgent;
impl Agent for OverflowAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Overflow check".to_string(),
            entries: vec![
                Entry { account: AccountId("A".to_string()), amount: i64::MAX },
                Entry { account: AccountId("B".to_string()), amount: 10 },
            ],
        })
    }
    fn name(&self) -> &str { "OverflowAgent" }
}

pub struct NonExistentAccountAgent;
impl Agent for NonExistentAccountAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Ghost account".to_string(),
            entries: vec![
                Entry { account: AccountId("External".to_string()), amount: -100 },
                Entry { account: AccountId("GhostAccount".to_string()), amount: 100 },
            ],
        })
    }
    fn name(&self) -> &str { "NonExistentAccountAgent" }
}

pub struct InsufficientFundsAgent;
impl Agent for InsufficientFundsAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Ok(AgentProposal {
            description: "Billion dollar debit".to_string(),
            entries: vec![
                Entry { account: AccountId("Checking".to_string()), amount: -1_000_000_000_000 },
                Entry { account: AccountId("External".to_string()), amount: 1_000_000_000_000 },
            ],
        })
    }
    fn name(&self) -> &str { "InsufficientFundsAgent" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_agent() {
        let agent = EmptyAgent;
        let p = agent.propose("").unwrap();
        assert!(p.entries.is_empty());
    }
    #[test]
    fn test_zero_amount_agent() {
        let agent = ZeroAmountAgent;
        let p = agent.propose("").unwrap();
        assert_eq!(p.entries[0].amount, 0);
    }
    #[test]
    fn test_unbalanced_agent() {
        let agent = UnbalancedAgent;
        let p = agent.propose("").unwrap();
        let sum: i64 = p.entries.iter().map(|e| e.amount).sum();
        assert_ne!(sum, 0);
    }
    #[test]
    fn test_overflow_agent() {
        let agent = OverflowAgent;
        let p = agent.propose("").unwrap();
        assert_eq!(p.entries[0].amount, i64::MAX);
    }
    #[test]
    fn test_non_existent_account_agent() {
        let agent = NonExistentAccountAgent;
        let p = agent.propose("").unwrap();
        assert_eq!(p.entries[1].account.0, "GhostAccount");
    }
    #[test]
    fn test_insufficient_funds_agent() {
        let agent = InsufficientFundsAgent;
        let p = agent.propose("").unwrap();
        assert!(p.entries[0].amount < -1_000_000);
    }
}
