# Session Context — LedgerGuard

## Last Completed
Phase 1: Project Scaffold + Ledger Core (2026-03-15)

## Current State
- All Phase 1 files created and compiling
- `Transaction::new` and `Ledger::apply` are `todo!()` stubs for user to implement
- 48 unit tests written; 10 pass (non-stub tests), 29 fail on stubs (expected)
- 6 doc tests pass

## Open Follow-ups
- User implements `Transaction::new` todo!() stub
- User implements `Ledger::apply` todo!() stub
- Phase 2: `pub mod agent;` — chaos agents
- Phase 3: `pub mod persistence;` + `pub mod cli;`
- Phase 4: `pub mod stress;`
- Phase 5: LLM integration

## Decisions Made
- i64 cents for all monetary amounts (domain-fintech skill)
- Manual Display impl on LedgerError (no thiserror — not in allowed deps)
- Result over panic for expected failures (m06-error-handling skill)
- Two-phase apply design for concurrent safety
- next_id starts at 1 (id 0 = "never committed")
- "External" account exempt from non-negative balance constraint

## Installed Skills
- rust-router
- domain-fintech
- m06-error-handling
- coding-guidelines
