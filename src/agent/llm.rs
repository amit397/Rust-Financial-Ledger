use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::Entry;
use crate::ledger::AccountId;

#[derive(Debug)]
pub struct LlmAgent {
    _known_accounts: Vec<String>,
}

impl LlmAgent {
    /// Create a new LLM agent.
    ///
    /// The LLM backend is disabled in this build. Use `--mock` for the interactive demo,
    /// or `--stress` for the concurrent stress test.
    pub fn new(_model_path: &str, _known_accounts: Vec<String>) -> Result<Self, String> {
        Err(
            "LLM backend is disabled in this build.\n\
             Use --mock for the interactive CLI demo.\n\
             Use --stress for the concurrent stress test.\n\
             See README for full LLM integration instructions."
                .to_string(),
        )
    }
}

impl Agent for LlmAgent {
    fn propose(&self, _input: &str) -> Result<AgentProposal, AgentError> {
        Err(AgentError::InferenceFailed(
            "LLM backend disabled".to_string(),
        ))
    }

    fn name(&self) -> &str {
        "LlmAgent"
    }
}

/// Intermediate deserialization types for untrusted LLM JSON.
/// Kept separate from domain types to prevent serde attributes on core ledger types.
#[derive(serde::Deserialize)]
struct RawProposal {
    description: String,
    entries: Vec<RawEntry>,
}

#[derive(serde::Deserialize)]
struct RawEntry {
    account: String,
    amount_cents: i64,
}

pub fn parse_proposal(raw: &str, _known_accounts: &[String]) -> Result<AgentProposal, AgentError> {
    let parsed: RawProposal = serde_json::from_str(raw)
        .map_err(|e| AgentError::ParseFailure(e.to_string()))?;

    if parsed.entries.is_empty() {
        return Err(AgentError::ParseFailure("Empty entries".to_string()));
    }

    let entries = parsed
        .entries
        .into_iter()
        .map(|e| Entry {
            account: AccountId(e.account),
            amount: e.amount_cents,
        })
        .collect();

    Ok(AgentProposal {
        description: parsed.description,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proposal_success() {
        let raw = r#"{
            "description": "buy groceries",
            "entries": [
                {"account": "Checking", "amount_cents": -5000},
                {"account": "Groceries", "amount_cents": 5000}
            ]
        }"#;
        let prop = parse_proposal(raw, &[]).unwrap();
        assert_eq!(prop.description, "buy groceries");
        assert_eq!(prop.entries.len(), 2);
        assert_eq!(prop.entries[0].amount, -5000);
        assert_eq!(prop.entries[0].account.0, "Checking");
    }

    #[test]
    fn parse_proposal_missing_fields_fails() {
        let raw = r#"{ "description": "buy groceries" }"#;
        assert!(parse_proposal(raw, &[]).is_err());
    }

    #[test]
    fn parse_proposal_empty_entries_fails() {
        let raw = r#"{ "description": "buy", "entries": [] }"#;
        assert!(parse_proposal(raw, &[]).is_err());
    }

    #[test]
    fn llm_agent_new_returns_error() {
        let result = LlmAgent::new("models", vec!["Checking".into()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("disabled"));
    }
}
