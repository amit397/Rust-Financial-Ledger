use proptest::prelude::*;
use ledger_guard::ledger::{Ledger, Transaction, Entry, AccountId};

const ACCOUNTS: &[&str] = &["Checking", "Savings", "External", "Revenue", "Expenses"];

fn make_test_ledger() -> Ledger {
    let mut ledger = Ledger::new();
    for name in ACCOUNTS {
        ledger.create_account(name).unwrap();
    }
    // Fund all non-External accounts heavily so random transfers mostly succeed
    for name in ACCOUNTS {
        if *name != "External" {
            let tx = Transaction::new(
                format!("Fund {}", name),
                vec![
                    Entry { account: AccountId("External".into()), amount: -1_000_000 },
                    Entry { account: AccountId(name.to_string()), amount: 1_000_000 },
                ],
            ).unwrap();
            ledger.apply(tx).unwrap();
        }
    }
    ledger
}

fn valid_transaction_strategy() -> impl Strategy<Value = Vec<(usize, usize, i64)>> {
    // Generate 1..50 transactions, each with from_idx, to_idx (into ACCOUNTS), amount 1..=10000
    prop::collection::vec(
        (0..ACCOUNTS.len(), 0..ACCOUNTS.len(), 1i64..=10_000),
        1..50
    )
}

proptest! {
    #[test]
    fn replay_consistency(txs in valid_transaction_strategy()) {
        let mut ledger = make_test_ledger();

        for (from_idx, to_idx, amount) in &txs {
            if from_idx == to_idx { continue; }
            let from = ACCOUNTS[*from_idx].to_string();
            let to = ACCOUNTS[*to_idx].to_string();
            let entries = vec![
                Entry { account: AccountId(from), amount: -*amount },
                Entry { account: AccountId(to), amount: *amount },
            ];
            if let Ok(tx) = Transaction::new(format!("Transfer {}", amount), entries) {
                let _ = ledger.apply(tx); // may fail (insufficient funds) — that's fine
            }
        }

        // Replay
        let mut replay = Ledger::new();
        for name in ACCOUNTS {
            replay.create_account(name).unwrap();
        }
        for tx in ledger.history() {
            replay.apply(tx.clone()).expect("Replay of committed tx must not fail");
        }

        prop_assert_eq!(
            ledger.accounts(),
            replay.accounts(),
            "Balance cache diverged from event log after {} transactions",
            ledger.transaction_count()
        );
    }
}
