# Recommended Community Skills for LedgerGuard

> Install via: `npx skills add <owner/repo>` — interactive picker lets you select individual skills.

---

## Custom Skills Assessment

**Zero custom skills are justified.** Every attempted justification reduces to:

> "A community skill for [X] teaches general [X] patterns. This project uses specific [X] patterns."

That's a `truths/` entry, not a skill override. Project-specific invariants live in:
- `.agent/truths/ledger-architecture.md` — two-phase commit, frozen Agent trait, Relaxed atomics, reservoir sampling
- `.agent/truths/chaos-agents.md` — agent variants, ChaosAgent weights, ValidAgent funding, 4 verification checks

---

## Tier 1 — Direct Project Relevance (Install These)

| Skill | Source | Why It Helps |
|-------|--------|-------------|
| `coding-guidelines` | `actionbook/rust-skills` | 50 core Rust rules — naming, error handling, memory, concurrency safety. Covers all the generic Rust idioms we deleted from custom skills. |
| `domain-fintech` | `actionbook/rust-skills` | FinTech constraints: immutable audit trails, financial precision (`rust_decimal` / integer cents), compliance patterns. Directly relevant to the ledger domain. |
| `m07-concurrency` | `actionbook/rust-skills` | `Arc`, `Mutex`, `Atomic`, `Send`/`Sync`, thread safety patterns. Covers the concurrency patterns needed for the stress test harness. |
| `m06-error-handling` | `actionbook/rust-skills` | `Result<T, E>` vs `Option`, `thiserror`/`anyhow`, `?` operator, when to panic vs return error. Covers typed error enum design for `LedgerError`. |
| `m05-type-driven` | `actionbook/rust-skills` | Making invalid states unrepresentable, newtype patterns. Directly supports `Transaction::new` returning `Result` and the account/entry type design. |

### Install Command

```bash
npx skills add actionbook/rust-skills
# Select: coding-guidelines, domain-fintech, m07-concurrency, m06-error-handling, m05-type-driven
```

---

## Tier 2 — Quality & Safety (Recommended)

| Skill | Source | Why It Helps |
|-------|--------|-------------|
| `m10-performance` | `actionbook/rust-skills` | Performance patterns, benchmarking with `criterion`. Relevant for the stress test throughput and latency measurement. |
| `m15-anti-pattern` | `actionbook/rust-skills` | Common Rust anti-patterns to avoid. Prevents mistakes during implementation. |
| `lint-hunter` | `udapy/rust-agentic-skills` | Diagnoses borrow checker E0xxx errors with step-by-step lifetime traces. Useful when implementing `Arc<Mutex<Ledger>>` and `catch_unwind` patterns. |
| `rust-kernel` | `udapy/rust-agentic-skills` | Generates idiomatic, safe Rust — enforces ownership, borrowing, and lifetime rules. General code quality enforcer. |

### Install Commands

```bash
npx skills add actionbook/rust-skills
# Select: m10-performance, m15-anti-pattern

npx skills add udapy/rust-agentic-skills
# Select: lint-hunter, rust-kernel
```

---

## Tier 3 — Useful but Optional

| Skill | Source | Why It Helps |
|-------|--------|-------------|
| `m01-ownership` | `actionbook/rust-skills` | Deep ownership/borrowing patterns. Helpful for complex `Arc` + closure + `catch_unwind` interactions. |
| `m03-mutability` | `actionbook/rust-skills` | Interior mutability patterns (`RefCell`, atomic types). Relevant for `AtomicU64` metrics design. |
| `m13-domain-error` | `actionbook/rust-skills` | Domain-specific error modeling. Useful for designing `LedgerError` and `AgentError` hierarchies. |
| `rust-router` | `actionbook/rust-skills` | Auto-routes Rust questions to the right specialist skill. Meta-skill for skill discovery. |
| `unsafe-checker` | `actionbook/rust-skills` | Audits unsafe code usage. Not directly needed (no `unsafe` in this project) but good for verification. |

---

## Not Relevant (Skip)

| Skill | Why Skip |
|-------|----------|
| `domain-cli`, `domain-web`, `domain-cloud-native`, `domain-embedded`, `domain-iot`, `domain-ml` | Wrong domain — this is a financial ledger, not a web/cloud/embedded/ML project. |
| `rust-daily`, `rust-learner` | Learning/news — not implementation skills. |
| `rust-deps-visualizer`, `rust-call-graph`, `rust-symbol-analyzer` | Analysis tooling — useful later for documentation, not for initial implementation. |
| `core-actionbook`, `core-agent-browser`, `core-dynamic-skills` | Infrastructure for the skill system itself, not project development. |
