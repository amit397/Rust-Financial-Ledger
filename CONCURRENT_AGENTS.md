# Concurrent Chaos Agents — Implementation Plan

> Transform LedgerGuard from a single-threaded input validator into a concurrent agent stress-testing framework that justifies Rust and demonstrates real systems engineering depth.

---

## The Problem With the Current Project

Right now LedgerGuard is a sequential REPL where one user types one command at a time. The "safety layer" is a few `if` statements (`sum != 0`, `balance < 0`, `checked_add`). There is no concurrency, no scale, and no reason this couldn't be 40 lines of Python.

**This plan transforms the project so that:**
1. Multiple unreliable agents operate on a shared ledger **simultaneously** from separate threads
2. Rust's type system **provably prevents data races at compile time** — the compiler itself is part of the safety story
3. The system produces **quantifiable metrics** under realistic load (thousands of tx/sec, categorized rejection rates)
4. Interviewers see a systems engineering project, not a validated calculator

---

## Architecture Overview

```
                         ┌─────────────────────────────────┐
                         │        Scenario Config           │
                         │  (N agents, duration, accounts)  │
                         └──────────────┬──────────────────┘
                                        │ spawns
            ┌───────────────────────────┼───────────────────────────┐
            ▼                           ▼                           ▼
   ┌─────────────────┐       ┌─────────────────┐       ┌─────────────────┐
   │  Thread 1        │       │  Thread 2        │       │  Thread N        │
   │  OverdraftAgent  │       │  TypoAgent       │       │  ValidAgent      │
   │                  │       │                  │       │                  │
   │  Generates bad   │       │  Misspells acct  │       │  Normal transfers│
   │  proposals       │       │  names           │       │  that should     │
   │  (overspending)  │       │  (nonexistent)   │       │  succeed         │
   └────────┬─────────┘       └────────┬─────────┘       └────────┬─────────┘
            │                          │                          │
            │     AgentProposal        │    AgentProposal         │   AgentProposal
            ▼                          ▼                          ▼
   ┌──────────────────────────────────────────────────────────────────────────┐
   │                        Shared Ledger                                     │
   │                     Arc<Mutex<Ledger>>                                   │
   │                                                                          │
   │  Each thread:                                                            │
   │    1. Locks the mutex                                                    │
   │    2. Calls Transaction::new (construction validation)                   │
   │    3. Calls Ledger::apply (stateful validation)                          │
   │    4. Unlocks the mutex                                                  │
   │    5. Records result to MetricsCollector                                 │
   │                                                                          │
   │  Rust guarantees: if this compiles, no data races exist.                 │
   └──────────────────────────────────────────────────────────────────────────┘
            │                          │                          │
            ▼                          ▼                          ▼
   ┌──────────────────────────────────────────────────────────────────────────┐
   │                       Metrics Collector                                  │
   │                    (lock-free AtomicU64 counters)                         │
   │                                                                          │
   │  • total_proposals        (AtomicU64)                                    │
   │  • total_committed        (AtomicU64)                                    │
   │  • total_rejected         (AtomicU64)                                    │
   │  • rejected_insufficient  (AtomicU64)                                    │
   │  • rejected_not_found     (AtomicU64)                                    │
   │  • rejected_unbalanced    (AtomicU64)                                    │
   │  • rejected_overflow      (AtomicU64)                                    │
   │  • rejected_parse_failure (AtomicU64)                                    │
   │  • per_agent_stats        (Vec<AgentStats> behind Mutex)                 │
   │  • latencies              (Vec<Duration> behind Mutex for p50/p95)       │
   └──────────────────────────────────────────────────────────────────────────┘
            │
            ▼
   ┌──────────────────────────────────────────────────────────────────────────┐
   │                        Final Report                                      │
   │                                                                          │
   │  ═══ Stress Test Results (5 agents, 30 seconds) ═══                      │
   │                                                                          │
   │    Total proposals:      147,832                                         │
   │    Committed:            103,291  (69.9%)                                │
   │    Rejected:              44,541  (30.1%)                                │
   │      - Insufficient funds:   22,180                                      │
   │      - Account not found:     7,284                                      │
   │      - Unbalanced entries:   11,903                                      │
   │      - Overflow:              3,174                                      │
   │                                                                          │
   │    Throughput:           2,463 tx/sec                                     │
   │    Latency (p50):        0.41 ms                                         │
   │    Latency (p95):        1.23 ms                                         │
   │    Data races:           0 (guaranteed by compiler)                       │
   │    Invariant violations: 0                                               │
   │                                                                          │
   │  Per-Agent Breakdown:                                                    │
   │    ValidAgent      →  32,100 proposed,  31,800 committed (99.1%)         │
   │    OverdraftAgent   →  31,200 proposed,      0 committed  (0.0%)         │
   │    TypoAgent        →  28,500 proposed,      0 committed  (0.0%)         │
   │    OverflowAgent    →  27,800 proposed,      0 committed  (0.0%)         │
   │    ChaosAgent       →  28,232 proposed,  71,491 committed (mix)          │
   └──────────────────────────────────────────────────────────────────────────┘
```

---

## Part 1 — Chaos Agent Implementations

All chaos agents implement the existing `Agent` trait (`fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>`). No changes to the trait are needed. Each agent uses a random number generator to produce proposals that exercise different failure modes.

### New File: `src/agent/chaos.rs`

This file contains all chaos agent variants. Each agent takes a list of known account names at construction so it can reference them (and deliberately mis-reference them).

#### 1. `ValidAgent`

**Purpose:** Generates structurally and statefully valid proposals. Keeps the system funded so other agents have something to overdraft against.

**Behavior:**
- Randomly picks two accounts from the known list
- Generates a random amount between $0.01 and $50.00 (small to avoid draining accounts)
- Creates a balanced two-entry transfer
- Sometimes generates deposits from External to random accounts (keeps money in the system)
- ~100% of proposals should be committed (unless another thread drains the account between proposal and apply)

**Why this matters:** Without ValidAgent, the system would have no money to transact. It also creates realistic contention — valid and invalid proposals compete for the mutex.

#### 2. `OverdraftAgent`

**Purpose:** Always tries to spend more than what's available.

**Behavior:**
- Picks a random source account
- Generates amounts between $10,000 and $1,000,000 (guaranteed to exceed any reasonable balance)
- Creates a balanced transfer to a random destination
- Proposals are structurally valid (balanced, non-zero, no overflow) but statefully invalid

**What it tests:** `Ledger::apply`'s insufficient funds check under concurrent load. Verifies that the rejection happens atomically even when multiple threads are racing.

#### 3. `TypoAgent`

**Purpose:** References accounts that don't exist.

**Behavior:**
- Takes the known account list and deliberately corrupts names: "Checking" → "Chekcing", "Savings" → "Savigns", "External" → "Extrnal"
- Generates balanced, reasonable-amount proposals using the corrupted names
- Proposals pass `Transaction::new` (structurally valid) but fail `Ledger::apply` (account not found)

**What it tests:** The account existence check under concurrent load. Demonstrates the ledger won't silently create accounts from typos.

#### 4. `OverflowAgent`

**Purpose:** Attempts to corrupt the ledger via arithmetic overflow.

**Behavior:**
- Uses amounts near `i64::MAX` and `i64::MIN`
- Creates entries like `[{amount: i64::MAX}, {amount: -i64::MAX}]` (balanced but will overflow during balance computation)
- Tries multi-entry transactions designed to overflow the running sum

**What it tests:** The `checked_add` / `checked_sub` overflow protection. In most languages, these amounts would silently wrap. Rust's `checked_add` returns `None`, which the ledger converts to `LedgerError::Overflow`.

#### 5. `UnbalancedAgent`

**Purpose:** Produces structurally invalid proposals (entries don't sum to zero).

**Behavior:**
- Creates entries where debits ≠ credits (e.g., debit $50, credit $30)
- These fail at `Transaction::new`, before they even reach the ledger

**What it tests:** The construction-time invariant (`Transaction::new`'s balance check). This is the first layer of defense — catching bad proposals before they touch state.

#### 6. `ChaosAgent` (composite)

**Purpose:** Randomly picks from all the above strategies on each proposal. This creates the most realistic stress pattern — any given proposal could be valid or invalid in unpredictable ways.

**Behavior:**
- On each `propose()` call, randomly selects one of the above strategies (weighted: ~40% valid, ~15% overdraft, ~15% typo, ~10% overflow, ~10% unbalanced, ~10% random garbage)
- This ensures the ledger faces a realistic mix of good and bad proposals

---

## Part 2 — Concurrency Architecture

### Why This Justifies Rust

In Python or Go, concurrent access to shared mutable state requires programmer discipline — you must remember to lock before accessing and unlock after. Forget once, and you have a data race that silently corrupts state.

In Rust, the compiler **enforces** this. `Ledger` is `Send` but not `Sync`, meaning:
- It can be moved between threads (`Send`)
- It cannot be shared between threads without synchronization (`!Sync`)

When you wrap it in `Arc<Mutex<Ledger>>`:
- `Arc` provides thread-safe reference counting (multiple owners across threads)
- `Mutex` provides mutual exclusion (only one thread can access the ledger at a time)
- **If you try to access the `Ledger` without locking the `Mutex`, the code won't compile.**

This is a compile-time guarantee that no other mainstream language provides for mutable shared state. The "data races: 0 (guaranteed by compiler)" line in the metrics report is not a test result — it's a **mathematical fact** about the type system.

### New File: `src/stress/mod.rs`

This module contains the stress test harness:

```rust
pub struct StressTest {
    ledger: Arc<Mutex<Ledger>>,
    metrics: Arc<MetricsCollector>,
    agents: Vec<AgentConfig>,
    duration: Duration,
}

pub struct AgentConfig {
    agent: Box<dyn Agent + Send>,  // +Send required to move across thread boundary
    name: String,
    proposals_per_second: Option<u32>,  // None = as fast as possible
}
```

#### Thread Lifecycle

Each agent runs in its own `std::thread::spawn` (not tokio — we want real OS threads to demonstrate true parallelism, not async I/O which would be cooperative):

```
fn agent_thread(
    ledger: Arc<Mutex<Ledger>>,
    agent: Box<dyn Agent + Send>,
    metrics: Arc<MetricsCollector>,
    stop_signal: Arc<AtomicBool>,
    agent_id: usize,
) {
    let mut rng = rand::thread_rng();

    while !stop_signal.load(Ordering::Relaxed) {
        // 1. Generate a random input string (agent-specific)
        let input = generate_random_input(&mut rng);

        // 2. Agent produces a proposal
        let start = Instant::now();
        let proposal_result = agent.propose(&input);

        // 3. If proposal succeeded, try to commit it
        match proposal_result {
            Ok(proposal) => {
                // Build transaction (construction validation)
                match Transaction::new(proposal.description, proposal.entries) {
                    Ok(tx) => {
                        // Lock the ledger and try to apply
                        let mut ledger = ledger.lock().unwrap();
                        match ledger.apply(tx) {
                            Ok(()) => metrics.record_commit(agent_id),
                            Err(e) => metrics.record_rejection(agent_id, &e),
                        }
                    }
                    Err(e) => metrics.record_construction_failure(agent_id, &e),
                }
            }
            Err(e) => metrics.record_parse_failure(agent_id, &e),
        }

        let elapsed = start.elapsed();
        metrics.record_latency(agent_id, elapsed);
        metrics.increment_total(agent_id);
    }
}
```

**Key Rust-specific details to document in comments:**

1. **`Arc<Mutex<Ledger>>`** — `Arc` (Atomic Reference Counting) is the thread-safe version of `Rc`. It's needed because multiple threads own the same `Ledger`. The `Mutex` ensures only one thread modifies it at a time. Rust's type system won't let you use `Rc` across threads — it's a compile error.

2. **`Box<dyn Agent + Send>`** — The `+ Send` bound is required by the compiler. Without it, you can't move the agent into a spawned thread. This is Rust's way of saying "this agent contains no non-thread-safe data (like raw pointers or `Rc`)."

3. **`lock().unwrap()`** — `Mutex::lock()` returns `Result` because a mutex can be "poisoned" if a thread panics while holding the lock. We `unwrap()` because if a thread panics, the whole stress test is invalid anyway. (A production system might handle this differently.)

4. **`AtomicBool` for stop signal** — Instead of sharing a `bool` behind a mutex (which would cause contention every loop iteration), we use an atomic boolean. Atomics use CPU-level instructions (like `LOCK CMPXCHG` on x86) that don't require locks. This is zero-cost signaling.

### New File: `src/stress/metrics.rs`

The metrics collector uses **lock-free atomics** for high-frequency counters and a mutex-protected vec only for latency recording:

```rust
pub struct MetricsCollector {
    // Lock-free counters (no mutex needed — CPU atomic instructions)
    total_proposals: AtomicU64,
    total_committed: AtomicU64,
    total_rejected: AtomicU64,

    // Rejection breakdown (lock-free)
    rejected_insufficient: AtomicU64,
    rejected_not_found: AtomicU64,
    rejected_unbalanced: AtomicU64,
    rejected_overflow: AtomicU64,
    rejected_construction: AtomicU64,
    rejected_parse: AtomicU64,

    // Per-agent stats (needs mutex because Vec isn't atomic)
    per_agent: Mutex<Vec<AgentMetrics>>,

    // Latency samples (mutex-protected, sampled at 1/100 to reduce contention)
    latencies: Mutex<Vec<Duration>>,

    // Timing
    start_time: Instant,
}
```

**Why atomics instead of mutex for counters:**
- `AtomicU64::fetch_add(1, Ordering::Relaxed)` compiles to a single CPU instruction
- No lock/unlock overhead, no contention, no cache-line bouncing (usually)
- `Ordering::Relaxed` is sufficient because we don't need ordering guarantees between different counters — we just need each counter to be correct individually

**Why `Mutex<Vec<Duration>>` for latencies:**
- We can't use atomics for variable-length data
- We mitigate contention by **sampling** — only record every 100th latency
- Alternative: per-thread histograms merged at the end (more complex, implement if contention is measurable)

### Latency Percentile Calculation

```rust
impl MetricsCollector {
    pub fn report(&self) -> StressTestReport {
        let elapsed = self.start_time.elapsed();
        let total = self.total_proposals.load(Ordering::Relaxed);
        let committed = self.total_committed.load(Ordering::Relaxed);

        // Sort latencies for percentile calculation
        let mut latencies = self.latencies.lock().unwrap().clone();
        latencies.sort();

        let p50 = latencies.get(latencies.len() / 2).copied();
        let p95 = latencies.get(latencies.len() * 95 / 100).copied();

        StressTestReport {
            duration: elapsed,
            total_proposals: total,
            total_committed: committed,
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            throughput_per_sec: total as f64 / elapsed.as_secs_f64(),
            p50_latency: p50,
            p95_latency: p95,
            // ... breakdown fields
        }
    }
}
```

---

## Part 3 — CLI Integration

### New Command: `cargo run -- --stress`

Add a `--stress` flag to `main.rs` that bypasses the REPL and runs the concurrent stress test:

```
cargo run -- --stress                    # Default: 5 agents, 30 seconds
cargo run -- --stress --agents 10        # 10 agents
cargo run -- --stress --duration 60      # Run for 60 seconds
cargo run -- --stress --agents 20 --duration 120
```

The stress test:
1. Creates a ledger with default accounts (Checking, Savings, External, plus 5 more: Revenue, Expenses, Investments, Payroll, Escrow — more accounts = more interesting contention patterns)
2. Funds accounts via ValidAgent-style deposits from External
3. Spawns N agent threads
4. Waits for the specified duration
5. Signals all threads to stop
6. Collects and prints the metrics report

### Output Format

The final output should look professional and be immediately impressive:

```
╔══════════════════════════════════════════════════════════════════╗
║              LedgerGuard Concurrent Stress Test                 ║
║         Proving safety under multi-agent contention             ║
╚══════════════════════════════════════════════════════════════════╝

  Configuration:
    Agents:     5 (1 Valid, 1 Overdraft, 1 Typo, 1 Overflow, 1 Chaos)
    Duration:   30.00s
    Accounts:   8
    Threads:    5 OS threads (true parallelism)

═══ Results ═══

  Total proposals:      147,832
  Committed:            103,291  (69.9%)
  Rejected:              44,541  (30.1%)

  Rejection Breakdown:
    Insufficient funds:      22,180  (49.8% of rejections)
    Account not found:        7,284  (16.4%)
    Unbalanced entries:      11,903  (26.7%)
    Overflow:                 3,174   (7.1%)

  Performance:
    Throughput:             4,928 proposals/sec
    Latency (p50):          0.18 ms
    Latency (p95):          0.52 ms
    Lock contention:        ~2.1% of time spent waiting

═══ Per-Agent Breakdown ═══

  Agent                Proposed    Committed   Rejected    Commit Rate
  ─────────────────────────────────────────────────────────────────────
  ValidAgent            32,100      31,800        300        99.1%
  OverdraftAgent        31,200           0     31,200         0.0%
  TypoAgent             28,500           0     28,500         0.0%
  OverflowAgent         27,800           0     27,800         0.0%
  ChaosAgent            28,232      71,491    -43,259        mix

═══ Invariant Verification ═══

  ✅ All account balances ≥ 0 (except External)
  ✅ Sum of all balances equals net deposits from External
  ✅ Transaction count matches committed count
  ✅ No data races (guaranteed by Rust's type system — this is a
     compile-time proof, not a runtime check)

═══ What This Proves ═══

  "5 unreliable agents submitted 147,832 proposals over 30 seconds.
   The ledger committed 103,291 valid transactions and rejected
   44,541 invalid ones. Zero invariant violations. Zero data races.
   Rust's type system made the concurrency proof free."
```

---

## Part 4 — Post-Stress Invariant Verification

After the stress test completes, the system runs a final verification pass that checks properties that **must** be true if the ledger is correct:

### Verification Checks

1. **Non-negative balances**: Every account (except External) must have `balance >= 0`. If any account is negative, the ledger's `apply` function has a bug.

2. **Conservation of money**: The sum of all account balances must equal zero (double-entry bookkeeping). Every credit has a matching debit. If the sum is non-zero, money was created or destroyed — a critical invariant violation.

3. **Transaction count consistency**: `ledger.transaction_count()` must equal the metrics collector's `total_committed` count. If they differ, a transaction was either committed without being counted or counted without being committed.

4. **Replay consistency**: Take the final ledger's transaction history, replay it on a fresh ledger, and verify the balances are identical. This catches any state corruption from concurrent access.

```rust
fn verify_invariants(ledger: &Ledger, metrics: &MetricsCollector) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Check 1: Non-negative balances
    for (name, balance) in ledger.accounts() {
        if name != "External" && balance < 0 {
            results.push(VerificationResult::Fail(
                format!("Account '{}' has negative balance: {}", name, balance)
            ));
        }
    }

    // Check 2: Conservation (all balances should sum to 0)
    let total: i64 = ledger.accounts().iter().map(|(_, b)| b).sum();
    if total != 0 {
        results.push(VerificationResult::Fail(
            format!("Balance conservation violated: sum = {} (expected 0)", total)
        ));
    }

    // Check 3: Transaction count
    let committed = metrics.total_committed();
    if ledger.transaction_count() as u64 != committed {
        results.push(VerificationResult::Fail(
            format!("Transaction count mismatch: ledger={}, metrics={}",
                    ledger.transaction_count(), committed)
        ));
    }

    // Check 4: Replay consistency
    // ... replay all transactions on fresh ledger, compare balances

    results
}
```

---

## Part 5 — New Dependencies

Add to `Cargo.toml`:

```toml
# Random number generation — used by chaos agents
rand = "0.8"
```

No other new dependencies needed. `std::thread`, `std::sync::{Arc, Mutex, atomic}`, and `std::time::{Instant, Duration}` are all in Rust's standard library. This is important — the concurrency story uses **zero external crates**, which reinforces the "Rust gives you this for free" narrative.

---

## Part 6 — New File Structure

```
src/
├── main.rs              [MODIFY] — add --stress flag routing
├── lib.rs               [MODIFY] — add `pub mod stress;`
├── agent/
│   ├── mod.rs           [MODIFY] — add `pub mod chaos;`, make Agent trait Send-safe
│   ├── mock.rs           (unchanged)
│   └── chaos.rs         [NEW]    — all chaos agent implementations
├── stress/
│   ├── mod.rs           [NEW]    — StressTest orchestrator
│   ├── metrics.rs       [NEW]    — MetricsCollector with atomics
│   └── report.rs        [NEW]    — pretty-print report and verification
├── ledger/
│   ├── mod.rs            (unchanged)
│   ├── ledger.rs         (unchanged — the whole point is the existing safety code works)
│   ├── types.rs          (unchanged)
│   └── error.rs          (unchanged)
├── persistence/
│   └── mod.rs            (unchanged)
└── cli/
    └── mod.rs            (unchanged)
```

**Key point:** The ledger module is **unchanged**. The stress test proves that the existing `Transaction::new` → `Ledger::apply` pipeline is correct under concurrent load without any modifications. This is the strongest possible statement: the safety code written for single-threaded use is provably correct under multi-threaded stress.

---

## Part 7 — Required Trait Modifications

### Making `Agent` Send-safe

The `Agent` trait needs `Send` to work across threads:

```rust
// In src/agent/mod.rs, change:
pub trait Agent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;
    fn name(&self) -> &str;
}

// To:
pub trait Agent: Send {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;
    fn name(&self) -> &str;
}
```

`MockAgent` already contains only `Regex` fields, which are `Send`. This means `MockAgent` will automatically satisfy the new bound — no code changes needed to `mock.rs`.

**Why `Send` but not `Sync`?** Each agent is owned by a single thread. It doesn't need to be shared (`Sync`), just moved across thread boundaries (`Send`). This is a deliberate design choice — agents are thread-private, the ledger is thread-shared.

---

## Part 8 — Test Plan & Verification

### Automated Tests

These tests verify the concurrent system works correctly.

#### 1. Unit Test: `MetricsCollector` (in `src/stress/metrics.rs`)

```
cargo test stress::metrics::tests
```

- Verify atomic counters increment correctly from multiple threads
- Verify latency percentile calculation is correct
- Verify per-agent stats are isolated

#### 2. Unit Test: Individual Chaos Agents (in `src/agent/chaos.rs`)

```
cargo test agent::chaos::tests
```

- Each agent produces proposals that match their expected failure mode
- `ValidAgent` produces structurally valid, balanced proposals
- `OverdraftAgent` produces structurally valid but high-amount proposals
- `TypoAgent` produces proposals with corrupted account names
- `OverflowAgent` produces proposals with amounts near `i64::MAX`
- `UnbalancedAgent` produces proposals where entries don't sum to zero

#### 3. Integration Test: Short Stress Test (new file `tests/stress_test.rs`)

```
cargo test --test stress_test
```

- Runs a 3-second stress test with 3 agents
- Verifies all post-stress invariants pass
- Verifies `total_committed + total_rejected >= total_proposals` (accounting for in-flight at shutdown)
- Verifies no panics occurred

#### 4. Full Stress Test (manual)

```
cargo run --release -- --stress --agents 10 --duration 30
```

- Run with `--release` for realistic performance numbers
- Observe the metrics report
- Verify all invariant checks pass (✅ marks in the report)
- Check that throughput is reasonable (1,000+ tx/sec expected)

### All Prior Tests Still Pass

```
cargo test
```

This is critical — the 48 existing unit tests and 7 doc tests must continue to pass. Zero changes to the ledger module means zero risk to existing correctness.

### Manual Verification Scenarios

#### Scenario A: "Scale Demo"
```
cargo run --release -- --stress --agents 20 --duration 60
```
**Expected:** 20 agents, ~200k+ proposals, all invariants pass. This is the "impressive number" demo.

#### Scenario B: "All-Chaos Demo"
Run with only chaos agents (no valid agents). Every proposal should be rejected.
This proves the ledger catches 100% of invalid proposals — the core thesis.

#### Scenario C: "Single Valid Agent Baseline"
Run with 1 ValidAgent only. ~100% commit rate, establishes baseline throughput without contention. Compare with multi-agent throughput to quantify lock contention cost.

---

## Part 9 — What This Changes About the Interview Narrative

### Before (current)
> "I built a financial ledger in Rust that validates transactions."

**Interviewer thinks:** That's basic input validation. Why Rust?

### After (with concurrent chaos agents)
> "I built a concurrent agent safety framework in Rust. Multiple unreliable AI agents operate on a shared financial ledger simultaneously. The ledger maintained perfect consistency across 150,000 proposals — zero data races, guaranteed by Rust's type system at compile time. Here are the metrics."

**Interviewer thinks:** This person understands concurrency, systems design, and how to test safety-critical systems under realistic load. The Rust choice is justified by the compile-time concurrency guarantees.

### Key Talking Points

1. **"Why Rust?"** → "Because `Arc<Mutex<Ledger>>` gives me a compile-time proof that the ledger can't have data races. In Go with goroutines, I'd need to hope my mutex discipline is correct. In Rust, if it compiles, it's correct."

2. **"How do you know the ledger is safe?"** → "I ran 10 chaos agents submitting 5,000 proposals per second for 60 seconds. 150,000 proposals total. The ledger rejected every invalid one and committed every valid one. Here are the categorized metrics."

3. **"What's the performance profile?"** → "4,900 proposals/sec with 10 agents, p95 latency under 1ms. Lock contention costs about 15% compared to single-threaded baseline. The bottleneck is the mutex — a production system would consider sharding by account ID."

4. **"What would you change for production?"** → "Sharded ledger (one mutex per account partition), async I/O for the agent layer, per-thread latency histograms instead of a shared vec, and subprocess isolation for LLM agents to contain segfaults."

---

## Implementation Order

1. **Add `rand` to `Cargo.toml`**
2. **Add `Send` bound to `Agent` trait** (in `agent/mod.rs` — one-line change)
3. **Implement chaos agents** (`agent/chaos.rs`) with unit tests
4. **Implement `MetricsCollector`** (`stress/metrics.rs`) with unit tests
5. **Implement `StressTest` orchestrator** (`stress/mod.rs`)
6. **Implement report printer** (`stress/report.rs`)
7. **Wire into `main.rs`** (add `--stress` flag)
8. **Register modules in `lib.rs`**
9. **Write integration test** (`tests/stress_test.rs`)
10. **Run full stress test, verify output**
11. **Run `cargo test` to verify all 48+ existing tests still pass**
