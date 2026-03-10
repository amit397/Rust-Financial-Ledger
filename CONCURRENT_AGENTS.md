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
   │    1. Wraps propose+apply in catch_unwind (prevents mutex poisoning)     │
   │    2. Locks the mutex (with poison recovery)                             │
   │    3. Calls Transaction::new (construction validation)                   │
   │    4. Calls Ledger::apply (stateful validation)                          │
   │    5. Unlocks the mutex                                                  │
   │    6. Records result to MetricsCollector (lock-free atomics)             │
   │                                                                          │
   │  Rust guarantees: if this compiles, no data races exist.                 │
   └──────────────────────────────────────────────────────────────────────────┘
            │                          │                          │
            ▼                          ▼                          ▼
   ┌──────────────────────────────────────────────────────────────────────────┐
   │                       Metrics Collector                                  │
   │                    (lock-free AtomicU64 counters)                         │
   │                                                                          │
   │  Global counters (AtomicU64 — no lock needed):                           │
   │  • total_proposals                                                       │
   │  • total_committed                                                       │
   │  • total_rejected                                                        │
   │  • rejected_insufficient / rejected_not_found / rejected_unbalanced      │
   │  • rejected_overflow / rejected_parse_failure                            │
   │                                                                          │
   │  Per-agent counters (Vec<AgentAtomicMetrics> — indexed by agent_id):     │
   │  • proposed / committed / rejected per agent (AtomicU64 each)            │
   │                                                                          │
   │  Latency (reservoir sampling, cap 10,000 samples):                       │
   │  • Randomly sampled via reservoir algorithm — unbiased percentiles       │
   │  • Stored in Mutex<Vec<Duration>> (contention is negligible due to cap)  │
   │                                                                          │
   │  Lock contention (per-thread AtomicU64 nanosecond accumulators):         │
   │  • Each thread measures time spent in .lock() via Instant::now() delta   │
   └──────────────────────────────────────────────────────────────────────────┘
            │
            ▼
   ┌──────────────────────────────────────────────────────────────────────────┐
   │                        Final Report                                      │
   │                                                                          │
   │  ═══ Stress Test Results (5 agents, 30 seconds) ═══                      │
   │                                                                          │
   │    Total proposals:      147,832                                         │
   │    Committed:             61,203  (41.4%)                                │
   │    Rejected:              86,629  (58.6%)                                │
   │      - Insufficient funds:   22,180                                      │
   │      - Account not found:    25,846                                      │
   │      - Unbalanced entries:   23,903                                      │
   │      - Overflow:              7,416                                      │
   │      - Construction failure:  7,284                                      │
   │                                                                          │
   │    Throughput:           4,928 proposals/sec                              │
   │    Latency (p50):        0.18 ms                                         │
   │    Latency (p95):        0.52 ms                                         │
   │    Lock contention:      2.1% of wall time spent waiting                 │
   │    Data races:           0 (guaranteed by compiler)                       │
   │    Invariant violations: 0                                               │
   │                                                                          │
   │  Per-Agent Breakdown:                                                    │
   │    ValidAgent      →  52,300 proposed,  51,803 committed (99.0%)         │
   │    OverdraftAgent   →  25,100 proposed,      0 committed  (0.0%)         │
   │    TypoAgent        →  24,200 proposed,      0 committed  (0.0%)         │
   │    OverflowAgent    →  22,800 proposed,      0 committed  (0.0%)         │
   │    ChaosAgent       →  23,432 proposed,  9,400 committed (40.1%)         │
   └──────────────────────────────────────────────────────────────────────────┘
```

> **Note on sample numbers:** `sum(proposed) = 147,832`, `sum(committed) = 61,203`,
> `committed ≤ proposed` for every agent. ValidAgent has highest throughput because
> its proposals succeed without triggering error-path overhead. ChaosAgent's 40.1%
> commit rate reflects its ~40% valid-proposal weight.

---

## Part 1 — Chaos Agent Implementations

All chaos agents implement the existing `Agent` trait (`fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>`). **The `Agent` trait is NOT modified** — no `Send` bound is added to the trait itself (see Part 7 for rationale). Each agent uses a random number generator to produce proposals that exercise different failure modes.

Chaos agents **ignore the `input` parameter** — they generate their own proposals internally using their RNG. The thread lifecycle passes `""` as the input string.

### New File: `src/agent/chaos.rs`

This file contains all chaos agent variants. Each agent takes a list of known account names at construction so it can reference them (and deliberately mis-reference them).

#### 1. `ValidAgent`

**Purpose:** Generates structurally and statefully valid proposals. Keeps the system funded so other agents have something to overdraft against.

**Behavior:**
- Randomly picks two accounts from the known list
- Generates a random amount between $0.01 and $50.00 (small to avoid draining accounts)
- Creates a balanced two-entry transfer
- Sometimes generates deposits from External to random accounts (keeps money in the system)
- ~99% of proposals should be committed (unless another thread drains the account between proposal and apply — a legitimate race)

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

### Why `Mutex` Instead of `RwLock`

The plan uses `Arc<Mutex<Ledger>>`, not `Arc<RwLock<Ledger>>`. This is a deliberate choice:

Every agent interaction calls `Ledger::apply()`, which is a **write** operation (`&mut self`). There are **no read-only lock acquisitions** during the stress test — no thread ever needs to just check a balance without potentially modifying it. `RwLock` is designed for workloads with many readers and few writers; here, every access is a write.

Using `RwLock` would add overhead (reader-writer fairness logic, writer starvation prevention) with zero benefit because:
1. `RwLock::write()` provides the same mutual exclusion as `Mutex::lock()`
2. No thread ever calls `RwLock::read()`
3. `Mutex` has simpler implementation = less overhead per lock/unlock cycle

If a future version adds read-only agents (e.g., a `BalanceCheckAgent` that only queries balances without applying), `RwLock` should be reconsidered.

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
    stop_signal: Arc<AtomicBool>,
}

pub struct AgentConfig {
    // +Send bound ONLY here, not on the Agent trait itself.
    // This keeps the trait unchanged for single-threaded consumers (CLI, tests).
    agent: Box<dyn Agent + Send>,
    name: String,
    proposals_per_second: Option<u32>,  // None = as fast as possible
}
```

#### Thread Lifecycle

Each agent runs in its own `std::thread::spawn` (not tokio — we want real OS threads to demonstrate true parallelism, not async I/O which would be cooperative):

```rust
fn agent_thread(
    ledger: Arc<Mutex<Ledger>>,
    agent: Box<dyn Agent + Send>,
    metrics: Arc<MetricsCollector>,
    stop_signal: Arc<AtomicBool>,
    agent_id: usize,
) {
    while !stop_signal.load(Ordering::Relaxed) {
        // Wrap the entire proposal+apply cycle in catch_unwind.
        // This prevents a panicking agent from poisoning the mutex,
        // which would cascade-kill all other threads.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // Chaos agents generate their own proposals internally.
            // The input string is ignored — pass empty string.
            let start = Instant::now();

            // 1. Agent produces a proposal
            let proposal_result = agent.propose("");

            // 2. If proposal succeeded, try to commit it
            match proposal_result {
                Ok(proposal) => {
                    // Build transaction (construction validation)
                    match Transaction::new(proposal.description, proposal.entries) {
                        Ok(tx) => {
                            // Measure lock acquisition time for contention metrics
                            let lock_start = Instant::now();
                            let mut ledger = ledger.lock()
                                .unwrap_or_else(|poisoned| {
                                    // Mutex was poisoned by a panicking thread.
                                    // The inner data is still valid because Ledger::apply
                                    // has all-or-nothing semantics — if apply() panicked,
                                    // no partial mutation occurred.
                                    poisoned.into_inner()
                                });
                            let lock_wait = lock_start.elapsed();
                            metrics.record_lock_wait(agent_id, lock_wait);

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
        }));

        if result.is_err() {
            // Agent panicked. Record it and continue — don't let one
            // broken agent take down the entire stress test.
            metrics.record_panic(agent_id);
        }
    }
}
```

**Key design decisions documented in comments:**

1. **`Arc<Mutex<Ledger>>`** — `Arc` (Atomic Reference Counting) is the thread-safe version of `Rc`. It's needed because multiple threads own the same `Ledger`. The `Mutex` ensures only one thread modifies it at a time. Rust's type system won't let you use `Rc` across threads — it's a compile error.

2. **`Box<dyn Agent + Send>`** — The `+ Send` bound is at the **call site** (in `AgentConfig`), not on the `Agent` trait itself. This means the `Agent` trait in `src/agent/mod.rs` remains unchanged — existing `Box<dyn Agent>` usage in `main.rs` is unaffected. Only stress-test agents need to be `Send`.

3. **`catch_unwind` + `AssertUnwindSafe`** — Wraps the entire proposal+apply cycle. If an agent panics (plausible with OverflowAgent's `i64::MAX` values), the panic is caught here instead of propagating to the thread, which would poison the mutex and cascade-kill all other threads.

4. **`.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`** — If the mutex IS poisoned (despite `catch_unwind`, e.g., from a panic in `Ledger::apply` itself), we recover the inner `Ledger`. This is safe because `Ledger::apply` has all-or-nothing semantics — it validates in Phase 1 (no mutation) and commits in Phase 2 (infallible operations only). A panic in Phase 1 leaves the ledger unmodified. A panic in Phase 2 is not possible (no fallible operations).

5. **`AtomicBool` for stop signal** — Instead of sharing a `bool` behind a mutex (which would cause contention every loop iteration), we use an atomic boolean. Atomics use CPU-level instructions (like `LOCK CMPXCHG` on x86) that don't require locks. This is zero-cost signaling.

6. **Lock contention measurement** — Each thread measures the time spent waiting for `.lock()` using `Instant::now()` deltas before and after the lock call. The accumulated wait time is stored in per-thread `AtomicU64` nanosecond counters and reported as a percentage of total wall time.

#### Graceful Shutdown (Ctrl+C Handling)

The stress test registers a `ctrlc` handler that sets the `AtomicBool` stop signal:

```rust
// In StressTest::run()
let stop_signal = Arc::clone(&self.stop_signal);
ctrlc::set_handler(move || {
    stop_signal.store(true, Ordering::Relaxed);
    eprintln!("\n⏹  Ctrl+C received — stopping agents and printing report...");
}).expect("Failed to set Ctrl+C handler");
```

This ensures that:
- Threads check the stop signal and exit their loop gracefully
- All in-flight proposals complete (no partial operations)
- The metrics report is collected and printed even on early termination
- The user always sees results, whether the test ran to completion or was interrupted

### New File: `src/stress/metrics.rs`

The metrics collector uses **lock-free atomics** for all high-frequency counters. The only mutex-protected data is the latency reservoir, which is capped.

```rust
pub struct MetricsCollector {
    // ── Lock-free global counters (AtomicU64 — CPU atomic instructions) ──
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

    // ── Per-agent counters (lock-free, indexed by agent_id) ──
    // Pre-allocated Vec of atomic structs. No mutex needed.
    // Each agent thread only writes to its own index.
    per_agent: Vec<AgentAtomicMetrics>,

    // ── Latency samples (reservoir sampling, capped at RESERVOIR_CAP) ──
    // Uses Algorithm R (Vitter, 1985) for unbiased random sampling.
    // Mutex contention is negligible because:
    //   1. The reservoir is capped (no unbounded growth)
    //   2. Once full, most iterations skip the lock (reservoir probability < 1)
    latencies: Mutex<Vec<Duration>>,
    latency_count: AtomicU64,  // Total latencies seen (for reservoir probability)

    // ── Lock contention (per-thread, lock-free) ──
    // Each thread accumulates nanoseconds spent waiting for .lock()
    lock_wait_ns: Vec<AtomicU64>,  // Indexed by agent_id

    // ── Panic counter ──
    total_panics: AtomicU64,

    // Timing
    start_time: Instant,
}

/// Per-agent metrics using only atomics. No mutex needed.
pub struct AgentAtomicMetrics {
    pub proposed: AtomicU64,
    pub committed: AtomicU64,
    pub rejected: AtomicU64,
}

const RESERVOIR_CAP: usize = 10_000;
```

**Why `Vec<AgentAtomicMetrics>` instead of `Mutex<Vec<AgentMetrics>>`:**
- Each agent thread only writes to its own index (`per_agent[agent_id]`)
- `AtomicU64::fetch_add(1, Ordering::Relaxed)` compiles to a single CPU instruction
- No lock/unlock overhead, no contention between agents
- Pre-allocated at stress test startup (length = number of agents)

**Why reservoir sampling instead of systematic (1/100) sampling:**
- Systematic sampling (every Nth) introduces bias if the workload has periodicity (OS scheduler quanta, GC pauses, lock contention waves)
- Reservoir sampling (Algorithm R) guarantees every observation has equal probability of being in the final sample, regardless of workload patterns
- The reservoir is capped at 10,000 entries — enough for accurate p50/p95/p99 while bounding memory

**`MetricsCollector` is `Sync` (required for `Arc<MetricsCollector>`):**
- `AtomicU64` → `Sync` ✅
- `Mutex<Vec<Duration>>` → `Sync` ✅
- `Vec<AgentAtomicMetrics>` → `Sync` ✅ (because `AtomicU64` is `Sync`)
- `Instant` → `Sync` ✅
- The compiler verifies this automatically — if any field were not `Sync`, `Arc<MetricsCollector>` would fail to compile.

**Why atomics use `Ordering::Relaxed`:**
- `Ordering::Relaxed` is sufficient because we don't need ordering guarantees between different counters — we just need each counter to be correct individually
- `fetch_add(1, Relaxed)` compiles to a single `LOCK XADD` instruction on x86 — no memory barrier overhead

### Latency Reservoir Sampling

```rust
impl MetricsCollector {
    pub fn record_latency(&self, _agent_id: usize, duration: Duration) {
        let count = self.latency_count.fetch_add(1, Ordering::Relaxed);

        if (count as usize) < RESERVOIR_CAP {
            // Reservoir not full — always add
            let mut latencies = self.latencies.lock()
                .unwrap_or_else(|p| p.into_inner());
            latencies.push(duration);
        } else {
            // Reservoir full — replace with probability RESERVOIR_CAP / count
            // This is Algorithm R (Vitter, 1985)
            let j = rand::random::<u64>() % (count + 1);
            if (j as usize) < RESERVOIR_CAP {
                let mut latencies = self.latencies.lock()
                    .unwrap_or_else(|p| p.into_inner());
                latencies[j as usize] = duration;
            }
            // else: skip — no lock acquired, zero overhead
        }
    }

    pub fn report(&self) -> StressTestReport {
        let elapsed = self.start_time.elapsed();
        let total = self.total_proposals.load(Ordering::Relaxed);
        let committed = self.total_committed.load(Ordering::Relaxed);

        // Sort latencies for percentile calculation
        let mut latencies = self.latencies.lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        latencies.sort();

        let p50 = latencies.get(latencies.len() / 2).copied();
        let p95 = latencies.get(latencies.len() * 95 / 100).copied();
        let p99 = latencies.get(latencies.len() * 99 / 100).copied();

        // Lock contention: sum all per-thread wait times
        let total_lock_wait_ns: u64 = self.lock_wait_ns.iter()
            .map(|a| a.load(Ordering::Relaxed))
            .sum();
        let total_agent_wall_ns = elapsed.as_nanos() as u64 * self.per_agent.len() as u64;
        let contention_pct = if total_agent_wall_ns > 0 {
            total_lock_wait_ns as f64 / total_agent_wall_ns as f64 * 100.0
        } else {
            0.0
        };

        StressTestReport {
            duration: elapsed,
            total_proposals: total,
            total_committed: committed,
            total_rejected: self.total_rejected.load(Ordering::Relaxed),
            total_panics: self.total_panics.load(Ordering::Relaxed),
            throughput_per_sec: total as f64 / elapsed.as_secs_f64(),
            p50_latency: p50,
            p95_latency: p95,
            p99_latency: p99,
            contention_pct,
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

### `[MODIFY]` Update `--help` Output

The existing `--help` in `main.rs` (lines 72–88) must be updated to include the new flags:

```
OPTIONS:
  --mock           Use deterministic mock agent (no model download needed)
  --data <path>    Path to ledger data file (default: ledger_data.json)
  --stress         Run concurrent chaos agent stress test (bypasses REPL)
  --agents <N>     Number of stress test agents (default: 5, max: num_cpus * 2)
  --duration <S>   Stress test duration in seconds (default: 30)
  --help, -h       Show this help message
```

### Thread Count vs CPU Core Count

When `--agents` exceeds available CPU cores, the OS scheduler context-switches heavily, inflating latency and deflating throughput. The stress test should:

1. **Default** to `min(5, num_cpus)` agents
2. **Warn** if `--agents > num_cpus`: `"⚠ Running {N} agents on {C} cores — context switching will inflate latency numbers"`
3. **Document** in `--help`: meaningful performance benchmarks require `agents ≤ num_cpus`

This uses `std::thread::available_parallelism()` (stable since Rust 1.59) to detect core count without adding a dependency.

### Stress Test Startup Flow

1. Creates a ledger with default accounts (Checking, Savings, External, plus 5 more: Revenue, Expenses, Investments, Payroll, Escrow — more accounts = more interesting contention patterns)
2. Funds accounts via ValidAgent-style deposits from External
3. Registers `ctrlc` handler → sets `AtomicBool` stop signal
4. Spawns N agent threads
5. Waits for the specified duration OR Ctrl+C (whichever comes first)
6. Sets the stop signal
7. Joins all threads
8. Collects and prints the metrics report

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
  Committed:             61,203  (41.4%)
  Rejected:              86,629  (58.6%)

  Rejection Breakdown:
    Insufficient funds:      22,180  (25.6% of rejections)
    Account not found:       25,846  (29.8%)
    Unbalanced entries:      23,903  (27.6%)
    Overflow:                 7,416   (8.6%)
    Construction failure:     7,284   (8.4%)

  Performance:
    Throughput:             4,928 proposals/sec
    Latency (p50):          0.18 ms
    Latency (p95):          0.52 ms
    Latency (p99):          1.03 ms
    Lock contention:        2.1% of wall time spent waiting

  Agent Health:
    Panics caught:          0 (via catch_unwind)

═══ Per-Agent Breakdown ═══

  Agent                Proposed    Committed   Rejected    Commit Rate
  ─────────────────────────────────────────────────────────────────────
  ValidAgent            52,300      51,803        497        99.0%
  OverdraftAgent        25,100           0     25,100         0.0%
  TypoAgent             24,200           0     24,200         0.0%
  OverflowAgent         22,800           0     22,800         0.0%
  ChaosAgent            23,432       9,400     14,032        40.1%

═══ Invariant Verification ═══

  ✅ All account balances ≥ 0 (except External)
  ✅ Sum of all balances equals zero (guaranteed by Transaction::new's
     balance-at-construction invariant — every committed transaction
     sums to zero, so cumulative balances always sum to zero)
  ✅ Transaction count matches committed count
  ✅ Replay consistency verified (replayed on fresh ledger, identical balances)
  ✅ No data races (guaranteed by Rust's type system — compile-time proof)

═══ What This Proves ═══

  "5 unreliable agents submitted 147,832 proposals over 30 seconds.
   The ledger committed 61,203 valid transactions and rejected
   86,629 invalid ones. Zero invariant violations. Zero data races.
   Rust's type system made the concurrency proof free."
```

---

## Part 4 — Post-Stress Invariant Verification

After the stress test completes, the system runs a final verification pass that checks properties that **must** be true if the ledger is correct:

### Verification Checks

1. **Non-negative balances**: Every account (except External) must have `balance >= 0`. If any account is negative, the ledger's `apply` function has a bug.

2. **Conservation of money (double-entry invariant)**: The sum of all account balances must equal zero. **Why?** `Transaction::new` enforces that every transaction's entries sum to zero at construction time. Since every committed transaction is balanced, the cumulative effect of all transactions on all accounts must also be zero. If the sum is non-zero, either `Transaction::new`'s balance check is broken or the balance cache diverged from the event log — both are critical bugs.

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

    // Check 2: Conservation of money (double-entry invariant)
    // Every committed transaction sums to zero (enforced by Transaction::new).
    // Therefore the sum of all account balances must be zero.
    // If this fails, either Transaction::new's balance check is broken or
    // the balance cache diverged from the event log.
    let total: i64 = ledger.accounts().iter().map(|(_, b)| b).sum();
    if total != 0 {
        results.push(VerificationResult::Fail(
            format!(
                "Balance conservation violated: sum = {} (expected 0). \
                 Every committed transaction sums to zero (Transaction::new invariant), \
                 so cumulative balances must also sum to zero.",
                total
            )
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
    // Create a fresh ledger with the same accounts, replay all transactions,
    // and verify balances are identical.
    let mut replay_ledger = Ledger::new();
    for (name, _) in ledger.accounts() {
        replay_ledger.create_account(name).unwrap();
    }
    for tx in ledger.history() {
        replay_ledger.apply(tx.clone()).unwrap();
    }
    let replay_balances = replay_ledger.accounts();
    let original_balances = ledger.accounts();
    if replay_balances != original_balances {
        results.push(VerificationResult::Fail(
            "Replay produced different balances than original".to_string()
        ));
    }

    results
}
```

---

## Part 5 — New Dependencies

Add to `Cargo.toml`:

```toml
# Random number generation — used by chaos agents for proposal generation
# `[DEPENDENCY_RISK: LOW]` — actively maintained, no CVEs, 0.9.x is current
rand = "0.9"

# Graceful Ctrl+C handling — sets AtomicBool stop signal on SIGINT
# `[DEPENDENCY_RISK: LOW]` — stable crate, single-purpose, minimal transitive deps
ctrlc = "3.4"
```

No other new dependencies needed. `std::thread`, `std::sync::{Arc, Mutex, atomic}`, `std::time::{Instant, Duration}`, and `std::thread::available_parallelism` are all in Rust's standard library. This is important — the concurrency story uses **two external crates** (`rand` for randomness, `ctrlc` for signal handling), reinforcing the "Rust gives you this nearly for free" narrative.

---

## Part 6 — New File Structure

```
src/
├── main.rs              [MODIFY] — add --stress/--agents/--duration flag routing + --help update
├── lib.rs               [MODIFY] — add `pub mod stress;`
├── agent/
│   ├── mod.rs           (unchanged — Agent trait is NOT modified)
│   ├── mock.rs           (unchanged)
│   └── chaos.rs         [NEW]    — all chaos agent implementations
├── stress/
│   ├── mod.rs           [NEW]    — StressTest orchestrator + ctrlc handler
│   ├── metrics.rs       [NEW]    — MetricsCollector with atomics + reservoir sampling
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

**Key point:** The ledger module is **unchanged**. The `Agent` trait in `agent/mod.rs` is **unchanged**. The stress test proves that the existing `Transaction::new` → `Ledger::apply` pipeline is correct under concurrent load without any modifications. This is the strongest possible statement: the safety code written for single-threaded use is provably correct under multi-threaded stress.

---

## Part 7 — Agent Trait: Why `Send` Is NOT Added to the Trait

> `[BREAKING_CHANGE: BC-01 — AVOIDED]` — Adding `Send` as a supertrait to `Agent` would be a breaking change that forces all implementors to be `Send`, including single-threaded consumers.

**Decision: Do NOT modify the `Agent` trait.**

The current trait in `src/agent/mod.rs`:
```rust
pub trait Agent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;
    fn name(&self) -> &str;
}
```

This stays exactly as-is. Instead, the `Send` bound is applied **at the call site** in the stress test module:

```rust
// In src/stress/mod.rs
pub struct AgentConfig {
    agent: Box<dyn Agent + Send>,  // Send bound only where it's needed
    name: String,
    proposals_per_second: Option<u32>,
}
```

**Why this is better than `pub trait Agent: Send`:**

1. **No breaking change** — existing `Box<dyn Agent>` in `main.rs` (line 129) remains valid
2. **Principle of least constraint** — the CLI is single-threaded and doesn't need `Send`
3. **More idiomatic Rust** — standard library traits like `Iterator` don't require `Send`; consumers add it when needed
4. **Future-proof** — if someone implements `Agent` with a `Rc` field (valid for single-threaded use), it still works via the CLI path; only the stress test path requires `Send`

`MockAgent` already satisfies `Send` (it contains only `Regex` fields, which are `Send`). All chaos agents will too (they contain only `Vec<String>` and `rand` RNG types, which are `Send`).

---

## Part 8 — Test Plan & Verification

### Automated Tests

These tests verify the concurrent system works correctly.

#### 1. Unit Test: `MetricsCollector` (in `src/stress/metrics.rs`)

```
cargo test stress::metrics::tests
```

- Verify atomic counters increment correctly from multiple threads
- Verify reservoir sampling caps at `RESERVOIR_CAP` entries
- Verify latency percentile calculation is correct
- Verify per-agent stats are isolated (thread A's writes don't affect thread B's reads)

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

#### 3. Unit Test: Mutex Poisoning Recovery (in `src/stress/mod.rs`)

```
cargo test stress::tests::test_poison_recovery
```

- Spawn a thread that panics while holding the ledger mutex
- Verify the next thread recovers via `unwrap_or_else(|p| p.into_inner())`
- Verify the ledger data is still consistent after recovery

#### 4. Integration Test: Short Stress Test (new file `tests/stress_test.rs`)

```
cargo test --test stress_test
```

- Runs a 3-second stress test with 3 agents
- Verifies all post-stress invariants pass
- Verifies `total_committed + total_rejected >= total_proposals` (accounting for in-flight at shutdown — the gap is bounded by the number of agents, since at most one proposal per agent can be in-flight at shutdown)
- Verifies no panics occurred

#### 5. Full Stress Test (manual)

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

This is critical — the 48 existing unit tests and 7 doc tests must continue to pass. Zero changes to the ledger module and zero changes to the Agent trait means zero risk to existing correctness.

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

#### Scenario D: "Ctrl+C Recovery"
Start a 120-second stress test, hit Ctrl+C after 10 seconds. Verify the report still prints with all metrics from the 10-second window.

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

3. **"What's the performance profile?"** → "4,900 proposals/sec with 10 agents, p95 latency under 1ms. Lock contention costs about 2% compared to single-threaded baseline. The bottleneck is the mutex — a production system would consider sharding by account ID."

4. **"What would you change for production?"** → "Sharded ledger (one mutex per account partition), async I/O for the agent layer, per-thread latency histograms instead of a shared reservoir, and subprocess isolation for LLM agents to contain segfaults."

5. **"Why Mutex instead of RwLock?"** → "Every agent operation is a write (`Ledger::apply`). RwLock's reader-writer fairness adds overhead with zero benefit when there are no read-only lock acquisitions."

---

## Implementation Order

This is the ordered implementation plan for agent-driven development. Each step has a precondition, input files, output files, and verification command.

| Step | Action | Precondition | Input | Output | Verify |
|---|---|---|---|---|---|
| 1 | Add `rand = "0.9"` and `ctrlc = "3.4"` to `[dependencies]` | None | `Cargo.toml` | `Cargo.toml` | `cargo check` |
| 2 | Add `pub mod stress;` to `lib.rs` | Step 1 | `src/lib.rs` | `src/lib.rs` | Deferred (module doesn't exist yet) |
| 3 | Add `pub mod chaos;` to `agent/mod.rs` | Step 1 | `src/agent/mod.rs` | `src/agent/mod.rs` | Deferred (module doesn't exist yet) |
| 4 | Implement chaos agents with unit tests | Steps 1,3 | `src/agent/mod.rs` | `src/agent/chaos.rs` | `cargo test agent::chaos` |
| 5 | Implement `MetricsCollector` with reservoir sampling + unit tests | Step 1 | — | `src/stress/metrics.rs` | `cargo test stress::metrics` |
| 6 | Implement `StressTest` orchestrator + `ctrlc` handler + poison recovery | Steps 2,4,5 | `src/stress/metrics.rs` | `src/stress/mod.rs` | `cargo check` |
| 7 | Implement report printer + invariant verification | Steps 5,6 | `src/stress/metrics.rs`, `src/ledger/ledger.rs` | `src/stress/report.rs` | `cargo check` |
| 8 | Wire `--stress`/`--agents`/`--duration` into `main.rs` + update `--help` | Steps 6,7 | `src/main.rs` | `src/main.rs` | `cargo run -- --help` |
| 9 | Write integration test | Steps 6,7,8 | — | `tests/stress_test.rs` | `cargo test --test stress_test` |
| 10 | Run full stress test | Step 9 | — | — | `cargo run --release -- --stress` |
| 11 | Verify all prior tests pass | Step 10 | — | — | `cargo test` (48 unit + 7 doc) |

**Agent-critical notes:**
- **Do NOT modify `src/agent/mod.rs` line 120** (`pub trait Agent`). The trait stays unchanged.
- **Do NOT modify any file in `src/ledger/`**. The ledger is untouched.
- Steps 4 and 5 can be executed **in parallel** (no dependencies between them).
- Step 11 is the final gate — if any of the 48 existing tests fail, the implementation has a regression.

---

## Appendix A — `[BREAKING_CHANGE]` Registry

| ID | Change | Status | Impact |
|---|---|---|---|
| `BC-01` | ~~Add `Send` to `Agent` trait~~ | **AVOIDED** | Would break `Box<dyn Agent>` in `main.rs`. Bound at call site instead. |
| `BC-02` | Add `--stress`, `--agents`, `--duration` CLI flags | **ADDITIVE** | No existing flags affected. |
| `BC-03` | Add `pub mod stress` to `lib.rs` | **ADDITIVE** | No existing public API changes. |
| `BC-04` | Add `pub mod chaos` to `agent/mod.rs` | **ADDITIVE** | No existing public API changes. |

## Appendix B — `[DEPENDENCY_RISK]` Assessment

| Dependency | Version | Risk | Rationale |
|---|---|---|---|
| `rand` | `0.9` | LOW | Actively maintained, no CVEs, standard RNG crate. Supersedes `0.8`. |
| `ctrlc` | `3.4` | LOW | Stable, single-purpose (SIGINT handling), minimal transitive deps. |

All concurrency primitives (`Arc`, `Mutex`, `AtomicU64`, `thread`, `available_parallelism`) are `std`. No external crate needed for the core concurrency story.
