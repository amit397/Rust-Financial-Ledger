use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::Entry;
use crate::ledger::AccountId;

pub struct MockAgent {
    pub known_accounts: Vec<String>,
}

pub fn to_transaction(proposal: AgentProposal) -> Result<crate::ledger::Transaction, crate::ledger::LedgerError> {
    crate::ledger::Transaction::new(proposal.description, proposal.entries)
}

impl Agent for MockAgent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError> {
        let trimmed = input.trim();
        let lower = trimmed.to_lowercase();
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        
        if tokens.len() == 6 && tokens[0] == "transfer" && tokens[2] == "from" && tokens[4] == "to" {
            let amount = parse_dollars(tokens[1])?;
            let source = find_account(&self.known_accounts, tokens[3]);
            let dest = find_account(&self.known_accounts, tokens[5]);
            
            return Ok(AgentProposal {
                description: trimmed.to_string(),
                entries: vec![
                    Entry { account: AccountId(source), amount: -amount },
                    Entry { account: AccountId(dest), amount },
                ],
            });
        }
        
        if tokens.len() == 4 && tokens[0] == "deposit" && tokens[2] == "to" {
            let amount = parse_dollars(tokens[1])?;
            let dest = find_account(&self.known_accounts, tokens[3]);
            
            return Ok(AgentProposal {
                description: trimmed.to_string(),
                entries: vec![
                    Entry { account: AccountId("External".to_string()), amount: -amount },
                    Entry { account: AccountId(dest), amount },
                ],
            });
        }
        
        if tokens.len() == 4 && tokens[0] == "withdraw" && tokens[2] == "from" {
            let amount = parse_dollars(tokens[1])?;
            let source = find_account(&self.known_accounts, tokens[3]);
            
            return Ok(AgentProposal {
                description: trimmed.to_string(),
                entries: vec![
                    Entry { account: AccountId(source), amount: -amount },
                    Entry { account: AccountId("External".to_string()), amount },
                ],
            });
        }
        
        Err(AgentError::ParseFailure("Unrecognized command. Try: transfer $X from A to B".to_string()))
    }
    
    fn name(&self) -> &str {
        "MockAgent"
    }
}

fn parse_dollars(s: &str) -> Result<i64, AgentError> {
    let s = s.trim_start_matches('$');
    if s.starts_with('-') {
        return Err(AgentError::ParseFailure("Negative amounts not allowed".to_string()));
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() > 2 {
        return Err(AgentError::ParseFailure("Invalid amount".to_string()));
    }
    let dollars: i64 = parts[0].parse().map_err(|_| AgentError::ParseFailure("Invalid amount".to_string()))?;
    let mut cents: i64 = 0;
    if parts.len() == 2 {
        let mut cent_str = parts[1].to_string();
        if cent_str.len() == 1 {
            cent_str.push('0');
        } else if cent_str.len() > 2 {
            cent_str.truncate(2);
        }
        cents = cent_str.parse().map_err(|_| AgentError::ParseFailure("Invalid amount".to_string()))?;
    }
    Ok(dollars * 100 + cents)
}

fn find_account(known: &[String], target: &str) -> String {
    let target_lower = target.to_lowercase();
    for acc in known {
        if acc.to_lowercase() == target_lower {
            return acc.clone();
        }
    }
    if target_lower == "external" {
        return "External".to_string();
    }
    
    // Attempt original casing format if not found
    let mut chars = target.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> MockAgent {
        MockAgent {
            known_accounts: vec!["Checking".to_string(), "Savings".to_string()],
        }
    }

    #[test]
    fn test_valid_transfer() {
        let a = agent();
        let prop = a.propose("transfer $25 from checking to savings").unwrap();
        assert_eq!(prop.entries.len(), 2);
        assert_eq!(prop.entries[0].amount, -2500);
        assert_eq!(prop.entries[1].amount, 2500);
    }

    #[test]
    fn test_valid_deposit() {
        let a = agent();
        let prop = a.propose("deposit $100 to checking").unwrap();
        assert_eq!(prop.entries.len(), 2);
        assert_eq!(prop.entries[0].amount, -10000); // External
        assert_eq!(prop.entries[1].amount, 10000);
    }

    #[test]
    fn test_amount_parsing() {
        let a = agent();
        assert_eq!(a.propose("deposit $25 to checking").unwrap().entries[1].amount, 2500);
        assert_eq!(a.propose("deposit $25.50 to checking").unwrap().entries[1].amount, 2550);
        assert_eq!(a.propose("deposit $25.5 to checking").unwrap().entries[1].amount, 2550);
        assert_eq!(a.propose("deposit $0.01 to checking").unwrap().entries[1].amount, 1);
        assert!(a.propose("deposit $-25 to checking").is_err());
    }

    #[test]
    fn test_unrecognized() {
        let a = agent();
        assert!(a.propose("do something").is_err());
    }
}
