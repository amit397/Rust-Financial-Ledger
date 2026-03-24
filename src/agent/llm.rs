use crate::agent::{Agent, AgentProposal, AgentError};
use crate::ledger::Entry;
use crate::ledger::AccountId;
use mistralrs::{GgufModelBuilder, TextMessageRole, TextMessages};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub struct LlmAgent {
    model_path: String,
    max_retries: usize,
    timeout_secs: u64,
    known_accounts: Vec<String>,
}

impl LlmAgent {
    /// Create a new LLM agent. Returns Err if model file not found.
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
        for attempt in 0..=self.max_retries {
            let (tx, rx) = mpsc::channel::<String>();

            let model_path = self.model_path.clone();
            let prompt = build_prompt(input, &self.known_accounts);

            thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build tokio runtime");

    // 2. Block the background thread while the async runtime executes the AI math
    rt.block_on(async {
        // 3. Initialize the model pipeline (pointing it to the downloaded file)
        // Assuming self.model_path is the directory like "./models"
        let model = match GgufModelBuilder::new(
            model_path, 
            vec!["Phi-3-mini-4k-instruct-q4.gguf".to_string()]
        )
        .build()
        .await 
        {
            Ok(m) => m,
            Err(e) => {
                eprintln!("\n[LLM Error] Engine failed to load: {}", e);
                return; // Exits the thread, causing a timeout/disconnect in the main thread
            }
        };

        // 4. Package our strict engineered prompt into a message
        let messages = TextMessages::new()
            .add_message(TextMessageRole::User, &prompt);

        // 5. Run the inference! 
        if let Ok(response) = model.send_chat_request(messages).await {
            // Extract the actual string text from the AI's response object
            if let Some(generated_text) = response.choices.first().and_then(|c| c.message.content.clone()) {
                
                // 6. Shove the result into the mpsc pneumatic tube back to the main thread!
                tx.send(generated_text).ok();
            }
        }
    });
            });
            
            let timeout = Duration::from_secs(self.timeout_secs);
        
        match rx.recv_timeout(timeout) {
            // Success! The thread finished before the 30 seconds were up.
            Ok(raw_json) => {
                match parse_proposal(&raw_json, &self.known_accounts) {
                    Ok(proposal) => return Ok(proposal), // Perfect parse, we are done!
                    Err(e) => {
                        // The LLM output garbage JSON. Should we try again?
                        if attempt < self.max_retries {
                            // Silent retry: We don't crash, we just loop again.
                            continue; 
                        } else {
                            // We are out of retries. Bubble the error up to the user.
                            return Err(e);
                        }
                    }
                }
            }
            // The model took too long, or the thread crashed (disconnected).
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(AgentError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err(AgentError::ParseFailure("LLM thread crashed".to_string())),
        }
    }
    
    unreachable!("Loop should always return or error out")
        
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

    let entries = parsed.entries.into_iter().map(|e| Entry {
        account: AccountId(e.account),
        amount: e.amount_cents,
    }).collect();

    Ok(AgentProposal {
        description: parsed.description,
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
    let accounts_list = known_accounts.join(", ");

    let system_prompt =  format!(
        "Output ONLY JSON. You are a strict financial ledger assistant.\n\
        Your job is to translate user requests into balanced double-entry bookkeeping JSON.\n\
        \n\
        SCHEMA:\n\
        {{\n\
          \"description\": \"short human-readable description\",\n\
          \"entries\": [\n\
            {{ \"account\": \"AccountName\", \"amount_cents\": -5000 }},\n\
            {{ \"account\": \"AccountName\", \"amount_cents\": 5000 }}\n\
          ]\n\
        }}\n\
        \n\
        RULES:\n\
        1. Output ONLY the raw JSON object. No prose. No markdown fences (```).\n\
        2. `amount_cents` must be an integer (no decimal points).\n\
        3. Negative `amount_cents` = debit (money leaves).\n\
        4. Positive `amount_cents` = credit (money enters).\n\
        5. The entries MUST sum to exactly zero.\n\
        6. Available accounts: {accounts_list}.\n"
    );

    let examples = "\
    Input: move $50 from Checking to Savings\n\
        Output: { \"description\": \"transfer to savings\", \"entries\": [ { \"account\": \"Checking\", \"amount_cents\": -5000 }, { \"account\": \"Savings\", \"amount_cents\": 5000 } ] }\n\
        \n\
        Input: deposited a $1000 check from work\n\
        Output: { \"description\": \"paycheck deposit\", \"entries\": [ { \"account\": \"External\", \"amount_cents\": -100000 }, { \"account\": \"Checking\", \"amount_cents\": 100000 } ] }\n\
        \n\
        Input: move fifty bucks to savings from checking\n\
        Output: { \"description\": \"transfer to savings\", \"entries\": [ { \"account\": \"Checking\", \"amount_cents\": -5000 }, { \"account\": \"Savings\", \"amount_cents\": 5000 } ] }\n\
        \n\
        Input: transfer negative $50 to Checking\n\
        Output: { \"error\": \"cannot process negative transfer\" }\n\
        \n\
        Input: pay rent\n\
        Output: { \"error\": \"ambiguous request: missing source or destination account\" }\n";
    
    format!("{}\n{}\nInput: {}\nOutput:\n", system_prompt, examples, input)
}

impl Agent for LlmAgent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError> {
        self.call_with_retry(input)
    }

    fn name(&self) -> &str {
        "LlmAgent"
    }
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
        let raw = r#"{ "description": "buy groceries" }"#; // missing entries
        assert!(parse_proposal(raw, &[]).is_err());
    }

    #[test]
    fn parse_proposal_empty_entries_fails() {
        let raw = r#"{ "description": "buy", "entries": [] }"#;
        assert!(parse_proposal(raw, &[]).is_err());
    }
}
