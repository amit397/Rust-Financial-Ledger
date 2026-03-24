use crate::ledger::Transaction;
use std::fs::{File, rename};
use std::io::{Write, Read};

/// Save the transaction history to disk atomically.
///
/// WHY ATOMIC WRITE (temp file → rename)?
///   A direct write to the target path is non-atomic: the OS flushes in chunks.
///   If the process is killed mid-write (Ctrl+C, OOM, power loss), the file is
///   partially written and corrupt — unreadable on next startup.
///
///   The fix: write to a temp file in the same directory, then rename.
///   On Linux/macOS (ext4, APFS) and modern Windows (NTFS): rename() is atomic.
///   It's a metadata pointer swap — the directory entry either points to the old
///   file or the new one. There is no in-between state. No corruption possible.
///
///   Why same directory? rename() is only atomic when source and destination are on
///   the same filesystem. Writing to /tmp then renaming to /home/user/data crosses
///   filesystem boundaries on many systems — not atomic. Same directory = safe.
///
/// IMPLEMENTATION:
///   1. Serialize history with serde_json::to_string_pretty
///   2. Derive temp path: format!("{}.tmp", path)
///   3. Write to temp path
///   4. std::fs::rename(temp_path, path)
///   5. Return any errors boxed
pub fn save(path: &str, history: &[Transaction]) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(history)?;
    let temp_path = format!("{}.tmp", path);
    
    let mut temp_file = File::create(&temp_path)?;
    temp_file.write_all(json.as_bytes())?;
    temp_file.sync_all()?;
    
    rename(&temp_path, path)?;
    Ok(())
}

/// Load transaction history from disk.
/// Returns an empty Vec if the file does not exist (first run — not an error).
/// Returns Err if the file exists but cannot be parsed (corrupt data — surface this).
pub fn load(path: &str) -> Result<Vec<Transaction>, Box<dyn std::error::Error>> {
    if !std::path::Path::new(path).exists() {
        return Ok(Vec::new());
    }
    let mut file = File::open(path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    
    let history: Vec<Transaction> = serde_json::from_str(&contents)?;
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{AccountId, Entry};
    use std::time::SystemTime;

    fn test_tx() -> Transaction {
        Transaction {
            id: 1,
            description: "Test".to_string(),
            entries: vec![
                Entry { account: AccountId("A".to_string()), amount: -100 },
                Entry { account: AccountId("B".to_string()), amount: 100 },
            ],
            timestamp: SystemTime::now(),
        }
    }

    fn assert_transactions_equal(a: &[Transaction], b: &[Transaction]) {
        assert_eq!(a.len(), b.len());
        for (ta, tb) in a.iter().zip(b.iter()) {
            assert_eq!(ta.id, tb.id);
            assert_eq!(ta.description, tb.description);
            assert_eq!(ta.entries.len(), tb.entries.len());
            for (ea, eb) in ta.entries.iter().zip(tb.entries.iter()) {
                assert_eq!(ea.account, eb.account);
                assert_eq!(ea.amount, eb.amount);
            }
        }
    }

    #[test]
    fn test_round_trip() {
        let path = std::env::temp_dir().join("ledger_test_round_trip.json");
        let path_str = path.to_str().unwrap();
        let history = vec![test_tx(), test_tx(), test_tx()];
        
        save(path_str, &history).unwrap();
        let loaded = load(path_str).unwrap();
        
        assert_transactions_equal(&history, &loaded);
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn test_missing_file() {
        let path = std::env::temp_dir().join("ledger_test_does_not_exist.json");
        let path_str = path.to_str().unwrap();
        let _ = std::fs::remove_file(&path);
        
        let loaded = load(path_str).unwrap();
        assert!(loaded.is_empty());
    }

    #[test]
    fn test_corrupt_file() {
        let path = std::env::temp_dir().join("ledger_test_corrupt.json");
        let path_str = path.to_str().unwrap();
        let mut f = File::create(&path).unwrap();
        f.write_all(b"not json").unwrap();
        
        let loaded = load(path_str);
        assert!(loaded.is_err());
        std::fs::remove_file(&path).unwrap();
    }
}
