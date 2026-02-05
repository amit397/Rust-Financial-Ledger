use wasm_bindgen::prelude::*;
use serde::{Serialize, Deserialize};

// Use wee_alloc for smaller WASM binary size (optional but good practice)
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Represents a single side of a transaction (Debit or Credit).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Entry {
    pub account_id: String,
    pub amount: i64, // Positive = Credit, Negative = Debit
}

/// A Financial Transaction consisting of multiple entries.
/// STRICT INVARIANT: The sum of all entries.amount MUST be 0.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Transaction {
    pub id: u32,
    pub description: String,
    pub timestamp: u64,
    pub entries: Vec<Entry>,
    pub category: Option<String>,
}

impl Transaction {
    /// key validation logic: Returns an Result.
    /// If sum != 0, it rejects the creation.
    pub fn new(id: u32, description: String, timestamp: u64, entries: Vec<Entry>) -> Result<Transaction, String> {
        let balance: i64 = entries.iter().map(|e| e.amount).sum();
        
        if balance != 0 {
            return Err(format!("Transaction unbalanced: Sum is {}. Must be 0.", balance));
        }

        if entries.is_empty() {
             return Err("Transaction cannot be empty.".to_string());
        }

        Ok(Transaction {
            id,
            description,
            timestamp,
            entries,
            category: None, 
        })
    }
}

#[wasm_bindgen]
pub struct Engine {
    transactions: Vec<Transaction>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        
        // Hook up panic handler for better debugging in browser console
        #[cfg(feature = "console_error_panic_hook")]
        console_error_panic_hook::set_once();
        
        Engine { transactions: Vec::new() }
    }

    /// Adds a transaction to the ledger.
    /// Accepts a JsValue which should ideally be a JSON object of the entries.
    /// Returns "Success" or an error string.
    pub fn add_transaction_val(&mut self, id: u32, description: String, timestamp: u64, js_entries: JsValue) -> String {
        let entries: Vec<Entry> = match serde_wasm_bindgen::from_value(js_entries) {
            Ok(e) => e,
            Err(_) => return "Error: Invalid entries format".to_string(),
        };

        match Transaction::new(id, description, timestamp, entries) {
            Ok(tx) => {
                self.transactions.push(tx);
                "Success: Transaction Committed".to_string()
            },
            Err(e) => format!("Error: {}", e),
        }
    }

    pub fn get_transaction_count(&self) -> usize {
        self.transactions.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balanced_transaction() {
        let entries = vec![
            Entry { account_id: "Cash".to_string(), amount: 100 },
            Entry { account_id: "Revenue".to_string(), amount: -100 },
        ];
        let tx = Transaction::new(1, "Sale".to_string(), 1000, entries);
        assert!(tx.is_ok());
    }

    #[test]
    fn test_unbalanced_transaction() {
        let entries = vec![
            Entry { account_id: "Cash".to_string(), amount: 100 },
            Entry { account_id: "Revenue".to_string(), amount: -50 },
        ];
        let tx = Transaction::new(2, "Bad Math".to_string(), 1001, entries);
        assert!(tx.is_err());
    }
}
