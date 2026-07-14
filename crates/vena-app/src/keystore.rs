//! Secret storage (§11.4 security-normative): BYO API keys live in the OS keychain
//! — NEVER in SQLite, settings, or logs. The app binary wires `KeyringKeyStore`;
//! the lib/tests use `MemoryKeyStore`. Either way the key never touches the db.

use vena_core::Result;

pub trait KeyStore: Send + Sync {
    fn set(&self, key: &str, secret: &str) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// In-memory store for the lib default + tests. Never persisted.
#[derive(Default)]
pub struct MemoryKeyStore {
    map: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl KeyStore for MemoryKeyStore {
    fn set(&self, key: &str, secret: &str) -> Result<()> {
        self.map.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.map.lock().unwrap().get(key).cloned())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.map.lock().unwrap().remove(key);
        Ok(())
    }
}
