use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::{AccountId, Entry};
use rand::prelude::IndexedRandom;
use rand::seq::SliceRandom;
use rand::Rng;

pub struct ValidAgent {
    accounts: Vec<String>,
}
impl ValidAgent {
    pub fn new(accounts: Vec<String>) -> Self { Self { accounts } }
}
impl Agent for ValidAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let is_deposit = rng.random_bool(0.2);
        
        if is_deposit {
            let dest = self.accounts.iter().filter(|a| *a != "External").collect::<Vec<_>>();
            let dest_acc = dest.choose(&mut rng).unwrap().to_string();
            let amount = rng.random_range(100..=10000);
            Ok(AgentProposal {
                description: "Deposit".to_string(),
                entries: vec![
                    Entry { account: AccountId("External".to_string()), amount: -amount },
                    Entry { account: AccountId(dest_acc), amount },
                ]
            })
        } else {
            let mut accs = self.accounts.iter().filter(|a| *a != "External").collect::<Vec<_>>();
            if accs.len() < 2 {
                return Err(AgentError::ParseFailure("Need at least 2 non-External accounts".into()));
            }
            accs.shuffle(&mut rng);
            let src = accs[0].to_string();
            let dest = accs[1].to_string();
            let amount = rng.random_range(1..=5000);
            Ok(AgentProposal {
                description: "Transfer".to_string(),
                entries: vec![
                    Entry { account: AccountId(src), amount: -amount },
                    Entry { account: AccountId(dest), amount },
                ]
            })
        }
    }
    fn name(&self) -> &str { "ValidAgent" }
}

pub struct OverdraftAgent {
    accounts: Vec<String>,
}
impl OverdraftAgent {
    pub fn new(accounts: Vec<String>) -> Self { Self { accounts } }
}
impl Agent for OverdraftAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let mut accs = self.accounts.clone();
        accs.shuffle(&mut rng);
        let src = accs[0].clone();
        let dest = accs[1].clone();
        let amount = rng.random_range(1_000_000..=100_000_000);
        Ok(AgentProposal {
            description: "Overdraft".to_string(),
            entries: vec![
                Entry { account: AccountId(src), amount: -amount },
                Entry { account: AccountId(dest), amount },
            ]
        })
    }
    fn name(&self) -> &str { "OverdraftAgent" }
}

pub struct TypoAgent {
    accounts: Vec<String>,
}
impl TypoAgent {
    pub fn new(accounts: Vec<String>) -> Self { Self { accounts } }
}
impl Agent for TypoAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let mut dest = self.accounts.choose(&mut rng).unwrap().clone();
        if dest.len() >= 2 {
            let pos = rng.random_range(0..dest.len()-1);
            let mut chars: Vec<char> = dest.chars().collect();
            chars.swap(pos, pos+1);
            dest = chars.into_iter().collect();
        } else {
            dest.push('X');
        }
        Ok(AgentProposal {
            description: "Typo".to_string(),
            entries: vec![
                Entry { account: AccountId("External".to_string()), amount: -100 },
                Entry { account: AccountId(dest), amount: 100 },
            ]
        })
    }
    fn name(&self) -> &str { "TypoAgent" }
}

pub struct OverflowAgent {
    accounts: Vec<String>,
}
impl OverflowAgent {
    pub fn new(accounts: Vec<String>) -> Self { Self { accounts } }
}
impl Agent for OverflowAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let strat = rng.random_range(0..3);
        let src = self.accounts.choose(&mut rng).unwrap().clone();
        let dest = self.accounts.choose(&mut rng).unwrap().clone();
        let dest2 = self.accounts.choose(&mut rng).unwrap().clone();
        
        let entries = match strat {
            0 => vec![
                Entry { account: AccountId(src.clone()), amount: (i64::MAX / 2) + 1 },
                Entry { account: AccountId(dest.clone()), amount: -((i64::MAX / 2) + 1) },
            ],
            1 => vec![
                Entry { account: AccountId(src.clone()), amount: i64::MAX },
                Entry { account: AccountId(dest.clone()), amount: -i64::MAX },
            ],
            _ => vec![
                Entry { account: AccountId(src.clone()), amount: i64::MAX - 100 },
                Entry { account: AccountId(dest.clone()), amount: 50 },
                Entry { account: AccountId(dest2.clone()), amount: 51 }, // 50+51=101 causes overflow when summed
            ]
        };
        Ok(AgentProposal {
            description: "Overflow".to_string(),
            entries
        })
    }
    fn name(&self) -> &str { "OverflowAgent" }
}

pub struct UnbalancedAgent {
    accounts: Vec<String>,
}
impl UnbalancedAgent {
    pub fn new(accounts: Vec<String>) -> Self { Self { accounts } }
}
impl Agent for UnbalancedAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let src = self.accounts.choose(&mut rng).unwrap().clone();
        let dest = self.accounts.choose(&mut rng).unwrap().clone();
        let amount1 = rng.random_range(100..5000);
        let amount2 = rng.random_range(100..5000); // independent, likely not balanced
        
        Ok(AgentProposal {
            description: "Unbalanced".to_string(),
            entries: vec![
                Entry { account: AccountId(src), amount: -amount1 },
                Entry { account: AccountId(dest), amount: amount2 },
            ]
        })
    }
    fn name(&self) -> &str { "UnbalancedAgent" }
}

pub struct ChaosAgent {
    valid: ValidAgent,
    overdraft: OverdraftAgent,
    typo: TypoAgent,
    overflow: OverflowAgent,
    unbalanced: UnbalancedAgent,
}
impl ChaosAgent {
    pub fn new(accounts: Vec<String>) -> Self {
        Self {
            valid: ValidAgent::new(accounts.clone()),
            overdraft: OverdraftAgent::new(accounts.clone()),
            typo: TypoAgent::new(accounts.clone()),
            overflow: OverflowAgent::new(accounts.clone()),
            unbalanced: UnbalancedAgent::new(accounts.clone()),
        }
    }
}
impl Agent for ChaosAgent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError> {
        let mut rng = rand::rng();
        let choice = rng.random_range(0..100);
        if choice < 40 {
            self.valid.propose(input)
        } else if choice < 55 {
            self.overdraft.propose(input)
        } else if choice < 70 {
            self.typo.propose(input)
        } else if choice < 80 {
            self.overflow.propose(input)
        } else if choice < 90 {
            self.unbalanced.propose(input)
        } else {
            // Intentionally includes zero and extreme values — chaos testing
            Ok(AgentProposal {
                description: "Garbage".to_string(),
                entries: vec![
                    Entry { account: AccountId("Fake1".to_string()), amount: rng.random() },
                    Entry { account: AccountId("Fake2".to_string()), amount: rng.random() },
                ]
            })
        }
    }
    fn name(&self) -> &str { "ChaosAgent" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accounts() -> Vec<String> {
        vec!["Checking".into(), "Savings".into(), "External".into()]
    }

    #[test]
    fn test_valid() {
        let a = ValidAgent::new(accounts());
        for _ in 0..100 {
            let p = a.propose("").unwrap();
            assert_eq!(p.entries.len(), 2);
            let sum: i64 = p.entries.iter().map(|e| e.amount).sum();
            assert_eq!(sum, 0);
        }
    }
    #[test]
    fn test_overdraft() {
        let a = OverdraftAgent::new(accounts());
        let p = a.propose("").unwrap();
        assert!(p.entries[0].amount.abs() >= 1_000_000);
    }
    #[test]
    fn test_typo() {
        let a = TypoAgent::new(accounts());
        let p = a.propose("").unwrap();
        assert_eq!(p.entries.len(), 2);
    }
    #[test]
    fn test_overflow() {
        let a = OverflowAgent::new(accounts());
        for _ in 0..100 {
            let p = a.propose("").unwrap();
            if p.entries.iter().any(|e| e.amount.abs() > i64::MAX / 4) {
                return;
            }
        }
        panic!("100 tests, none had big overflow amounts");
    }
    #[test]
    fn test_unbalanced() {
        let a = UnbalancedAgent::new(accounts());
        let mut unbal = false;
        for _ in 0..100 {
            let p = a.propose("").unwrap();
            let sum: i64 = p.entries.iter().map(|e| e.amount).sum();
            if sum != 0 { unbal = true; }
        }
        assert!(unbal, "100 unbalanced proposals all accidentally balanced!");
    }
    #[test]
    fn test_chaos() {
        let a = ChaosAgent::new(accounts());
        for _ in 0..1000 {
            let _ = a.propose("").unwrap();
        }
    }
    #[test]
    fn test_valid_agent_insufficient_accounts() {
        let a = ValidAgent::new(vec!["External".into(), "Solo".into()]);
        // With only 1 non-External account, transfer path should error, not panic
        let mut saw_error = false;
        for _ in 0..20 {
            match a.propose("") {
                Err(_) => { saw_error = true; break; }
                Ok(_) => {} // deposit path still works
            }
        }
        assert!(saw_error, "Expected an error from ValidAgent with only 1 non-External account");
    }
}
