#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // UNIMPLEMENTED — Developer task
    //
    // WHAT IS COVERAGE-GUIDED FUZZING?
    //   The fuzzer generates random byte sequences and feeds them to this function.
    //   It instruments the binary to track which code paths each input exercises.
    //   Inputs that reach NEW code paths are kept and mutated further.
    //   Inputs that reach only previously-seen paths are discarded.
    //   Result: the fuzzer efficiently explores the entire reachable input space,
    //   finding panics and invariant violations you'd never think to test manually.
    //
    // WHAT YOU'RE FUZZING — the untrusted input boundary:
    //   Raw bytes → UTF-8 string → JSON parse → AgentProposal → Transaction::new
    //   This is exactly the path a malicious or buggy LLM could exploit.
    //   If any input causes a panic, it's a bug — even if it "should" be rejected.
    //   Rejecting invalid input is correct. PANICKING on invalid input is a bug.
    //
    // WHAT TO LOOK FOR:
    //   1. Any panic (unwrap(), index out of bounds, arithmetic overflow)
    //   2. Transaction::new returning Ok when entries don't sum to zero (invariant violation)
    //   3. Any input that takes > ~10ms (potential infinite loop / catastrophic backtracking)
    //   The fuzzer catches panics automatically. You must check #2 manually (see step 5).
    //
    // IMPLEMENTATION STEPS:
    //
    //   1. Convert bytes to UTF-8. If invalid, return early (not a bug — just not valid UTF-8).
    //      let Ok(s) = std::str::from_utf8(data) else { return; };
    //
    //   2. Attempt to parse as the expected JSON format:
    //      { "description": "...", "entries": [{ "account": "...", "amount_cents": N }] }
    //      Use serde_json::from_str::<serde_json::Value>(s).
    //      If parse fails, return early (expected — most random bytes aren't valid JSON).
    //
    //   3. If parse succeeds, build a Vec<Entry> from the parsed value.
    //      Be defensive: if any field is missing or wrong type, return early.
    //
    //   4. Call Transaction::new(description, entries).
    //      If it returns Err, return early (correct rejection behavior — not a bug).
    //
    //   5. If it returns Ok(tx): THIS IS THE CRITICAL CHECK.
    //      Verify the invariant independently:
    //        let sum: i64 = tx.entries.iter().map(|e| e.amount).fold(0i64, |a, b| a.saturating_add(b));
    //        if sum != 0 {
    //            panic!("INVARIANT VIOLATION: Transaction::new returned Ok for unbalanced transaction. sum={}", sum);
    //        }
    //      If this panic fires, Transaction::new has a bug. That's a critical finding.
    //
    // TO RUN:
    //   cargo install cargo-fuzz  (one-time setup)
    //   cargo fuzz run fuzz_transaction
    //   Let it run for ≥1 hour. Any "CRASH" output is a finding — fix it.
    //   Re-run after fixing to confirm clean.
    //   For CI: cargo fuzz run fuzz_transaction -- -max_total_time=60  (60-second run)
    //
    // EXPECTED RESULT after ≥1 hour with correct implementation:
    //   No panics. The fuzzer will report "no crashes" and show coverage statistics.
    //   High coverage of Transaction::new's error paths is a good sign.
    todo!("fuzz_transaction — implement fuzzing harness for the JSON→Transaction pipeline")
});
