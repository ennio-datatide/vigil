//! Key-value store backed by [`redb`].
//!
//! Provides a simple string key-value store for daemon settings and
//! (in later phases) encrypted secrets. Uses a single `kv` table with
//! caller-namespaced keys (e.g. `"settings:telegram"`, `"secrets:api_key"`).

#![allow(dead_code)] // Module is wired ahead of its consumers.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableTable, TableDefinition};

use crate::error::{KvError, Result};

/// Table definition: `&str -> &str`.
const KV_TABLE: TableDefinition<&str, &str> = TableDefinition::new("kv");

/// A redb-backed key-value store.
///
/// Thread-safety is handled internally by redb; the `Arc` wrapper allows
/// cheap cloning when sharing across async tasks.
#[derive(Clone, Debug)]
pub(crate) struct KvStore {
    db: Arc<Database>,
}

impl KvStore {
    /// Open (or create) a redb database at the given path.
    ///
    /// # Errors
    ///
    /// Returns [`KvError::Open`] if the database cannot be opened.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path).map_err(KvError::Open)?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Get the value associated with `key`, or `None` if absent.
    ///
    /// # Errors
    ///
    /// Returns [`KvError::Read`] on transaction or table errors.
    pub(crate) fn get(&self, key: &str) -> Result<Option<String>> {
        let txn = self.db.begin_read().map_err(|e| KvError::Read(e.into()))?;
        let table = match txn.open_table(KV_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(None),
            Err(e) => return Err(KvError::Read(e.into()).into()),
        };
        let value = table
            .get(key)
            .map_err(|e| KvError::Read(e.into()))?
            .map(|v| v.value().to_owned());
        Ok(value)
    }

    /// Set `key` to `value`, creating or overwriting as needed.
    ///
    /// # Errors
    ///
    /// Returns [`KvError::Write`] on transaction or table errors.
    pub(crate) fn set(&self, key: &str, value: &str) -> Result<()> {
        let txn = self.db.begin_write().map_err(|e| KvError::Write(e.into()))?;
        {
            let mut table = txn.open_table(KV_TABLE).map_err(|e| KvError::Write(e.into()))?;
            table.insert(key, value).map_err(|e| KvError::Write(e.into()))?;
        }
        txn.commit().map_err(|e| KvError::Write(e.into()))?;
        Ok(())
    }

    /// Delete `key` from the store. No-op if the key does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`KvError::Write`] on transaction or table errors.
    pub(crate) fn delete(&self, key: &str) -> Result<()> {
        let txn = self.db.begin_write().map_err(|e| KvError::Write(e.into()))?;
        {
            let table = txn.open_table(KV_TABLE);
            match table {
                Ok(mut t) => {
                    t.remove(key).map_err(|e| KvError::Write(e.into()))?;
                }
                Err(redb::TableError::TableDoesNotExist(_)) => {
                    return Ok(());
                }
                Err(e) => return Err(KvError::Write(e.into()).into()),
            }
        }
        txn.commit().map_err(|e| KvError::Write(e.into()))?;
        Ok(())
    }

    /// List all keys currently stored.
    ///
    /// # Errors
    ///
    /// Returns [`KvError::Read`] on transaction or table errors.
    pub(crate) fn list_keys(&self) -> Result<Vec<String>> {
        let txn = self.db.begin_read().map_err(|e| KvError::Read(e.into()))?;
        let table = match txn.open_table(KV_TABLE) {
            Ok(t) => t,
            Err(redb::TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(e) => return Err(KvError::Read(e.into()).into()),
        };
        let mut keys = Vec::new();
        let iter = table.iter().map_err(|e| KvError::Read(e.into()))?;
        for entry in iter {
            let (k, _v) = entry.map_err(|e| KvError::Read(e.into()))?;
            keys.push(k.value().to_owned());
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_kv() -> (tempfile::TempDir, KvStore) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let store = KvStore::open(&dir.path().join("test.redb")).expect("open kv store");
        (dir, store)
    }

    #[test]
    fn open_creates_database() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let path = dir.path().join("new.redb");
        assert!(!path.exists());
        let _store = KvStore::open(&path).expect("open kv store");
        assert!(path.exists());
    }

    #[test]
    fn set_and_get_value() {
        let (_dir, store) = temp_kv();
        store.set("settings:theme", "dark").expect("set");
        let val = store.get("settings:theme").expect("get");
        assert_eq!(val.as_deref(), Some("dark"));
    }

    #[test]
    fn get_missing_key_returns_none() {
        let (_dir, store) = temp_kv();
        let val = store.get("nonexistent").expect("get");
        assert_eq!(val, None);
    }

    #[test]
    fn delete_removes_key() {
        let (_dir, store) = temp_kv();
        store.set("k", "v").expect("set");
        store.delete("k").expect("delete");
        let val = store.get("k").expect("get");
        assert_eq!(val, None);
    }

    #[test]
    fn delete_nonexistent_is_noop() {
        let (_dir, store) = temp_kv();
        store.delete("ghost").expect("delete nonexistent");
    }

    #[test]
    fn list_keys_returns_all_keys() {
        let (_dir, store) = temp_kv();
        store.set("a", "1").expect("set a");
        store.set("b", "2").expect("set b");
        store.set("c", "3").expect("set c");
        let mut keys = store.list_keys().expect("list keys");
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn list_keys_empty_store() {
        let (_dir, store) = temp_kv();
        let keys = store.list_keys().expect("list keys");
        assert!(keys.is_empty());
    }

    #[test]
    fn set_overwrites_existing_value() {
        let (_dir, store) = temp_kv();
        store.set("k", "old").expect("set old");
        store.set("k", "new").expect("set new");
        let val = store.get("k").expect("get");
        assert_eq!(val.as_deref(), Some("new"));
    }
}
