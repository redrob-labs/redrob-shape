// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The shared Redrob Code daemon, as this sidecar sees it.
//!
//! Every Redrob app on a computer talks to one `redrob serve --register` (redrob-code
//! `docs/LOCAL-ENGINE-API.md`, section 3). The daemon holds the Console key, applies masking, and
//! answers an OpenAI-compatible `/v1/chat/completions`; this sidecar holds no key at all. There is
//! deliberately no path to the Console for chat: two paths would mean nobody could tell whether
//! masking applied to a given plan.
//!
//! `redrob-engine-client` does the finding and starting. What this module adds is the part that is
//! this app's: its name on every request, one session id per Blender run, the cached record with a
//! single re-read when the daemon's secret has rotated, and the three daemon routes the add-on's
//! connection flow needs (`PUT`/`DELETE /auth/redrob`, `GET /provider`).

use redrob_engine_client::{app_headers, openai_base, require_as, Record, Unavailable};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// How the daemon labels this app's requests (`x-redrob-app`) and its own start.
pub const APP: &str = "shape";
/// A daemon this sidecar starts stays up this long with nobody attached.
pub const IDLE_EXIT_MINUTES: u32 = 10;
/// The environment variable the add-on sets to one id per Blender session.
pub const SESSION_ENV: &str = "REDROB_SHAPE_SESSION";
/// Where the user gets Redrob Code when it is missing.
pub const INSTALL_URL: &str = "https://github.com/redrob-labs/redrob-launcher/releases/latest";
/// The provider the Console key belongs to, in the daemon's registry.
pub const PROVIDER: &str = "redrob";
const MAX_SESSION_BYTES: usize = 128;
const MAX_REASON_CHARS: usize = 240;

/// Why there is no daemon to talk to. Rendered by the add-on, so the message is for a person.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("Redrob Code is not installed. Install it with Redrob Launcher.")]
    NotInstalled,
    #[error("Redrob Code could not be started: {0}")]
    StartFailed(String),
}

impl EngineError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotInstalled => "engine_not_installed",
            Self::StartFailed(_) => "engine_start_failed",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::StartFailed(_))
    }
}

impl From<Unavailable> for EngineError {
    fn from(error: Unavailable) -> Self {
        match error {
            Unavailable::NotInstalled => Self::NotInstalled,
            Unavailable::StartFailed(reason) => Self::StartFailed(readable(&reason)),
        }
    }
}

/// The daemon, found once and kept until it stops answering.
pub struct Engine {
    session: String,
    record: Mutex<Option<Record>>,
}

impl Engine {
    pub fn new(session: String) -> Self {
        Self {
            session,
            record: Mutex::new(None),
        }
    }

    /// `REDROB_SHAPE_SESSION` from the add-on, else an id of this process's own. Either way one id
    /// per sidecar life, so the daemon can group one Blender session's requests.
    pub fn from_env() -> Self {
        let session = std::env::var(SESSION_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| {
                !value.is_empty()
                    && value.len() <= MAX_SESSION_BYTES
                    && !value.chars().any(|c| c.is_whitespace() || c.is_control())
            })
            .unwrap_or_else(|| {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default();
                format!("shape-{:x}-{nanos:x}", std::process::id())
            });
        Self::new(session)
    }

    pub fn session(&self) -> &str {
        &self.session
    }

    fn cached(&self) -> MutexGuard<'_, Option<Record>> {
        self.record
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The daemon's record: the cached one, or found (and if need be started) now.
    pub async fn record(&self) -> Result<Record, EngineError> {
        if let Some(record) = self.cached().clone() {
            return Ok(record);
        }
        self.fresh().await
    }

    /// Forget the cached record and find the daemon again. For a 401 or a refused connection: the
    /// secret rotates on every daemon start, so a stale record is the usual reason either happens.
    pub async fn fresh(&self) -> Result<Record, EngineError> {
        self.cached().take();
        // `require_as` reads a file, probes the daemon and may run `redrob service start`: blocking.
        let record = tokio::task::spawn_blocking(|| require_as(APP, IDLE_EXIT_MINUTES))
            .await
            .expect("finding the engine must not panic")?;
        *self.cached() = Some(record.clone());
        Ok(record)
    }

    /// What goes on every request to the daemon: its `Authorization`, this app's name, and this
    /// Blender session's id.
    pub fn headers(&self, record: &Record) -> Vec<(&'static str, String)> {
        app_headers(record, APP, Some(&self.session))
    }

    /// `<url>/v1/chat/completions`.
    pub fn chat_url(record: &Record) -> String {
        format!("{}/chat/completions", openai_base(record))
    }

    fn auth_url(record: &Record) -> String {
        format!("{}/auth/{PROVIDER}", record.url.trim_end_matches('/'))
    }

    fn provider_url(record: &Record) -> String {
        format!("{}/provider", record.url.trim_end_matches('/'))
    }
}

/// What the daemon says about the Console credential it holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderState {
    pub connected: bool,
    /// The daemon's word for where its credential came from: `api` (stored through this or another
    /// app), `env`, `launcher`, `config`.
    pub source: Option<String>,
    /// The last four characters of the key, when the daemon reports it. Never the key.
    pub key_hint: Option<String>,
}

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("the Redrob Code engine did not answer")]
    Transport,
    /// The record's secret no longer opens the daemon: it restarted.
    #[error("the Redrob Code engine refused this sidecar's credentials")]
    Unauthorized,
    #[error("the Redrob Code engine answered HTTP status {0}")]
    HttpStatus(u16),
    #[error("the Redrob Code engine answered something this version cannot read")]
    Malformed,
}

impl DaemonError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Transport => "transport_error",
            Self::Unauthorized => "engine_unauthorized",
            Self::HttpStatus(_) => "http_error",
            Self::Malformed => "malformed_response",
        }
    }

    /// Worth finding the daemon again and asking once more.
    pub fn stale(&self) -> bool {
        matches!(self, Self::Transport | Self::Unauthorized)
    }
}

fn apply_headers(
    mut request: reqwest::RequestBuilder,
    headers: Vec<(&'static str, String)>,
) -> reqwest::RequestBuilder {
    for (name, value) in headers {
        request = request.header(name, value);
    }
    request
}

fn check(status: StatusCode) -> Result<(), DaemonError> {
    if status.is_success() {
        Ok(())
    } else if status == StatusCode::UNAUTHORIZED {
        Err(DaemonError::Unauthorized)
    } else {
        Err(DaemonError::HttpStatus(status.as_u16()))
    }
}

/// `PUT /auth/redrob`: hand the Console key to the daemon. It is the only copy from here on.
pub async fn store_key(
    client: &Client,
    engine: &Engine,
    record: &Record,
    key: &str,
) -> Result<(), DaemonError> {
    let body = serde_json::json!({ "type": "api", "key": key });
    let response = apply_headers(client.put(Engine::auth_url(record)), engine.headers(record))
        .json(&body)
        .send()
        .await
        .map_err(|_| DaemonError::Transport)?;
    check(response.status())
}

/// `DELETE /auth/redrob`: the daemon forgets the Console key. Every Redrob app on this computer is
/// signed out by it, which is what sharing one engine means.
pub async fn forget_key(
    client: &Client,
    engine: &Engine,
    record: &Record,
) -> Result<(), DaemonError> {
    let response = apply_headers(
        client.delete(Engine::auth_url(record)),
        engine.headers(record),
    )
    .send()
    .await
    .map_err(|_| DaemonError::Transport)?;
    check(response.status())
}

/// `GET /provider`: whether the daemon holds a Console credential, and from where.
pub async fn provider_state(
    client: &Client,
    engine: &Engine,
    record: &Record,
) -> Result<ProviderState, DaemonError> {
    #[derive(Deserialize)]
    struct Listing {
        #[serde(default)]
        all: Vec<Provider>,
    }
    #[derive(Deserialize)]
    struct Provider {
        id: String,
        #[serde(default)]
        source: Option<String>,
        #[serde(default)]
        key: Option<String>,
    }
    let response = apply_headers(
        client.get(Engine::provider_url(record)),
        engine.headers(record),
    )
    .send()
    .await
    .map_err(|_| DaemonError::Transport)?;
    check(response.status())?;
    let listing: Listing = response.json().await.map_err(|_| DaemonError::Malformed)?;
    Ok(listing
        .all
        .into_iter()
        .find(|provider| provider.id == PROVIDER)
        .map(|provider| {
            let key = provider
                .key
                .as_deref()
                .map(str::trim)
                .filter(|key| !key.is_empty() && *key != "public");
            // The registry seeds the provider without a credential as `custom`; a credential
            // arriving from anywhere changes its source.
            let credentialed = key.is_some()
                || matches!(
                    provider.source.as_deref(),
                    Some("api" | "env" | "launcher" | "config")
                );
            ProviderState {
                connected: credentialed,
                source: credentialed.then_some(provider.source).flatten(),
                key_hint: key.and_then(hint),
            }
        })
        .unwrap_or_default())
}

fn hint(key: &str) -> Option<String> {
    let characters: Vec<char> = key.chars().collect();
    (characters.len() > 8).then(|| characters[characters.len() - 4..].iter().collect())
}

/// Text from a subprocess or the network, shown in Blender: one line, no control characters,
/// bounded.
pub fn readable(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    flat.chars().take(MAX_REASON_CHARS).collect()
}

#[cfg(test)]
pub(crate) mod fake {
    //! A stand-in for the daemon: one `127.0.0.1` HTTP/1.1 server, scripted per route, with a
    //! `REDROB_RUN_DIR` holding the record that points at it. Tests that set the environment take
    //! `ENV_LOCK` first, because the environment is one per process.

    use super::*;
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    pub static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// One request the fake daemon saw.
    #[derive(Clone, Debug)]
    pub struct Seen {
        pub method: String,
        pub path: String,
        pub headers: Vec<(String, String)>,
        pub body: String,
    }

    impl Seen {
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str())
        }
    }

    /// One scripted answer: status and JSON body.
    #[derive(Clone, Debug)]
    pub struct Reply {
        pub status: u16,
        pub body: String,
    }

    pub fn ok(body: serde_json::Value) -> Reply {
        Reply {
            status: 200,
            body: body.to_string(),
        }
    }

    pub fn status(status: u16) -> Reply {
        Reply {
            status,
            body: "{}".into(),
        }
    }

    #[derive(Default)]
    struct Script {
        /// Answers for `/v1/chat/completions`, in order; the last one repeats.
        chat: VecDeque<Reply>,
        /// Answers for `/provider`, in order; the last one repeats.
        provider: VecDeque<Reply>,
        /// Answers for `/auth/redrob`, in order; the last one repeats.
        auth: VecDeque<Reply>,
        /// The secret `/api/health` accepts. Everything else gets 401 when set.
        secret: Option<String>,
        seen: Vec<Seen>,
    }

    pub struct Daemon {
        pub port: u16,
        script: Arc<Mutex<Script>>,
        pub run_dir: PathBuf,
    }

    impl Daemon {
        pub async fn start(name: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let script = Arc::new(Mutex::new(Script::default()));
            let served = Arc::clone(&script);
            tokio::spawn(async move {
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        return;
                    };
                    let served = Arc::clone(&served);
                    tokio::spawn(async move {
                        let _ = serve_one(stream, served).await;
                    });
                }
            });
            let run_dir = std::env::temp_dir().join(format!(
                "redrob-shape-agent-{name}-{}-{port}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&run_dir);
            std::fs::create_dir_all(&run_dir).unwrap();
            Self {
                port,
                script,
                run_dir,
            }
        }

        fn script(&self) -> MutexGuard<'_, Script> {
            self.script
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        }

        pub fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }

        pub fn record(&self, secret: &str) -> Record {
            Record {
                id: format!("fake-{secret}"),
                version: "0.6.3".into(),
                url: self.url(),
                port: self.port,
                pid: std::process::id(),
                secret: secret.into(),
            }
        }

        /// Write `server.json` for `secret` and make `/api/health` accept only it.
        pub fn register(&self, secret: &str) -> Record {
            let record = self.record(secret);
            std::fs::write(
                self.run_dir.join("server.json"),
                serde_json::to_string(&record).unwrap(),
            )
            .unwrap();
            self.register_secret_only(secret);
            record
        }

        /// Accept only `secret`, without writing a record: a daemon nobody has found yet.
        pub fn register_secret_only(&self, secret: &str) {
            self.script().secret = Some(secret.into());
        }

        /// A record for a daemon that does not exist, for tests that never send anything.
        pub fn record_for_tests(secret: &str) -> Record {
            Record {
                id: format!("fake-{secret}"),
                version: "0.6.3".into(),
                url: "http://127.0.0.1:1".into(),
                port: 1,
                pid: 1,
                secret: secret.into(),
            }
        }

        pub fn chat(&self, reply: Reply) {
            self.script().chat.push_back(reply);
        }

        pub fn provider(&self, reply: Reply) {
            self.script().provider.push_back(reply);
        }

        pub fn auth(&self, reply: Reply) {
            self.script().auth.push_back(reply);
        }

        pub fn seen(&self) -> Vec<Seen> {
            self.script().seen.clone()
        }

        /// What `/v1/chat/completions` saw, in order.
        pub fn chats(&self) -> Vec<Seen> {
            self.seen()
                .into_iter()
                .filter(|seen| seen.path == "/v1/chat/completions")
                .collect()
        }
    }

    impl Drop for Daemon {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.run_dir);
        }
    }

    fn next(queue: &mut VecDeque<Reply>) -> Reply {
        if queue.len() > 1 {
            queue.pop_front().unwrap()
        } else {
            queue.front().cloned().unwrap_or_else(|| status(500))
        }
    }

    async fn serve_one(
        stream: tokio::net::TcpStream,
        script: Arc<Mutex<Script>>,
    ) -> std::io::Result<()> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_owned();
        let path = parts.next().unwrap_or_default().to_owned();
        let mut headers = Vec::new();
        let mut length = 0usize;
        loop {
            let mut header = String::new();
            reader.read_line(&mut header).await?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().unwrap_or(0);
                }
                headers.push((name.trim().to_owned(), value.trim().to_owned()));
            }
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).await?;
        let seen = Seen {
            method: method.clone(),
            path: path.clone(),
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        };
        let reply = {
            let mut script = script
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let authorized = script.secret.as_ref().is_none_or(|secret| {
                seen.header("authorization")
                    == Some(redrob_engine_client::authorization(secret).as_str())
            });
            script.seen.push(seen);
            if !authorized {
                status(401)
            } else {
                match path.as_str() {
                    "/api/health" => ok(serde_json::json!({ "healthy": true })),
                    "/v1/chat/completions" => next(&mut script.chat),
                    "/provider" => next(&mut script.provider),
                    "/auth/redrob" => next(&mut script.auth),
                    _ => status(404),
                }
            }
        };
        let response = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            reply.status,
            if reply.status == 200 { "OK" } else { "Error" },
            reply.body.len(),
            reply.body
        );
        writer.write_all(response.as_bytes()).await?;
        writer.shutdown().await?;
        Ok(())
    }

    /// A `redrob` stand-in for `REDROB_CODE_BIN`: `service start --json` prints `record`.
    #[cfg(unix)]
    pub fn stub_binary(dir: &std::path::Path, record: Option<&Record>) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("redrob");
        let script = match record {
            Some(record) => format!(
                "#!/bin/sh\nprintf '%s\\n' '{}'\n",
                serde_json::to_string(record).unwrap()
            ),
            None => "#!/bin/sh\necho 'port 4096 is taken' >&2\nexit 1\n".to_owned(),
        };
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)] // each test owns its runtime; the guard spans the test
mod tests {
    use super::fake::{ok, status, Daemon, ENV_LOCK};
    use super::*;

    fn client() -> Client {
        Client::builder().build().unwrap()
    }

    #[test]
    fn the_session_id_comes_from_the_add_on_or_is_minted_here() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(SESSION_ENV, " blender-7f3a ");
        assert_eq!(Engine::from_env().session(), "blender-7f3a");
        std::env::set_var(SESSION_ENV, "has space");
        assert!(Engine::from_env().session().starts_with("shape-"));
        std::env::remove_var(SESSION_ENV);
        let minted = Engine::from_env();
        assert!(minted.session().starts_with("shape-"));
        assert!(!minted.session().contains(char::is_whitespace));
    }

    #[test]
    fn headers_name_the_app_and_the_session_and_carry_the_secret() {
        let engine = Engine::new("blender-1".into());
        let record = Record {
            id: "r".into(),
            version: "0.6.3".into(),
            url: "http://127.0.0.1:4096/".into(),
            port: 4096,
            pid: 1,
            secret: "s3cret".into(),
        };
        let headers = engine.headers(&record);
        assert_eq!(
            headers[0],
            (
                "Authorization",
                redrob_engine_client::authorization("s3cret")
            )
        );
        assert!(headers.contains(&("x-redrob-app", "shape".to_owned())));
        assert!(headers.contains(&("x-redrob-session", "blender-1".to_owned())));
        assert_eq!(
            Engine::chat_url(&record),
            "http://127.0.0.1:4096/v1/chat/completions"
        );
        assert_eq!(
            Engine::auth_url(&record),
            "http://127.0.0.1:4096/auth/redrob"
        );
    }

    #[test]
    fn the_two_failures_have_their_own_codes_and_messages() {
        let missing = EngineError::from(Unavailable::NotInstalled);
        assert_eq!(missing.code(), "engine_not_installed");
        assert_eq!(
            missing.to_string(),
            "Redrob Code is not installed. Install it with Redrob Launcher."
        );
        assert!(!missing.retryable());
        let failed = EngineError::from(Unavailable::StartFailed(
            "redrob service start failed (exit status: 1):\n\tport taken".into(),
        ));
        assert_eq!(failed.code(), "engine_start_failed");
        assert_eq!(
            failed.to_string(),
            "Redrob Code could not be started: redrob service start failed (exit status: 1): port taken"
        );
        assert!(failed.retryable());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_provider_listing_says_whether_the_daemon_holds_a_key() {
        let daemon = Daemon::start("provider").await;
        let record = daemon.record("s");
        let engine = Engine::new("t".into());
        daemon.provider(ok(serde_json::json!({
            "all": [{ "id": "redrob", "source": "custom", "options": { "apiKey": "public" } }],
            "connected": ["redrob"]
        })));
        daemon.provider(ok(serde_json::json!({
            "all": [{ "id": "redrob", "source": "api", "key": "rdb_live_0123456789abcd" }],
            "connected": ["redrob"]
        })));
        daemon.provider(ok(serde_json::json!({
            "all": [{ "id": "redrob", "source": "launcher" }]
        })));
        daemon.provider(status(401));
        let client = client();
        let seeded = provider_state(&client, &engine, &record).await.unwrap();
        assert_eq!(seeded, ProviderState::default());
        let stored = provider_state(&client, &engine, &record).await.unwrap();
        assert_eq!(
            stored,
            ProviderState {
                connected: true,
                source: Some("api".into()),
                key_hint: Some("abcd".into()),
            }
        );
        let launcher = provider_state(&client, &engine, &record).await.unwrap();
        assert!(launcher.connected);
        assert_eq!(launcher.source.as_deref(), Some("launcher"));
        assert_eq!(launcher.key_hint, None);
        let refused = provider_state(&client, &engine, &record).await.unwrap_err();
        assert!(matches!(refused, DaemonError::Unauthorized));
        assert!(refused.stale());
        let seen = daemon.seen();
        assert!(seen
            .iter()
            .all(|s| s.path == "/provider" && s.method == "GET"));
        assert_eq!(seen[0].header("x-redrob-app"), Some("shape"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn storing_and_forgetting_use_the_auth_route_with_the_daemon_headers() {
        let daemon = Daemon::start("auth").await;
        let record = daemon.record("s");
        let engine = Engine::new("blender-9".into());
        daemon.auth(ok(serde_json::json!(true)));
        let client = client();
        store_key(&client, &engine, &record, "rdb_new_0123456789abcd")
            .await
            .unwrap();
        forget_key(&client, &engine, &record).await.unwrap();
        let seen = daemon.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(
            (seen[0].method.as_str(), seen[0].path.as_str()),
            ("PUT", "/auth/redrob")
        );
        let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["type"], "api");
        assert_eq!(body["key"], "rdb_new_0123456789abcd");
        assert_eq!(seen[0].header("x-redrob-session"), Some("blender-9"));
        assert_eq!(
            seen[0].header("authorization"),
            Some(redrob_engine_client::authorization("s").as_str())
        );
        assert_eq!(
            (seen[1].method.as_str(), seen[1].path.as_str()),
            ("DELETE", "/auth/redrob")
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_that_is_not_there_is_a_transport_error() {
        let engine = Engine::new("t".into());
        let record = Record {
            id: "r".into(),
            version: "0.6.3".into(),
            url: "http://127.0.0.1:1".into(),
            port: 1,
            pid: 1,
            secret: "s".into(),
        };
        let error = provider_state(&client(), &engine, &record)
            .await
            .unwrap_err();
        assert!(matches!(error, DaemonError::Transport), "{error:?}");
        assert!(error.stale());
    }

    #[test]
    fn readable_flattens_and_bounds_a_reason() {
        let noisy = format!("line one\nline\ttwo {}", "x".repeat(400));
        let text = readable(&noisy);
        assert!(text.starts_with("line one line two "));
        assert_eq!(text.chars().count(), MAX_REASON_CHARS);
    }
}
