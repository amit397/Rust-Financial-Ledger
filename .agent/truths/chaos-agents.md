# Chaos Agent Constraints

## Agent Variants and Expected Behavior

| Agent | Structurally Valid? | Statefully Valid? | Commit Rate |
|-------|--------------------|--------------------|-------------|
| ValidAgent | Yes | Yes | ~99% |
| OverdraftAgent | Yes | No | 0% |
| TypoAgent | Yes | No | 0% |
| OverflowAgent | Yes | No | 0% |
| UnbalancedAgent | No | N/A | 0% |
| ChaosAgent | Mixed | Mixed | ~40% |

## ChaosAgent Weights

~40% Valid, ~15% Overdraft, ~15% Typo, ~10% Overflow, ~10% Unbalanced, ~10% Garbage.

## ValidAgent Must Fund the System

Without periodic deposits from External → other accounts, all accounts drain
and every transfer fails with InsufficientFunds. ValidAgent strategy:
60% transfers ($0.01–$50), 40% deposits from External ($10–$100).

## Agents Ignore the `input` Parameter

Chaos agents generate proposals internally via RNG. The thread lifecycle
passes `""` as input.

## Post-Stress Verification (4 checks, all must pass)

1. Non-negative balances (except External)
2. `sum(all_balances) == 0`
3. `ledger.transaction_count() == metrics.total_committed`
4. Replay on fresh ledger produces identical balances
