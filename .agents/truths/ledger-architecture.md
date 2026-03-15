# LedgerGuard — Architectural Invariants

## Two-Phase Commit in `Ledger::apply`

`Ledger::apply(tx)` MUST validate all entries (read-only) before mutating any state.
Phase 1 failures return an error with zero bytes of state changed. Phase 2 is infallible.
This is what makes mutex poison recovery (`into_inner()`) safe — a panic during
validation leaves the ledger pristine.

## Balance Conservation

`Transaction::new` enforces `sum(entries) == 0` at construction. Therefore:
- It is impossible to hold an invalid `Transaction`
- `sum(all_account_balances) == 0` is an invariant maintained inductively
- Post-stress verification checks this as a proof of correctness

## Integer Cents, Not Floats

All amounts are `i64` in cents. All arithmetic uses `checked_add`/`checked_sub`.
Overflow returns `LedgerError::Overflow`. This is non-negotiable.

## Agent Trait Is Frozen

```rust
pub trait Agent {
    fn propose(&self, input: &str) -> Result<AgentProposal, AgentError>;
    fn name(&self) -> &str;
}
```

Do NOT add `Send` as a supertrait. Apply `+ Send` at the call site (`Box<dyn Agent + Send>`)
in `AgentConfig` only. The trait stays unchanged to preserve backward compatibility with
single-threaded consumers (CLI, tests).

## Ledger Module Is Frozen During Stress Test

The concurrent stress test MUST prove the existing `Transaction::new` → `Ledger::apply`
pipeline is correct under multi-threaded load **without modifying any file in `src/ledger/`**.

## Atomics Use `Ordering::Relaxed`

Metrics counters (`AtomicU64`) use `Relaxed` ordering because they are independent
counters — no happens-before relationship between them is needed.

## Reservoir Sampling (Algorithm R) for Latency

Latency percentiles use Vitter's Algorithm R with a cap of 10,000 samples.
Systematic sampling (every Nth) is rejected due to aliasing bias.
