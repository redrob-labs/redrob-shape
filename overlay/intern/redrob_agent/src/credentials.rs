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
//!
//! When this app has no key of its own, the computer's shared sign-in from Redrob Launcher is the
//! fallback: one `account.json` the launcher writes and every Redrob app may read (redrob-launcher
//! `docs/shared-account.md`). It is read at the moment a key is needed, never cached, because the
//! person may sign in or out in the launcher while Blender is open. Nothing here writes or deletes
//! it: disconnecting removes only this app's own key, and the launcher's stays.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

pub const KEYCHAIN_SERVICE: &str = "ai.redrob.shape";
pub const KEYCHAIN_ACCOUNT: &str = "redrob";
/// Overrides the shared account file's directory (Redrob Launcher's `REDROB_SHARED_DIR`).
pub const SHARED_DIR_ENV: &str = "REDROB_SHARED_DIR";
const SHARED_FILE_NAME: &str = "account.json";
const MAX_KEY_BYTES: usize = 512;
const MAX_SHARED_FILE_BYTES: u64 = 64 * 1024;

/// Redrob Launcher's shared account file for this platform, or `None` when the environment does
/// not say where the user's configuration lives.
pub fn shared_account_path() -> Option<PathBuf> {
    shared_account_path_in(|key| {
        std::env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    })
}

/// The rule itself, over any environment: `REDROB_SHARED_DIR` replaces the directory, otherwise
/// `%APPDATA%\Redrob\Shared`, `~/Library/Application Support/ai.redrob.shared`, or
/// `$XDG_CONFIG_HOME/redrob-shared` (default `~/.config/redrob-shared`). The file name is fixed.
fn shared_account_path_in(env: impl Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let dir = if let Some(dir) = env(SHARED_DIR_ENV) {
        dir
    } else if cfg!(windows) {
        env("APPDATA")?.join("Redrob").join("Shared")
    } else if cfg!(target_os = "macos") {
        env("HOME")?.join("Library/Application Support/ai.redrob.shared")
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|home| home.join(".config")))?
            .join("redrob-shared")
    };
    Some(dir.join(SHARED_FILE_NAME))
}

/// What the launcher's file says, with the key already checked to be a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedAccount {
    pub api_key: String,
    pub api_key_name: String,
    pub account_name: String,
}

/// Read the shared file as it is right now. A missing, unreadable, oversized or malformed file,
/// or one without an `apiKey`, means "not signed in" -- never an error the user has to see.
pub fn read_shared_account(path: &Path) -> Option<SharedAccount> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_SHARED_FILE_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_owned()
    };
    let api_key = validate(&field("apiKey")).ok()?;
    Some(SharedAccount {
        api_key,
        api_key_name: field("apiKeyName"),
        account_name: field("accountName"),
    })
}

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
    /// Redrob Launcher's shared sign-in: this app has no key of its own and uses the computer's.
    Launcher,
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
    /// The workspace the launcher is signed in to. Only with `Source::Launcher`; this app's own
    /// key already told the add-on its account when it was connected.
    pub account_name: Option<String>,
}

pub struct Credentials {
    environment: Option<String>,
    keychain: Box<dyn SecretStore>,
    /// Redrob Launcher's shared account file, read fresh whenever this app has no key of its own.
    shared: Option<PathBuf>,
    state: Mutex<State>,
}

struct State {
    /// The key once read or stored, so a keychain that prompts does not prompt for every plan.
    /// It is also the only copy when the keychain could not take the key.
    cached: Option<(String, Source)>,
    keychain_available: bool,
}

impl Credentials {
    /// Production: the environment, the OS keychain, then Redrob Launcher's shared sign-in.
    pub fn new(environment: Option<String>, keychain: Box<dyn SecretStore>) -> Self {
        Self::with_shared(environment, keychain, shared_account_path())
    }

    /// The same, with the shared account file at an explicit place (`None`: no fallback at all).
    pub fn with_shared(
        environment: Option<String>,
        keychain: Box<dyn SecretStore>,
        shared: Option<PathBuf>,
    ) -> Self {
        Self {
            environment,
            keychain,
            shared,
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

    /// This app's own key, if it has one: from the environment, the cache, or the keychain.
    fn own(&self) -> Option<(String, Source)> {
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

    /// The launcher's shared sign-in, read now. Never cached: the file is the launcher's to change.
    fn shared(&self) -> Option<SharedAccount> {
        self.shared.as_deref().and_then(read_shared_account)
    }

    /// The key to plan with, and where it came from: this app's own key wins, the computer's shared
    /// sign-in is the fallback.
    pub fn current(&self) -> Option<(String, Source)> {
        if let Some(own) = self.own() {
            return Some(own);
        }
        self.shared()
            .map(|account| (account.api_key, Source::Launcher))
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

    /// Remove this app's own key from this machine. It is not revoked; that happens in the console.
    /// Redrob Launcher's shared sign-in is not this app's to remove, so it stays, and `current()`
    /// keeps answering with it.
    pub fn forget(&self) {
        let mut state = self.state();
        state.cached = None;
        state.keychain_available = self.keychain.delete().is_ok();
    }

    pub fn status(&self) -> Status {
        let (current, account_name) = match self.own() {
            Some(own) => (Some(own), None),
            None => match self.shared() {
                Some(account) => (
                    Some((account.api_key, Source::Launcher)),
                    (!account.account_name.is_empty()).then_some(account.account_name),
                ),
                None => (None, None),
            },
        };
        Status {
            connected: current.is_some(),
            source: current.as_ref().map(|(_, source)| *source),
            key_hint: current.as_ref().and_then(|(key, _)| hint(key)),
            keychain_available: self.state().keychain_available,
            account_name,
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
        let credentials =
            Credentials::with_shared(Some(KEY.into()), Box::new(Arc::clone(&keychain)), None);
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
        let first = Credentials::with_shared(None, Box::new(Arc::clone(&keychain)), None);
        assert_eq!(first.store(KEY).unwrap(), Source::Keychain);

        let next = Credentials::with_shared(None, Box::new(Arc::clone(&keychain)), None);
        assert_eq!(next.current(), Some((KEY.into(), Source::Keychain)));
    }

    #[test]
    fn without_a_keychain_the_key_lasts_the_session_and_the_status_says_so() {
        let credentials =
            Credentials::with_shared(None, Box::new(MemoryStore::unavailable()), None);
        assert_eq!(credentials.store(KEY).unwrap(), Source::Session);
        let status = credentials.status();
        assert!(status.connected);
        assert_eq!(status.source, Some(Source::Session));
        assert!(!status.keychain_available);
    }

    #[test]
    fn forgetting_removes_the_keychain_entry() {
        let keychain = Arc::new(MemoryStore::default());
        let credentials = Credentials::with_shared(None, Box::new(Arc::clone(&keychain)), None);
        credentials.store(KEY).unwrap();
        credentials.forget();
        assert!(!credentials.status().connected);
        assert_eq!(keychain.get().unwrap(), None);
    }

    #[test]
    fn the_status_shows_a_hint_and_never_the_key() {
        let credentials = Credentials::with_shared(None, Box::new(MemoryStore::default()), None);
        credentials.store(KEY).unwrap();
        let status = credentials.status();
        assert_eq!(status.key_hint.as_deref(), Some("cdef"));
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains(KEY), "{encoded}");
    }

    #[test]
    fn rejects_keys_that_cannot_be_keys() {
        let credentials = Credentials::with_shared(None, Box::new(MemoryStore::default()), None);
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

    /// A private stand-in for `$REDROB_SHARED_DIR`, so no test reads or writes the real
    /// `~/.config/redrob-shared`. Removed when dropped.
    struct SharedDir(PathBuf);

    impl SharedDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("redrob-shape-agent-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self) -> PathBuf {
            self.0.join(SHARED_FILE_NAME)
        }

        /// What Redrob Launcher writes (redrob-launcher `docs/shared-account.md`).
        fn sign_in(&self, key: &str) {
            let text = serde_json::json!({
                "apiKey": key,
                "apiKeyName": "Redrob Launcher (2026-10-12)",
                "accountName": "Acme",
                "apiBaseUrl": "https://console.redrob.ai/api/backend/v1",
            });
            std::fs::write(self.file(), text.to_string()).unwrap();
        }

        fn sign_out(&self) {
            std::fs::remove_file(self.file()).unwrap();
        }
    }

    impl Drop for SharedDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const LAUNCHER_KEY: &str = "rdb_launcher_0123456789wxyz";

    #[test]
    fn the_shared_directory_comes_from_the_override_then_the_platform_default() {
        let lookup = |vars: &'static [(&str, &str)]| {
            move |key: &str| {
                vars.iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| PathBuf::from(value))
            }
        };
        let overridden = shared_account_path_in(lookup(&[
            (SHARED_DIR_ENV, "/elsewhere/shared"),
            ("HOME", "/home/someone"),
        ]));
        assert_eq!(
            overridden,
            Some(PathBuf::from("/elsewhere/shared/account.json"))
        );

        let default = shared_account_path_in(lookup(&[
            ("HOME", "/home/someone"),
            ("APPDATA", r"C:\Users\someone\AppData\Roaming"),
        ]))
        .expect("a home is enough");
        assert_eq!(default.file_name().unwrap(), SHARED_FILE_NAME);
        let expected = if cfg!(windows) {
            PathBuf::from(r"C:\Users\someone\AppData\Roaming")
                .join("Redrob")
                .join("Shared")
        } else if cfg!(target_os = "macos") {
            PathBuf::from("/home/someone/Library/Application Support/ai.redrob.shared")
        } else {
            PathBuf::from("/home/someone/.config/redrob-shared")
        };
        assert_eq!(default.parent().unwrap(), expected);

        if cfg!(all(unix, not(target_os = "macos"))) {
            let xdg = shared_account_path_in(lookup(&[
                ("XDG_CONFIG_HOME", "/xdg"),
                ("HOME", "/home/someone"),
            ]));
            assert_eq!(xdg, Some(PathBuf::from("/xdg/redrob-shared/account.json")));
        }
        assert_eq!(shared_account_path_in(lookup(&[])), None);
    }

    #[test]
    fn the_shared_launcher_key_is_the_fallback_and_is_read_when_it_is_needed() {
        let dir = SharedDir::new("fallback");
        let credentials =
            Credentials::with_shared(None, Box::new(MemoryStore::default()), Some(dir.file()));
        // No key anywhere: not signed in.
        assert_eq!(credentials.current(), None);
        let status = credentials.status();
        assert!(!status.connected);
        assert_eq!(status.account_name, None);

        // The launcher signs in while this process runs: used, and flagged as the launcher's.
        dir.sign_in(LAUNCHER_KEY);
        assert_eq!(
            credentials.current(),
            Some((LAUNCHER_KEY.into(), Source::Launcher))
        );
        let status = credentials.status();
        assert!(status.connected);
        assert_eq!(status.source, Some(Source::Launcher));
        assert_eq!(status.key_hint.as_deref(), Some("wxyz"));
        assert_eq!(status.account_name.as_deref(), Some("Acme"));
        let encoded = serde_json::to_string(&status).unwrap();
        assert!(!encoded.contains(LAUNCHER_KEY), "{encoded}");

        // The launcher signs out: not signed in again, with nothing cached from before.
        dir.sign_out();
        assert_eq!(credentials.current(), None);
        assert!(!credentials.status().connected);
    }

    #[test]
    fn this_apps_own_key_wins_over_the_shared_one() {
        let dir = SharedDir::new("own-wins");
        dir.sign_in(LAUNCHER_KEY);
        let keychain = Arc::new(MemoryStore::default());
        let credentials =
            Credentials::with_shared(None, Box::new(Arc::clone(&keychain)), Some(dir.file()));
        credentials.store(KEY).unwrap();
        assert_eq!(credentials.current(), Some((KEY.into(), Source::Keychain)));
        let status = credentials.status();
        assert_eq!(status.source, Some(Source::Keychain));
        assert_eq!(status.account_name, None);

        // A key found in the keychain by the next process also wins.
        let next = Credentials::with_shared(None, Box::new(keychain), Some(dir.file()));
        assert_eq!(next.current(), Some((KEY.into(), Source::Keychain)));

        // So does the environment.
        let env = Credentials::with_shared(
            Some("rdb_env_0123456789abcdef".into()),
            Box::new(MemoryStore::default()),
            Some(dir.file()),
        );
        assert_eq!(env.current().unwrap().1, Source::Environment);
    }

    #[test]
    fn disconnecting_removes_only_this_apps_key_and_the_shared_file_survives() {
        let dir = SharedDir::new("disconnect");
        dir.sign_in(LAUNCHER_KEY);
        let keychain = Arc::new(MemoryStore::default());
        let credentials =
            Credentials::with_shared(None, Box::new(Arc::clone(&keychain)), Some(dir.file()));
        credentials.store(KEY).unwrap();
        credentials.forget();
        assert_eq!(keychain.get().unwrap(), None);
        assert!(dir.file().is_file(), "the launcher's file was touched");
        // With its own key gone, the app falls back to the launcher's.
        assert_eq!(
            credentials.current(),
            Some((LAUNCHER_KEY.into(), Source::Launcher))
        );
        // Disconnecting again, with nothing of its own left, still leaves the launcher alone.
        credentials.forget();
        assert!(dir.file().is_file());
        assert_eq!(credentials.status().source, Some(Source::Launcher));
    }

    #[test]
    fn a_shared_file_that_is_not_a_sign_in_means_not_signed_in() {
        let dir = SharedDir::new("malformed");
        let credentials =
            Credentials::with_shared(None, Box::new(MemoryStore::default()), Some(dir.file()));
        for text in [
            "",
            "not json",
            "[]",
            r#"{"apiKeyName":"x"}"#,
            r#"{"apiKey":""}"#,
            r#"{"apiKey":"   "}"#,
            r#"{"apiKey":"has a space"}"#,
            r#"{"apiKey":12345}"#,
        ] {
            std::fs::write(dir.file(), text).unwrap();
            assert_eq!(credentials.current(), None, "{text:?} was accepted");
        }
        assert_eq!(
            read_shared_account(&dir.0),
            None,
            "a directory is not a file"
        );
    }
}
