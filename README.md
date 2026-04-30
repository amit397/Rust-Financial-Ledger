# LedgerGuard

**A type-safe Rust ledger that catches every AI agent mistake.**

Build a bulletproof guard that makes a dumb agent safe. The LLM will be wrong ~30% of the time. The ledger catches 100% of those errors. This project demonstrates how to build safe AI agents for high-stakes domains by combining compile-time correctness with runtime verification.

## Architecture

```
  Agent (LLM / Mock / Chaos)
      |
      v  AgentProposal { description, entries }
  Transaction::new()      <-- Layer 1: Structural validation
      |                       - Entries sum to zero (double-entry)
      |                       - No zero amounts
      |                       - Overflow detection (checked_add)
      v  Result<Transaction, LedgerError>
  Ledger::apply()         <-- Layer 2: Contextual validation
      |                       - All accounts must exist
      |                       - Sufficient funds (except External)
      |                       - Atomic balance updates
      v
  Committed (append-only event log + balance cache)
```

**Two layers of defense**, each independently verifiable:
1. **Construction-time invariants** -- `Transaction::new` rejects invalid entries before they exist. You cannot hold an invalid `Transaction`.
2. **Stateful validation** -- `Ledger::apply` checks funds and account existence before committing. Failed transactions leave the ledger unchanged.

## Quick Start

```bash
git clone https://github.com/YOUR_USERNAME/rust-financial-ledger
cd rust-financial-ledger
cargo build --release
```

### Three Modes

**Interactive CLI** (rule-based agent, no model download needed):
```bash
cargo run -- --mock
```
```
LedgerGuard -- AI-Safe Financial Ledger
>> transfer $50 from Checking to Savings
  Transaction committed: transfer $50 from Checking to Savings
>> withdraw $99999 from Checking
  Transaction rejected: Insufficient funds: Checking has $950.00, requested $99999.00
```

**Concurrent Stress Test** (5 chaos agents, 30 seconds):
```bash
cargo run --release -- --stress --agents 5 --duration 30
```

**Full Showcase** (stress test + evaluation + graph generation):
```bash
cargo run --release --bin showcase -- --duration 10
```

**Evaluation Benchmark** (200-query corpus):
```bash
cargo run --bin eval
```

## Stress Test Results

5 unreliable agents submitted **24.7 million proposals** over 16 seconds in release mode. The ledger committed 11.4M valid transactions and rejected 13.3M invalid ones.

```
  Agent                Proposed    Committed    Rejected   Commit%
  ─────────────────────────────────────────────────────────────
  ValidAgent            4471440      4471437           3    100.0%
  OverdraftAgent        3859158      3853512        5646     99.9%
  TypoAgent             4711036        98421     4612615      2.1%
  OverflowAgent         6326581            1     6326580      0.0%
  ChaosAgent            5357333      2961994     2395339     55.3%

  Invariant Verification:
    All non-External balances >= 0
    Sum of all balances = 0 (double-entry conservation)
    Transaction count matches committed count
    Replay consistency verified
```

**Zero invariant violations. Zero data races. 1.5M proposals/sec.**

Rust's type system (`Arc<Mutex<Ledger>>`) makes the concurrency proof free -- the compiler won't compile code with data races.

### Generated Graphs

The showcase binary generates SVG graphs in `output/`:

| Graph | Description |
|-------|-------------|
| `rejection_breakdown.svg` | Bar chart of rejection categories |
| `per_agent_commits.svg` | Per-agent proposed vs committed |
| `eval_accuracy.svg` | Parse rate vs commit rate by corpus category |
| `latency_histogram.svg` | Latency distribution histogram |

## Evaluation Corpus (200 Queries)

The benchmark corpus tests the MockAgent against 200 queries across 3 categories:

| Category | Count | Parse Rate | Commit Rate | Description |
|----------|-------|-----------|-------------|-------------|
| **Templated** | 100 | 100% | 80% | Exact-pattern commands (`transfer $X from A to B`) |
| **Paraphrased** | 50 | 0% | 0% | Natural language the MockAgent can't parse |
| **Adversarial** | 50 | 26% | 8% | Edge cases, injection, overflow, nonsense |

**Ledger correctness: zero false negatives, zero false positives.** The agent's accuracy is a measured number reported honestly. The ledger's job is 100% true-positive and true-negative rates.

## Safety Boundaries

### Compile-Time Guarantees (Rust Type System)
- **No data races**: `Ledger` is `Send` but not `Sync`. `Arc<Mutex<Ledger>>` enforces mutual exclusion. The compiler rejects code that accesses the ledger without holding the lock.
- **No use-after-free / double-free**: Ownership system prevents memory corruption.
- **No null pointer dereference**: `Option<T>` forces explicit handling.

### Runtime Guarantees (Ledger Invariants)
- **Double-entry conservation**: Every `Transaction` sums to zero. Verified at construction and confirmed by the conservation invariant check (sum of all balances = 0).
- **Non-negative balances**: No account (except External) can go below zero.
- **Account existence**: Every entry must reference a known account.
- **Overflow protection**: All arithmetic uses `checked_add`/`checked_sub`. Overflow returns `LedgerError::Overflow`, not silent wraparound.

### Agent Isolation
- Agent panics are caught via `std::panic::catch_unwind` -- one broken agent cannot crash the stress test.
- Mutex poisoning is recovered via `into_inner()` -- a panicked agent doesn't lock out other agents.

### What This Does NOT Guarantee
- **Agent accuracy**: The agent can produce wrong outputs. The ledger catches them.
- **FFI segfaults**: If using a C/C++ model backend, segfaults are fatal (documented limitation).
- **Availability**: A long-running model inference can block the CLI. Timeout (30s) mitigates this.

## Project Structure

```
src/
  ledger/           Core ledger (DO NOT MODIFY -- proven correct)
    types.rs        AccountId, Entry, Transaction
    error.rs        LedgerError enum (6 variants)
    ledger.rs       Transaction::new + Ledger::apply
  agent/
    mod.rs          Agent trait + AgentProposal
    mock.rs         Rule-based MockAgent (regex parser)
    llm.rs          LLM agent stub (disabled, use --mock)
    chaos.rs        6 chaos agents for stress testing
  stress/
    mod.rs          StressTest orchestrator (Arc<Mutex<Ledger>>)
    metrics.rs      Lock-free MetricsCollector (reservoir sampling)
    report.rs       Invariant verification (4 checks)
  eval/
    mod.rs          Evaluation framework (run_eval, compute_accuracy)
  graphs/
    mod.rs          SVG graph generation (pure Rust, no deps)
  cli/
    mod.rs          Interactive REPL (rustyline)
  persistence/
    mod.rs          Atomic file I/O (temp file -> rename)
  bin/
    eval.rs         Evaluation binary
    showcase.rs     Full showcase (stress + eval + graphs)

tests/
  ledger_integration.rs   10 integration tests
  stress_test.rs          2 stress integration tests
  proptest_replay.rs      Property-based replay consistency

benches/corpus/
  templated.json    100 exact-pattern queries
  paraphrased.json  50 natural language queries
  adversarial.json  50 edge case / attack queries
```

## Testing

```bash
# Run all tests (unit + integration + proptest + stress)
cargo test

# Run just the stress test (3-second integration test)
cargo test stress_test_3_seconds

# Run property-based tests
cargo test replay_consistency

# Run the full evaluation
cargo run --bin eval
```

**Test coverage**: 80+ tests covering all invariant checks, error paths, agent behaviors, persistence round-trips, concurrent stress, and property-based replay consistency.

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| `Transaction::new` returns `Result` | Impossible to hold an invalid transaction. Simpler than typestate. |
| Amounts as `i64` cents | Eliminates floating-point rounding. Matches industry practice (Stripe, Square). |
| Balance cache + event log | O(1) lookups during validation. Consistency verified by proptest. |
| `Mutex` over `RwLock` | Every operation is a write (`Ledger::apply`). No read-only paths during stress. |
| `Send` at call site, not on trait | `Box<dyn Agent + Send>` only in `AgentConfig`. CLI remains single-threaded. |
| Reservoir sampling for latency | Unbiased percentiles without unbounded memory (Algorithm R, Vitter 1985). |
| Atomic file persistence | temp file -> rename prevents corruption on crash. No database dependency. |
| `--mock` mode | Reviewers can evaluate the full pipeline without downloading a 2GB model. |

## System Requirements

- **Rust**: 1.70+ (2021 edition)
- **RAM**: ~50 MB (stress test with 5 agents)
- **OS**: Windows, macOS, Linux
- **Dependencies**: serde, serde_json, rustyline, rand, ctrlc (no external model files needed)

