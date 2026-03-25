use ledger_guard::ledger::{Ledger, Transaction, Entry, AccountId, LedgerError};
use ledger_guard::agent::Agent;
use ledger_guard::agent::mock::MockAgent;

fn entry(account: &str, amount: i64) -> Entry {
    Entry { account: AccountId(account.to_string()), amount }
}

fn test_ledger() -> Ledger {
    let mut ledger = Ledger::new();
    for name in &["Checking", "Savings", "External", "Revenue", "Expenses", "Investments", "Payroll", "Escrow"] {
        ledger.create_account(name).unwrap();
    }
    // Fund Checking with $1000 and Savings with $500
    let tx = Transaction::new("Fund Checking".into(), vec![
        entry("External", -100_000), entry("Checking", 100_000),
    ]).unwrap();
    ledger.apply(tx).unwrap();

    let tx = Transaction::new("Fund Savings".into(), vec![
        entry("External", -50_000), entry("Savings", 50_000),
    ]).unwrap();
    ledger.apply(tx).unwrap();

    ledger
}

// 1. Valid transfer updates balances
#[test]
fn valid_transfer_updates_balances() {
    let agent = MockAgent { known_accounts: vec!["Checking".into(), "Savings".into(), "External".into()] };
    let mut ledger = test_ledger();

    let proposal = agent.propose("transfer $50.00 from Checking to Savings").unwrap();
    let tx = Transaction::new(proposal.description, proposal.entries).unwrap();
    ledger.apply(tx).unwrap();

    assert_eq!(ledger.balance("Checking"), Some(100_000 - 5000));
    assert_eq!(ledger.balance("Savings"), Some(50_000 + 5000));
}

// 2. Overdraft rejected — balances unchanged
#[test]
fn overdraft_rejected_balances_unchanged() {
    let mut ledger = test_ledger();
    let before_checking = ledger.balance("Checking").unwrap();
    let before_savings = ledger.balance("Savings").unwrap();

    let tx = Transaction::new("Overdraft".into(), vec![
        entry("Checking", -999_999), entry("Savings", 999_999),
    ]).unwrap();

    match ledger.apply(tx) {
        Err(LedgerError::InsufficientFunds { .. }) => {}
        other => panic!("Expected InsufficientFunds, got {other:?}"),
    }

    assert_eq!(ledger.balance("Checking"), Some(before_checking));
    assert_eq!(ledger.balance("Savings"), Some(before_savings));
}

// 3. Nonexistent account rejected
#[test]
fn nonexistent_account_rejected() {
    let mut ledger = test_ledger();
    let tx = Transaction::new("Ghost".into(), vec![
        entry("Checking", -1000), entry("Ghost", 1000),
    ]).unwrap();

    match ledger.apply(tx) {
        Err(LedgerError::AccountNotFound { name }) => assert_eq!(name, "Ghost"),
        other => panic!("Expected AccountNotFound, got {other:?}"),
    }
}

// 4. Zero amount entry rejected
#[test]
fn zero_amount_entry_rejected() {
    let result = Transaction::new("Zero".into(), vec![
        entry("Checking", 0), entry("Savings", 0),
    ]);
    match result {
        Err(LedgerError::InvalidAmount) => {}
        other => panic!("Expected InvalidAmount, got {other:?}"),
    }
}

// 5. Unbalanced entries rejected
#[test]
fn unbalanced_entries_rejected() {
    let result = Transaction::new("Unbalanced".into(), vec![
        entry("Checking", -5000), entry("Savings", 3000),
    ]);
    match result {
        Err(LedgerError::Unbalanced { actual_sum }) => assert_eq!(actual_sum, -2000),
        other => panic!("Expected Unbalanced, got {other:?}"),
    }
}

// 6. Overflow entries rejected
#[test]
fn overflow_entries_rejected() {
    let result = Transaction::new("Overflow".into(), vec![
        entry("A", i64::MAX), entry("B", -(i64::MAX - 1)),
    ]);
    match result {
        Err(LedgerError::Unbalanced { .. }) | Err(LedgerError::Overflow) => {}
        other => panic!("Expected Unbalanced or Overflow, got {other:?}"),
    }
}

// 7. External can go negative
#[test]
fn external_can_go_negative() {
    let mut ledger = test_ledger();
    // External is already negative from funding; debit it further
    let tx = Transaction::new("More funding".into(), vec![
        entry("External", -500_000), entry("Revenue", 500_000),
    ]).unwrap();
    assert!(ledger.apply(tx).is_ok());
    assert!(ledger.balance("External").unwrap() < 0);
}

// 8. Persistence round trip
#[test]
fn persistence_round_trip() {
    let mut ledger = test_ledger();

    let tx = Transaction::new("Extra".into(), vec![
        entry("Checking", -2000), entry("Savings", 2000),
    ]).unwrap();
    ledger.apply(tx).unwrap();

    let tmp_path = std::env::temp_dir().join("ledger_integration_test.json");
    let path_str = tmp_path.to_str().unwrap();

    ledger_guard::persistence::save(path_str, ledger.history()).unwrap();

    let loaded = ledger_guard::persistence::load(path_str).unwrap();
    assert_eq!(loaded.len(), ledger.history().len());

    // Replay on fresh ledger
    let mut replay = Ledger::new();
    for name in &["Checking", "Savings", "External", "Revenue", "Expenses", "Investments", "Payroll", "Escrow"] {
        replay.create_account(name).unwrap();
    }
    for tx in &loaded {
        replay.apply(tx.clone()).unwrap();
    }

    assert_eq!(replay.accounts(), ledger.accounts());

    // Cleanup
    let _ = std::fs::remove_file(&tmp_path);
}

// 9. Replay consistency
#[test]
fn replay_consistency() {
    let mut ledger = test_ledger();

    // Apply 5 additional transactions
    for i in 1..=5 {
        let amount = i * 1000;
        let tx = Transaction::new(format!("Transfer {i}"), vec![
            entry("Checking", -amount), entry("Savings", amount),
        ]).unwrap();
        ledger.apply(tx).unwrap();
    }

    // Replay from history
    let mut replay = Ledger::new();
    for (name, _) in ledger.accounts() {
        replay.create_account(&name).unwrap();
    }
    for tx in ledger.history() {
        replay.apply(tx.clone()).unwrap();
    }

    assert_eq!(replay.accounts(), ledger.accounts());
}

// 10. Transaction ids auto-increment
#[test]
fn transaction_ids_autoincrement() {
    let mut ledger = Ledger::new();
    ledger.create_account("External").unwrap();
    ledger.create_account("A").unwrap();

    for _ in 0..3 {
        let tx = Transaction::new("Fund".into(), vec![
            entry("External", -100), entry("A", 100),
        ]).unwrap();
        ledger.apply(tx).unwrap();
    }

    let history = ledger.history();
    assert_eq!(history[0].id, 1);
    assert_eq!(history[1].id, 2);
    assert_eq!(history[2].id, 3);
}
