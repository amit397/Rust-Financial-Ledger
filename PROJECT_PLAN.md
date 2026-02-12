# LedgerGuard — Project Plan

> A local-first CLI tool with a Rust-native LLM agent where every agent action is validated against financial invariants before execution. The focus is on **safety guarantees**, not agent intelligence.

## Core Thesis

**"Build a bulletproof guard that makes a dumb agent safe."**

The LLM will be wrong ~30% of the time. The ledger will catch 100% of those errors. The project demonstrates how to build safe AI agents for high-stakes domains by combining compile-time correctness with runtime verification.

**Why trust the ledger?** Two layers of defense, each independently verifiable:
1. **Construction-time invariants** — `Transaction::new` rejects invalid entries before they exist.
2. **Stateful validation** — `Ledger::apply` checks funds and account existence before committing.
3. **Verification** — 100% unit test coverage of error paths, property-based tests (`proptest`) proving replay consistency, and fuzzing of the untrusted-input boundary.

---

## Architecture

```
┌──────────────┐   natural language   ┌──────────────────┐
│  User (CLI)  │ ──────────────────►  │  Agent (LLM)     │
│  rustyline   │                      │  mistralrs or    │
│              │                      │  llama.cpp FFI   │
└──────┬───────┘                      └────────┬─────────┘
       ▲                                       │
       │                                       ▼
       │                              ┌──────────────────┐
       │                              │ JSON Proposal    │
       │                              │ (parsed + retry  │
       │                              │  up to 2x on     │
       │                              │  parse failure)  │
       │                              └────────┬─────────┘
       │                                       │
       │                                       ▼
       │                              ┌──────────────────┐
       │                              │  Ledger Core     │
       │                              │  Transaction::new│
       │                              │  → Result<T, E>  │
       │                              └────────┬─────────┘
       │                                       │
       └───────────────────────────────────────┘
                  ✅ success / ❌ structured error
```

| Component | Role | Key Detail |
|-----------|------|------------|
| **Ledger Core** (`ledger/`) | Validates & stores transactions | `Transaction::new(entries) → Result<Transaction, LedgerError>` — invariants enforced at construction |
| **Agent** (`agent/`) | Translates natural language → JSON proposals | Wraps LLM inference with timeout (30s), retry (2x on parse failure), crash recovery |
| **CLI** (`cli/`) | User-facing REPL | `rustyline`; displays agent proposals + ledger verdicts; handles operational errors gracefully |
| **Persistence** | Durable event log | `serde_json` serialize `Vec<Transaction>` to file on exit, load on startup. Atomic write (temp file → rename) |
| **Benchmarks** (`benches/`) | Quantified evaluation | 200+ query corpus with per-category accuracy reporting |

---

## Metrics — Agent vs. Ledger (Separated)

Agent accuracy and ledger correctness are fundamentally different claims and must never be conflated.

### Agent Metrics (measured, imperfect — reported honestly, not pass/fail)

Agent metrics are **published as-is**, not treated as success/failure thresholds. If parse rate is 72%, that's the number we report. The project's value comes from the ledger catching every invalid attempt, not from the agent being perfect.

| Metric | Definition | Reported As |
|--------|-----------|-------------|
| **Parse rate** | % of queries where agent produces valid JSON (before retry) | Exact %, per-category |
| **Parse rate after retry** | % with up to 2 silent retries | Exact % |
| **Agent accuracy (valid queries)** | % of intended-valid queries where agent proposes a correct transaction | Exact %, per-category |
| **Inference latency** | Time from query submission to agent response | P50 and P95, in milliseconds |

### Ledger Metrics (guaranteed, by construction)

| Metric | Definition | Target |
|--------|-----------|--------|
| **True-positive rate** | Ledger accepts a valid transaction | **100%** |
| **True-negative rate** | Ledger rejects an invalid transaction | **100%** |

If ledger metrics are ever below 100%, the invariant enforcement is broken — that's a bug, not a trade-off.

---

## Benchmark Corpus Design (200+ Queries)

| Category | Count | Purpose | Examples |
|----------|-------|---------|----------|
| **Templated** | 100 | Baseline parse/accuracy rate | "Transfer {amount} from {account_a} to {account_b}" with randomized values |
| **Paraphrased** | 50 | Tests phrasing robustness | "Move fifty bucks to savings," "Pay rent from checking," "Put $200 in investments" |
| **Adversarial** | 50 | Tests failure handling | "Transfer negative $50," "Move money to nonexistent account," "Undo everything," "What if I had $1000?" |

**Reporting:** Accuracy reported **per category**, not just aggregate. If templated is 95% but adversarial is 40%, that's published honestly.

**Reproducibility:** Full corpus, generation scripts, model parameters, and evaluation script published in `benches/`.

---

## Model Distribution & System Requirements

**System requirements:**
- 8 GB RAM recommended, 6 GB minimum
- ~2 GB disk for primary model (Phi-3-mini 4-bit GGUF)
- No GPU required (CPU inference)

**Model strategy:**
1. README documents exact model name and Hugging Face URL.
2. A `setup.sh` / `setup.ps1` script auto-downloads the GGUF file to `models/` on first run.
3. `cargo run` checks for model file at startup; if missing, prints clear instructions instead of crashing.
4. `.gitignore` excludes `models/` (2-4 GB files don't belong in Git).
5. **Fallback model** for low-memory machines: document a smaller alternative (e.g., TinyLlama 1.1B Q4, ~700 MB) with expected accuracy trade-offs.

**Zero-friction fallback:** `cargo run -- --mock` launches with a deterministic rule-based agent (no model needed). Reviewers can evaluate the ledger, CLI, and error handling without any download.

**Manual quickstart** (if auto-download fails):
```
1. cargo build --release
2. Download [model-name].gguf from [URL] into models/
3. cargo run --release
```

---

## LLM Backend Decision Protocol

This decision must be made **before coding begins**. Spike both options for 2 hours each.

| Criteria | `mistralrs` | `llama.cpp` (via `llama-cpp-rs`) |
|----------|-------------|----------------------------------|
| Build complexity | `cargo add mistralrs` | CMake + bindgen + system libs |
| Model support | Check if Phi-3-mini GGUF works | Any GGUF — confirmed |
| Interview signal | "Pure Rust stack" | "Real FFI experience" |

**Decision rule:**
- Spike `mistralrs` first. If it runs Phi-3-mini (or equivalent) in 2 hours → commit.
- If showstopper bugs → switch to `llama.cpp`, accept FFI complexity.
- **Document the decision and reasoning in README.**

---

## Operational Resilience

The agent module handles not just logical errors but operational failures:

| Failure | Handling | User sees |
|---------|----------|-----------|
| JSON parse failure | Silent retry up to 2x | (nothing if retry succeeds); "Model produced invalid output. Please rephrase." after 2 failures |
| Inference timeout (>30s) | `tokio::time::timeout` or thread timeout | "Model took too long. Please try again." |
| Rust panic in model crate | `catch_unwind` at agent boundary | "Model failed unexpectedly. Please try again." |
| Model segfault (FFI) | **Fatal — process exits** | Process exits with error message. Documented limitation: *"In-process FFI segfaults cannot be caught. A production system would isolate the model in a subprocess."* |
| Ledger invariant violation | `LedgerError` returned to CLI | Structured error: "Insufficient funds: Savings has $50, requested $200." |

---

## Timeline (10 Weeks, Part-Time)

### Pre-Work (Before Week 1)
- [ ] **2-hour spike:** Test `mistralrs` and `llama.cpp` with smallest available model
- [ ] **Commit to LLM backend** and document decision

### Week 1–2: Ledger Core
- [ ] `Account`, `Entry`, `Transaction`, `Ledger` structs
- [ ] `LedgerError` enum: `InsufficientFunds`, `Unbalanced`, `AccountNotFound`, `Overflow`, `InvalidAmount`
- [ ] `Transaction::new(entries) → Result<Transaction, LedgerError>` with `checked_add`/`checked_sub`
- [ ] Event log (`Vec<Transaction>`) + balance cache (`HashMap<AccountId, i64>`)
- [ ] File persistence: serialize/deserialize event log with `serde_json` (atomic write via temp file → rename)
- [ ] 100% unit test coverage of all invariant checks and error paths
- [ ] Property-based tests with `proptest`: random valid entry sets → apply → replay → assert identical balances
- [ ] Replay benchmark: target < 100ms for 100k events

### Week 3: LLM Integration (Raw Inference)
- [ ] Integrate chosen backend; get raw text output from model
- [ ] Implement 30-second inference timeout
- [ ] Implement crash recovery (graceful error on model failure)
- [ ] Validate: can call model and get *any* text response reliably

### Week 4: Prompt Engineering & Structured Output
- [ ] Define JSON function schema for `create_transaction`
- [ ] Write prompt with 5 few-shot examples of valid/invalid transactions
- [ ] Implement JSON parsing with `serde_json` + silent retry (up to 2x)
- [ ] Implement `Agent::generate_transaction(query: &str) → Result<Transaction, AgentError>`
- [ ] Validate: ≥90% parse rate on 50-query mini-corpus

### Week 5: CLI REPL
- [ ] `rustyline` REPL with commands: natural language input, `balance <account>`, `history`, `quit`
- [ ] `--mock` mode: deterministic rule-based agent that produces proposals through the **same validation pipeline** as the real LLM (ledger treats both as untrusted)
- [ ] Display agent proposals → ledger verdict (✅ / ❌ with structured error)
- [ ] Graceful handling of all operational errors (timeout, panic, parse failure)
- [ ] Manual testing of full pipeline end-to-end

### Week 6: Benchmark Corpus & Evaluation
- [ ] Generate 200+ queries: 100 templated, 50 paraphrased, 50 adversarial
- [ ] Run agent against full corpus; record per-category accuracy, P50/P95 latency, rejection rates
- [ ] Publish raw results, generation script, and evaluation script in `benches/`

### Week 7: Fuzzing & Edge Cases
- [ ] `cargo fuzz` target for JSON → `Transaction::new` path
- [ ] Run fuzzer for ≥1 hour; fix all panics
- [ ] Add CI fuzz job (short run per push, long run weekly)
- [ ] Fix any edge cases discovered during corpus evaluation

### Week 8: Documentation & README
- [ ] Record 1-minute GIF of CLI demo (success → failure → error message)
- [ ] Architecture diagram in README
- [ ] Per-category accuracy/latency table
- [ ] "Safety Boundary" section: where compile-time guarantees end, where runtime checks begin
- [ ] Model download instructions / setup script
- [ ] GitHub Actions CI: `cargo check`, `cargo test`, `cargo clippy`, `cargo bench`

### Week 9: LLM Stabilization & Polish
- [ ] Improve parse rate if below target
- [ ] Extend corpus to 300 queries if time permits
- [ ] Record 3-minute demo video with voiceover
- [ ] Write blog post (optional, secondary to README)

### Week 10: Buffer
- [ ] Only if needed. If on track, **ship early.**

---

## Done Criteria (All Must Be True)

1. **`cargo test` passes** with 100% coverage of all invariant checks (balanced entries, sufficient funds, account existence, overflow). Property-based tests verify replay consistency.
2. **Benchmark corpus** of ≥200 queries exists in `benches/` with per-category results **published** (exact accuracy and P50/P95 latency — numbers are reported honestly, not pass/fail). Ledger accept/reject rates are 100%.
3. **`cargo fuzz` runs** for ≥1 hour with zero panics.
4. **First-run experience works two ways:** (a) Full mode: clone → setup script downloads model → `cargo run --release` → interact. (b) Mock mode: clone → `cargo run -- --mock` → interact with rule-based agent. Both under 2 minutes.
5. **README contains:** GIF demo, architecture diagram, per-category accuracy/latency table, safety boundary explanation, system requirements, honest limitations, and documented model crash behaviour.
6. **Persistence is atomic:** event log written via temp file → rename. No corruption on crash.

---

## Major Design Decisions

### Why `Transaction::new` Returns `Result` Instead of Typestate

The typestate pattern (`DraftTransaction` → `ValidatedTransaction` → `PostedTransaction`) was considered and deliberately cut. Typestate is justified when you have many consumers who might skip validation steps. This project has two consumers (the LLM agent and the CLI), making typestate overkill.

Instead, `Transaction::new(entries)` enforces all invariants at construction and returns `Result<Transaction, LedgerError>`. You simply cannot hold an invalid `Transaction`. This is simpler, equally safe, and trivially explainable:

> *"It is impossible to construct a `Transaction` that violates the balance invariant. The constructor checks it; if it fails, you get an error, not a broken object."*

If a future version adds more consumers (batch import, API, etc.), typestate can be introduced as a non-breaking extension.

### Why Amounts Are `i64` in Cents, Not Floating Point

All monetary values are stored as `i64` representing the smallest currency unit (cents for USD). This matches industry practice (Stripe, Square, Ramp) and eliminates floating-point rounding errors entirely.

All arithmetic uses `checked_add` / `checked_sub`. If any operation overflows, `Transaction::new` returns `LedgerError::Overflow` instead of silently wrapping. Multi-currency is explicitly out of scope for v1.

### Why a Running Balance Cache Instead of Replay-on-Query

The ledger maintains a `HashMap<AccountId, i64>` that is updated atomically when `Ledger::apply` succeeds. This gives O(1) balance lookups during interactive use, which matters for real-time validation (e.g., checking sufficient funds).

The trade-off is two sources of truth (cache vs. event log). This is mitigated by:
- Property-based tests (`proptest`) that verify cache-replay consistency on every test run.
- A replay benchmark that rebuilds the cache from the full event log (target: <100ms for 100k events).

### Why the Agent Doesn't Self-Correct

Small quantized models (Phi-3-mini at 4-bit) degrade rapidly in multi-turn conversation. Promising "agent apologizes and retries correctly" would be dishonest — the retry would likely hallucinate a different wrong answer.

Instead, the ledger rejects the invalid transaction with a structured error, the CLI displays it, and the **user** rephrases. This mirrors how production AI-assisted interfaces work (e.g., Copilot suggests; the human edits).

### Why Silent Retry on Parse Failure (Up to 2x)

LLM output is non-deterministic. A query that produces malformed JSON on the first attempt may produce valid JSON on a retry with the same input (due to sampling randomness). Silent retry is a cheap way to improve the user-facing success rate without claiming agent intelligence.

The retry count is logged and reported in benchmarks. This is not hidden — it's documented as an engineering trade-off, not a feature.

### Why Atomic File Persistence Instead of a Database

The event log is serialized to a JSON file via `serde_json`. This is ~30 lines of code and keeps the project focused on the safety narrative rather than storage infrastructure.

Writes use a temp-file → rename pattern to prevent corruption if the process is killed mid-write. Rename is atomic on most filesystems (it's a metadata pointer swap, not a data copy).

SQLite was considered but adds a dependency that doesn't support the core thesis. If persistence becomes a bottleneck, it can be swapped in without changing the ledger API.

### Why `--mock` Mode Exists

Not every reviewer will have 8 GB of RAM or want to download a 2 GB model file. The `--mock` flag launches a deterministic rule-based agent (regex-based) that handles a subset of queries.

Critically, mock output enters the **same untrusted-input path** as real LLM output (`Transaction::new` → `Ledger::apply`). The ledger has zero awareness of whether its input came from a real model or the mock. This guarantees the safety demonstration works regardless of model availability.

### Why Segfaults Are Fatal (Not Caught)

If using `llama.cpp` via FFI, a segfault in the C++ code is undefined behaviour and cannot be caught by Rust's `catch_unwind`. The process will exit. This is a known limitation, documented honestly:

> *"In-process FFI segfaults cannot be caught. A production system would isolate the model in a separate subprocess and communicate via IPC."*

Subprocess isolation was considered but rejected as scope creep. Documenting the limitation honestly demonstrates understanding of the FFI safety boundary — another deliberate trade-off.

### Why Metrics Are Reported, Not Prescribed

Agent accuracy targets (e.g., "≥90% parse rate") were removed from the Done Criteria. If the model achieves 72%, that's the number that gets published. The project's value comes from the ledger catching every failure, not from the agent being perfect.

This honesty is a deliberate signal: real systems engineering means measuring and publishing, not cherry-picking numbers that make the project look good.
