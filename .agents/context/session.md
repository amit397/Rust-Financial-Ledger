# Session Context — LedgerGuard

## Last Completed
Phase 2: Agent Layer (mock, llm stubs, chaos agents)

## Current State
- Phase 1 (Ledger Core) is fully implemented, all comments cleaned up, tests pass.
- Phase 2 (Agent Layer) is fully implemented and compiling.
- `MockAgent`, `LlmAgent`, and 6 Chaos agents added.
- `build_prompt` and `call_with_retry` inside `LlmAgent` are `todo!()` stubs.

## Open Follow-ups
- User implements `LlmAgent` todo!() stubs (`build_prompt` and `call_with_retry`)
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
- Agent trait established as uniform boundary for all inference/mock/chaos sources.

## Installed Skills
- rust-router
- domain-fintech
- m06-error-handling
- coding-guidelines
