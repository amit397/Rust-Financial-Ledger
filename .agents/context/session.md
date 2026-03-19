# Session Context — LedgerGuard

## Last Completed
AI Code Debt Review: Fixed potential reversed transfers in `mock.rs` and refactored JSON deserialization in `llm.rs`.

## Current State
- Codebase (Phase 1 & 2) is clean with one-liner doc comments.
- Preserved instructional `todo!()` stubs inside `src/agent/llm.rs` per User's request (Interpretation B).
- 58 tests passing successfully.

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
- Retained User instructional `todo!()` stubs during aggressive codebase comment cleanup.

## Installed Skills
- rust-router
- domain-fintech
- m06-error-handling
- coding-guidelines
