use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::Entry;
use crate::ledger::AccountId;

pub struct LlmAgent {
    model_path: String,
    max_retries: usize,
    timeout_secs: u64,
    known_accounts: Vec<String>,
}

impl LlmAgent {
    /// Create a new LLM agent. Returns Err with clear instructions if model file not found.
    /// The error message must be copy-pasteable: tell the user exactly where to download the model.
    pub fn new(model_path: &str, known_accounts: Vec<String>) -> Result<Self, String> {
        if !std::path::Path::new(model_path).exists() {
            return Err(format!(
                "Model file not found: {}\n\
                 To download: run ./setup.sh\n\
                 Or manually: download Phi-3-mini-4bit GGUF to models/\n\
                 To skip the model entirely: cargo run -- --mock",
                model_path
            ));
        }
        Ok(Self {
            model_path: model_path.to_string(),
            max_retries: 2,
            timeout_secs: 30,
            known_accounts,
        })
    }

    fn call_with_retry(&self, input: &str) -> Result<AgentProposal, AgentError> {
        // UNIMPLEMENTED — Developer task (retry loop + timeout)
        //
        // WHY SILENT RETRY?
        //   LLM output is non-deterministic (temperature > 0 means same input → different output).
        //   A query that produces malformed JSON on attempt 1 may produce valid JSON on attempt 2.
        //   Silent retry (no user-visible message) buys ~10-15% parse rate improvement for free.
        //   The retry count is logged in benchmark results — it's an engineering trade-off,
        //   documented honestly, not a hidden cheat. Max retries = 2 (diminishing returns after that).
        //
        // WHY A 30-SECOND TIMEOUT?
        //   Quantized models on CPU can take 60+ seconds if they enter a repetition loop
        //   or receive a prompt that triggers long completions. Without a timeout the CLI
        //   hangs indefinitely — a terrible user experience. 30 seconds is long enough for
        //   normal CPU inference on Phi-3-mini, short enough to feel responsive on failure.
        //
        // IMPLEMENTATION USING std::sync::mpsc (no tokio required):
        //
        //   for attempt in 0..=self.max_retries {
        //       let (tx_chan, rx_chan) = std::sync::mpsc::channel::<String>();
        //       let model_path = self.model_path.clone();
        //       let prompt = build_prompt(input, &self.known_accounts);
        //
        //       std::thread::spawn(move || {
        //           // Replace this with actual model inference call:
        //           // let output = your_model_backend.infer(&prompt);
        //           // tx_chan.send(output).ok();
        //           todo!("call your chosen LLM backend here — mistralrs or llama-cpp-rs");
        //       });
        //
        //       let timeout = std::time::Duration::from_secs(self.timeout_secs);
        //       match rx_chan.recv_timeout(timeout) {
        //           Err(_) => return Err(AgentError::Timeout),
        //           Ok(raw) => match parse_proposal(&raw, &self.known_accounts) {
        //               Ok(proposal) => return Ok(proposal),
        //               Err(e) if attempt < self.max_retries => {
        //                   eprintln!("[LLM] Parse failure on attempt {}, retrying...", attempt + 1);
        //                   continue;
        //               }
        //               Err(e) => return Err(e),
        //           }
        //       }
        //   }
        //   unreachable!()
        //
        // NOTE ON FFI SEGFAULTS (if using llama-cpp-rs):
        //   A segfault in the C++ backend CANNOT be caught by catch_unwind or the timeout.
        //   The entire process exits. This is a known, documented limitation.
        //   Production fix: isolate the model in a subprocess, communicate via IPC.
        //   For this project: document it in README. The limitation itself demonstrates
        //   understanding of FFI safety boundaries — valuable in an interview.
        todo!("call_with_retry — 2-attempt retry loop with 30s per-attempt timeout")
    }
}

pub fn parse_proposal(raw: &str, _known_accounts: &[String]) -> Result<AgentProposal, AgentError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| AgentError::ParseFailure(e.to_string()))?;
    
    let description = v.get("description")
        .and_then(|d| d.as_str())
        .ok_or_else(|| AgentError::ParseFailure("Missing description".to_string()))?
        .to_string();
        
    let entries_arr = v.get("entries")
        .and_then(|e| e.as_array())
        .ok_or_else(|| AgentError::ParseFailure("Missing entries".to_string()))?;
        
    if entries_arr.is_empty() {
        return Err(AgentError::ParseFailure("Empty entries".to_string()));
    }
    
    let mut entries = Vec::new();
    for entry in entries_arr {
        let account = entry.get("account")
            .and_then(|a| a.as_str())
            .ok_or_else(|| AgentError::ParseFailure("Missing account".to_string()))?
            .to_string();
            
        let amount = entry.get("amount_cents")
            .and_then(|a| a.as_i64())
            .ok_or_else(|| AgentError::ParseFailure("Missing amount_cents".to_string()))?;
            
        entries.push(Entry {
            account: AccountId(account),
            amount,
        });
    }
    
    Ok(AgentProposal {
        description,
        entries,
    })
}

fn build_prompt(input: &str, known_accounts: &[String]) -> String {
    // UNIMPLEMENTED — Developer task (prompt engineering)
    //
    // WHAT IS A SYSTEM PROMPT?
    //   The system prompt establishes the model's role and output contract before
    //   any user input arrives. Small quantized models (Phi-3-mini 4-bit) fail
    //   without explicit structure guidance — they'll add prose, markdown fences,
    //   or explanation unless told not to. Without examples, parse rate drops from
    //   ~85% to ~40% on paraphrased inputs.
    //
    // THE JSON SCHEMA THE MODEL MUST OUTPUT (document this clearly in the prompt):
    //   {
    //     "description": "short human-readable description",
    //     "entries": [
    //       { "account": "AccountName", "amount_cents": -5000 },
    //       { "account": "AccountName", "amount_cents":  5000 }
    //     ]
    //   }
    //   Rules to state explicitly:
    //     - amount_cents is an integer (no decimal points)
    //     - negative = debit (money leaves this account)
    //     - positive = credit (money enters this account)
    //     - entries must sum to exactly zero
    //     - output ONLY the JSON object, no prose, no markdown fences
    //
    // WHAT IS A FEW-SHOT EXAMPLE?
    //   Include 5 examples inline showing Input → Output pairs.
    //   Example types to cover:
    //     1. Simple transfer between two named accounts
    //     2. A deposit (from External)
    //     3. A paraphrase ("move fifty bucks to savings")
    //     4. An adversarial input ("transfer negative $50") → model should output
    //        a JSON error signal, not a transaction. Define a convention: e.g.
    //        { "error": "cannot process negative transfer" }
    //     5. An ambiguous input ("pay rent") → ask for clarification via error signal
    //
    // AVAILABLE ACCOUNTS (inject into prompt so model uses real names):
    //   Format as: "Available accounts: Checking, Savings, External, ..."
    //   Models that don't see the account list invent names → parse succeeds but
    //   Ledger::apply rejects with AccountNotFound.
    //
    // STRUCTURE OF THE FULL PROMPT STRING:
    //   [SYSTEM]: role + output rules + JSON schema + available accounts
    //   [EXAMPLES]: 5 input→output pairs
    //   [USER]: "Input: {input}\nOutput:"
    //   (The trailing "Output:" primes the model to complete with JSON immediately)
    //
    // TIPS FROM RUNNING SMALL MODELS:
    //   - "Output ONLY JSON" must be the FIRST sentence of the system block.
    //   - Restate the zero-sum rule: "entries must sum to zero" — models forget this.
    //   - Use the word "cents" in the field name (models confuse dollars vs cents).
    //   - Keep examples short — long examples fill context and reduce accuracy on later input.
    //
    // AFTER IMPLEMENTING: test parse rate against benches/corpus/paraphrased.json.
    // Target: ≥85% first-attempt parse rate, ≥95% after retry.
    todo!("build_prompt — write system prompt + 5 few-shot examples with JSON schema")
}

impl Agent for LlmAgent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError> {
        self.call_with_retry(input)
    }

    fn name(&self) -> &str {
        "LlmAgent"
    }
}
