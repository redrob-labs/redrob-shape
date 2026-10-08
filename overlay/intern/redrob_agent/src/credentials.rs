// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the Redrob gateway key lives.
//!
//! The key never passes through Blender's Python. The device flow runs in this process, and the key
//! it yields goes to the operating system keychain (Keychain on macOS, Credential Manager on
//! Windows, the Secret Service on Linux) under the service and account the add-on already used,
//! `ai.redrob.shape` / `redrob`. Blender has no protected store of its own: an add-on preference is
//! written to `userpref.blend`, an ordinary file, which is why nothing here falls back to a file.
//!
//! When no keychain answers (a Linux session with no Secret Service running is the usual case), the
//! key is held in this process for the session only, and `status()` says so.
//!
//! `REDROB_API_KEY` in the environment wins over both, so CI and development keep working. It is
//! reported as the environment, and disconnecting cannot remove it.

use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

pub const KEYCHAIN_SERVICE: &str = "ai.redrob.shape";
pub const KEYCHAIN_ACCOUNT: &str = "redrob";
const MAX_KEY_BYTES: usize = 512;

#[derive(Debug, Error)]
#[error("the system keychain is not available: {0}")]
pub struct StoreError(pub String);

#[derive(Debug, Error)]
#[error("a key is 1 to 512 visible characters, with no spaces")]
pub struct InvalidKey;

/// One secret slot. The real one is the OS keychain; tests use memory.
pub trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, StoreError>;
    fn set(&self, secret: &str) -> Result<(), StoreError>;
    fn delete(&self) -> Result<(), StoreError>;
}

impl<T: SecretStore> SecretStore for Arc<T> {
    fn get(&self) -> Result<Option<String>, StoreError> {
        self.as_ref().get()
    }
    fn set(&self, secret: &str) -> Result<(), StoreError> {
        self.as_ref().set(secret)
    }
    fn delete(&self) -> Result<(), StoreError> {
        self.as_ref().delete()
    }
}

/// The operating system keychain.
pub struct KeychainStore;

impl KeychainStore {
    fn entry() -> Result<keyring::Entry, StoreError> {
        keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
            .map_err(|error| StoreError(error.to_string()))
    }
}

impl SecretStore for KeychainStore {
    fn get(&self) -> Result<Option<String>, StoreError> {
        match Self::entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(StoreError(error.to_string())),
        }
    }

    fn set(&self, secret: &str) -> Result<(), StoreError> {
        Self::entry()?
            .set_password(secret)
            .map_err(|error| StoreError(error.to_string()))
    }

    fn delete(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(StoreError(error.to_string())),
        }
    }
}

/// A slot in memory. With `unavailable()` it behaves like a machine with no keychain.
#[derive(Default)]
pub struct MemoryStore {
    slot: Mutex<Option<String>>,
    unavailable: bool,
}

impl MemoryStore {
    pub fn unavailable() -> Self {
        Self {
            slot: Mutex::new(None),
            unavailable: true,
        }
    }

    fn slot(&self) -> Result<MutexGuard<'_, Option<String>>, StoreError> {
        if self.unavailable {
            return Err(StoreError("no keychain on this machine".into()));
        }
        Ok(self
            .slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

impl SecretStore for MemoryStore {
    fn get(&self) -> Result<Option<String>, StoreError> {
        Ok(self.slot()?.clone())
    }
    fn set(&self, secret: &str) -> Result<(), StoreError> {
        *self.slot()? = Some(secret.to_owned());
        Ok(())
    }
    fn delete(&self) -> Result<(), StoreError> {
        *self.slot()? = None;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Environment,
    Keychain,
    Session,
}

/// What the add-on may show. It carries a hint of the key, never the key.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub connected: bool,
    pub source: Option<Source>,
    /// The last four characters: enough to find the key in the console's list, nothing more.
    pub key_hint: Option<String>,
    /// Whether the system keychain answered the last time it was asked.
    pub keychain_available: bool,
}

pub struct Credentials {
    environment: Option<String>,
    keychain: Box<dyn SecretStore>,
    state: Mutex<State>,
}

struct State {
    /// The key once read or stored, so a keychain that prompts does not prompt for every plan.
    /// It is also the only copy when the keychain could not take the key.
    cached: Option<(String, Source)>,
    keychain_available: bool,
}

impl Credentials {
    pub fn new(environment: Option<String>, keychain: Box<dyn SecretStore>) -> Self {
        Self {
            environment,
            keychain,
            state: Mutex::new(State {
                cached: None,
                keychain_available: true,
            }),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The key to plan with, and where it came from.
    pub fn current(&self) -> Option<(String, Source)> {
        if let Some(key) = &self.environment {
            return Some((key.clone(), Source::Environment));
        }
        let mut state = self.state();
        if let Some(found) = &state.cached {
            return Some(found.clone());
        }
        match self.keychain.get() {
            Ok(found) => {
                state.keychain_available = true;
                state.cached = found.map(|key| (key, Source::Keychain));
                state.cached.clone()
            }
            Err(_) => {
                state.keychain_available = false;
                None
            }
        }
    }

    /// Keep a key: in the keychain when it answers, for this session only when it does not.
    pub fn store(&self, key: &str) -> Result<Source, InvalidKey> {
        let key = validate(key)?;
        let mut state = self.state();
        let source = match self.keychain.set(&key) {
            Ok(()) => {
                state.keychain_available = true;
                Source::Keychain
            }
            Err(_) => {
                state.keychain_available = false;
                Source::Session
            }
        };
        state.cached = Some((key, source));
        Ok(source)
    }

    /// Remove the key from this machine. It is not revoked; that happens in the console.
    pub fn forget(&self) {
        let mut state = self.state();
        state.cached = None;
        state.keychain_available = self.keychain.delete().is_ok();
    }

    pub fn status(&self) -> Status {
        let current = self.current();
        Status {
            connected: current.is_some(),
            source: current.as_ref().map(|(_, source)| *source),
            key_hint: current.as_ref().and_then(|(key, _)| hint(key)),
            keychain_available: self.state().keychain_available,
        }
    }
}

fn validate(key: &str) -> Result<String, InvalidKey> {
    let key = key.trim();
    if key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(InvalidKey);
    }
    Ok(key.to_owned())
}

fn hint(key: &str) -> Option<String> {
    let characters: Vec<char> = key.chars().collect();
    (characters.len() > 8).then(|| characters[characters.len() - 4..].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "rdb_test_0123456789abcdef";

    #[test]
    fn the_environment_wins_and_says_so() {
        let keychain = Arc::new(MemoryStore::default());
        keychain.set("rdb_stored_key_000000").unwrap();
        let credentials = Credentials::new(Some(KEY.into()), Box::new(Arc::clone(&keychain)));
        assert_eq!(
            credentials.current(),
            Some((KEY.into(), Source::Environment))
        );
        credentials.forget();
        assert_eq!(credentials.status().source, Some(Source::Environment));
    }

    #[test]
    fn a_stored_key_is_found_again_by_the_next_process() {
        let keychain = Arc::new(MemoryStore::default());
        let first = Credentials::new(None, Box::new(Arc::clone(&keychain)));
        assert_eq!(first.store(KEY).unwrap(), Source::Keychain);

        let next = Credentials::new(None, Box::new(Arc::clone(&keychain)));
        assert_eq!(next.current(), Some((KEY.into(), Source::Keychain)));
    }

    #[test]
    fn without_a_keychain_the_key_lasts_the_session_and_the_status_says_so() {
        let credentials = Credentials::new(None, Box::new(MemoryStore::unavailable()));
        assert_eq!(credentials.store(KEY).unwrap(), Source::Session);
        let status = credentials.status();
        assert!(status.connected);
        assert_eq!(status.source, Some(Source::Session));
        assert!(!status.keychain_available);
    }

    #[test]
    fn forgetting_removes_the_keychain_entry() {
        let keychain = Arc::new(MemoryStore::default());
        let credentials = Credentials::new(None, Box::new(Arc::clone(&keychain)));
        credentials.store(KEY).unwrap();
        credentials.forget();
        assert!(!credentials.status().connected);
        assert_eq!(keychain.get().unwrap(), None);
    }

    #[test]
    fn the_status_shows_a_hint_and_never_the_key() {
        let credentials = Credentials::new(None, Box::new(MemoryStore::default()));
        credentials.store(KEY).unwrap();
        let status = credentials.status();
        assert_eq!(status.key_hint.as_deref(), Some("cdef"));
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains(KEY), "{encoded}");
    }

    #[test]
    fn rejects_keys_that_cannot_be_keys() {
        let credentials = Credentials::new(None, Box::new(MemoryStore::default()));
        for bad in [
            "",
            "   ",
            "rdb_with space",
            "rdb_tab\tkey",
            &"x".repeat(513),
        ] {
            assert!(credentials.store(bad).is_err(), "{bad:?} was accepted");
        }
        assert!(!credentials.status().connected);
    }
}
