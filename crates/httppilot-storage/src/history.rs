//! Minimal, privacy-preserving local request execution history.
//!
//! No URLs, bodies, query parameters, headers or credentials are stored here.

use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;

use crate::{Storage, StorageError};

const MAX_HISTORY_ROWS: i64 = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: i64,
    pub request_id: u64,
    pub method: String,
    pub status_code: u16,
    pub duration_ms: u64,
    pub occurred_at_ms: i64,
}

impl Storage {
    /// Record only non-sensitive execution metadata. Old entries are pruned
    /// within the same transaction to enforce a hard retention bound.
    pub fn record_history(
        &self,
        request_id: u64,
        method: &str,
        status_code: u16,
        duration_ms: u64,
    ) -> Result<(), StorageError> {
        let occurred_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        let transaction = self.db.unchecked_transaction()?;
        transaction.execute(
            "INSERT INTO request_history
                 (request_id, method, status_code, duration_ms, occurred_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![request_id, method, status_code, duration_ms, occurred_at_ms],
        )?;
        transaction.execute(
            "DELETE FROM request_history
             WHERE id NOT IN (
                 SELECT id FROM request_history ORDER BY id DESC LIMIT ?1
             )",
            params![MAX_HISTORY_ROWS],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Return newest entries first, with a bounded result size.
    pub fn recent_history(&self, limit: usize) -> Result<Vec<HistoryEntry>, StorageError> {
        let mut statement = self.db.prepare(
            "SELECT id, request_id, method, status_code, duration_ms, occurred_at_ms
             FROM request_history ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit.min(250) as i64], |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                request_id: row.get(1)?,
                method: row.get(2)?,
                status_code: row.get(3)?,
                duration_ms: row.get(4)?,
                occurred_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn clear_history(&self) -> Result<(), StorageError> {
        self.db.execute("DELETE FROM request_history", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Storage {
        Storage::open(":memory:").unwrap()
    }

    #[test]
    fn history_records_safe_fields_and_orders_recent_first() {
        let store = store();
        store.record_history(2, "GET", 200, 25).unwrap();
        store.record_history(2, "POST", 201, 40).unwrap();

        let history = store.recent_history(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].method, "POST");
        assert_eq!(history[0].status_code, 201);
        assert_eq!(history[1].duration_ms, 25);
        assert!(history[0].occurred_at_ms > 0);
    }

    #[test]
    fn history_has_hard_retention_limit() {
        let store = store();
        for i in 0..260u64 {
            store.record_history(2, "GET", 200, i).unwrap();
        }
        let history = store.recent_history(300).unwrap();
        assert_eq!(history.len(), 250);
        assert_eq!(history[0].duration_ms, 259);
        assert_eq!(history.last().unwrap().duration_ms, 10);
    }

    #[test]
    fn history_clear_does_not_change_workspace_revision() {
        let store = store();
        let (workspace, revision) = store.load_versioned().unwrap();
        store.record_history(2, "GET", 200, 25).unwrap();
        store.clear_history().unwrap();
        assert!(store.recent_history(10).unwrap().is_empty());
        assert_eq!(store.load_versioned().unwrap().1, revision);
        assert_eq!(store.load().unwrap(), workspace);
    }
}
