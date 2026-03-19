use crate::ledger::Entry;

pub mod mock;
pub mod llm;
pub mod chaos;

/// An unvalidated proposal from an agent.
#[derive(Debug, Clone)]
pub struct AgentProposal {
    pub description: String,
    pub entries: Vec<Entry>,
}

/// All errors an agent can produce before the ledger validates the proposal.
#[derive(Debug)]
pub enum AgentError {
    ParseFailure(String),
    Timeout,
    InferenceFailed(String),
}

/// All agents implement this trait. The ledger treats all implementations identically.
pub trait Agent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;
    fn name(&self) -> &str;
}
