// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the Redrob Console key lives: in the shared Redrob Code daemon, and nowhere in this app.
//!
//! The device flow still runs in this process, because it is how a person obtains a key, but the
//! key it yields is handed straight to the daemon (`PUT /auth/redrob`) and never kept here. Signed
//! in or not is whatever the daemon says (`GET /provider`). Disconnecting asks the daemon to forget
//! the key, which signs out every Redrob app on the computer: one engine, one login.
//!
//! Earlier versions kept the key in the operating system keychain under `ai.redrob.shape` /
//! `redrob`. That entry is read once, moved to the daemon, and deleted -- the only thing this module
//! still does with a keychain.

use crate::engine::{self, DaemonError, Engine, ProviderState};
use redrob_engine_client::Record;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// The legacy keychain slot. The real one is the OS keychain; tests use memory.
pub trait SecretStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, StoreError>;
    fn delete(&self) -> Result<(), StoreError>;
}

/// The operating system keychain, read and emptied only.
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

    fn delete(&self) -> Result<(), StoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(StoreError(error.to_string())),
        }
    }
}

/// A slot in memory, standing in for a keychain that holds (or does not hold) an old key.
#[derive(Default)]
pub struct MemoryStore {
    slot: Mutex<Option<String>>,
    unavailable: bool,
}

impl MemoryStore {
    pub fn holding(key: &str) -> Self {
        Self {
            slot: Mutex::new(Some(key.to_owned())),
            unavailable: false,
        }
    }

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
    fn delete(&self) -> Result<(), StoreError> {
        *self.slot()? = None;
        Ok(())
    }
}

/// What the add-on may show. It carries a hint of the key, never the key.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Status {
    pub connected: bool,
    /// `engine` while the daemon holds a Console credential; the key is never anywhere else.
    pub source: Option<&'static str>,
    /// The daemon's own word for where its credential came from (`api`, `env`, `launcher`,
    /// `config`), when it says.
    pub provider_source: Option<String>,
    /// The last four characters: enough to find the key in the console's list, nothing more.
    pub key_hint: Option<String>,
    /// The daemon that answered.
    pub engine_version: Option<String>,
}

impl Status {
    pub fn from_provider(state: ProviderState, record: &Record) -> Self {
        Self {
            connected: state.connected,
            source: state.connected.then_some("engine"),
            provider_source: state.source,
            key_hint: state.key_hint,
            engine_version: Some(record.version.clone()),
        }
    }
}

/// What became of a key an earlier version left in the keychain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Migration {
    /// Nothing there, or no keychain to ask: nothing to do, and never asked again.
    Nothing,
    /// Moved into the daemon and removed from the keychain.
    Moved,
    /// The daemon did not take it; the keychain entry stays, and the next start tries again.
    Deferred,
}

/// The Console credential, as seen through the daemon, plus the one-time move of a legacy key.
pub struct Credentials {
    legacy: Arc<dyn SecretStore>,
    migrated: AtomicBool,
}

impl Credentials {
    pub fn new(legacy: Arc<dyn SecretStore>) -> Self {
        Self {
            legacy,
            migrated: AtomicBool::new(false),
        }
    }

    /// The daemon's answer, as the add-on sees it.
    pub async fn status(
        &self,
        client: &reqwest::Client,
        engine: &Engine,
        record: &Record,
    ) -> Result<Status, DaemonError> {
        let state = engine::provider_state(client, engine, record).await?;
        Ok(Status::from_provider(state, record))
    }

    /// Hand a key to the daemon. Checked for shape first, so a paste with a stray space fails here
    /// instead of as a mystery 401 later.
    pub async fn store(
        &self,
        client: &reqwest::Client,
        engine: &Engine,
        record: &Record,
        key: &str,
    ) -> Result<Result<(), DaemonError>, InvalidKey> {
        let key = validate(key)?;
        Ok(engine::store_key(client, engine, record, &key).await)
    }

    /// Ask the daemon to forget the key. It is not revoked; that happens in the console.
    pub async fn forget(
        &self,
        client: &reqwest::Client,
        engine: &Engine,
        record: &Record,
    ) -> Result<(), DaemonError> {
        engine::forget_key(client, engine, record).await
    }

    /// Move a key an earlier version stored in the keychain into the daemon, once per process.
    /// Keychain calls block, and on macOS can wait for the user to answer a prompt.
    pub async fn migrate(
        &self,
        client: &reqwest::Client,
        engine: &Engine,
        record: &Record,
    ) -> Migration {
        if self.migrated.swap(true, Ordering::AcqRel) {
            return Migration::Nothing;
        }
        let legacy = Arc::clone(&self.legacy);
        let found = tokio::task::spawn_blocking(move || legacy.get())
            .await
            .expect("keychain work must not panic");
        let Ok(Some(key)) = found else {
            return Migration::Nothing;
        };
        let Ok(key) = validate(&key) else {
            // Not a key: nothing the daemon could use, so nothing to move. Leave it be.
            return Migration::Nothing;
        };
        if engine::store_key(client, engine, record, &key)
            .await
            .is_err()
        {
            self.migrated.store(false, Ordering::Release);
            return Migration::Deferred;
        }
        let legacy = Arc::clone(&self.legacy);
        let _ = tokio::task::spawn_blocking(move || legacy.delete()).await;
        Migration::Moved
    }
}

pub fn validate(key: &str) -> Result<String, InvalidKey> {
    let key = key.trim();
    if key.is_empty()
        || key.len() > MAX_KEY_BYTES
        || key.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(InvalidKey);
    }
    Ok(key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::fake::{ok, status, Daemon};

    const KEY: &str = "rdb_test_0123456789abcdef";

    fn client() -> reqwest::Client {
        reqwest::Client::builder().build().unwrap()
    }

    #[test]
    fn rejects_keys_that_cannot_be_keys() {
        for bad in [
            "",
            "   ",
            "rdb_with space",
            "rdb_tab\tkey",
            &"x".repeat(513),
        ] {
            assert!(validate(bad).is_err(), "{bad:?} was accepted");
        }
        assert_eq!(
            validate("  rdb_ok_0123456789  ").unwrap(),
            "rdb_ok_0123456789"
        );
    }

    #[test]
    fn the_status_shows_a_hint_and_never_the_key() {
        let record = Daemon::record_for_tests("s");
        let status = Status::from_provider(
            ProviderState {
                connected: true,
                source: Some("api".into()),
                key_hint: Some("cdef".into()),
            },
            &record,
        );
        assert_eq!(status.source, Some("engine"));
        assert_eq!(status.key_hint.as_deref(), Some("cdef"));
        assert_eq!(status.engine_version.as_deref(), Some("0.6.3"));
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains(KEY), "{encoded}");
        assert!(encoded.contains("\"provider_source\":\"api\""), "{encoded}");

        let signed_out = Status::from_provider(ProviderState::default(), &record);
        assert!(!signed_out.connected);
        assert_eq!(signed_out.source, None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_legacy_keychain_key_is_moved_into_the_daemon_once_and_the_keychain_emptied() {
        let daemon = Daemon::start("migrate").await;
        let record = daemon.record("s");
        let engine = Engine::new("t".into());
        daemon.auth(ok(serde_json::json!(true)));
        let keychain = Arc::new(MemoryStore::holding(KEY));
        let credentials = Credentials::new(Arc::clone(&keychain) as Arc<dyn SecretStore>);
        let client = client();
        assert_eq!(
            credentials.migrate(&client, &engine, &record).await,
            Migration::Moved
        );
        assert_eq!(keychain.get().unwrap(), None, "the keychain entry stays");
        let seen = daemon.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("PUT", "/auth/redrob")
        );
        assert!(seen[0].body.contains(KEY));
        // Asked again in the same process: nothing happens, and the daemon is not bothered.
        assert_eq!(
            credentials.migrate(&client, &engine, &record).await,
            Migration::Nothing
        );
        assert_eq!(daemon.seen().len(), 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_that_refuses_the_key_leaves_the_keychain_alone_for_next_time() {
        let daemon = Daemon::start("migrate-refused").await;
        let record = daemon.record("s");
        let engine = Engine::new("t".into());
        daemon.auth(status(500));
        daemon.auth(ok(serde_json::json!(true)));
        let keychain = Arc::new(MemoryStore::holding(KEY));
        let credentials = Credentials::new(Arc::clone(&keychain) as Arc<dyn SecretStore>);
        let client = client();
        assert_eq!(
            credentials.migrate(&client, &engine, &record).await,
            Migration::Deferred
        );
        assert_eq!(keychain.get().unwrap().as_deref(), Some(KEY));
        // The next attempt (this process or the next start) gets to try again.
        assert_eq!(
            credentials.migrate(&client, &engine, &record).await,
            Migration::Moved
        );
        assert_eq!(keychain.get().unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn no_keychain_and_an_empty_keychain_both_mean_nothing_to_move() {
        let daemon = Daemon::start("migrate-nothing").await;
        let record = daemon.record("s");
        let engine = Engine::new("t".into());
        let client = client();
        for store in [MemoryStore::default(), MemoryStore::unavailable()] {
            let credentials = Credentials::new(Arc::new(store));
            assert_eq!(
                credentials.migrate(&client, &engine, &record).await,
                Migration::Nothing
            );
        }
        assert!(daemon.seen().is_empty(), "the daemon was asked for nothing");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn store_checks_the_shape_before_asking_the_daemon() {
        let daemon = Daemon::start("store-shape").await;
        let record = daemon.record("s");
        let engine = Engine::new("t".into());
        let credentials = Credentials::new(Arc::new(MemoryStore::default()));
        assert!(credentials
            .store(&client(), &engine, &record, "rdb with space")
            .await
            .is_err());
        assert!(daemon.seen().is_empty());
    }
}
