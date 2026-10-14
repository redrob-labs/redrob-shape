// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::api::{ApiError, RedrobClient};
use crate::conversation::{self, Turn};
use crate::credentials::{Credentials, Status};
use crate::device::{self, Authorization, DeviceClient, DeviceError, Grant};
use crate::engine::{DaemonError, Engine, EngineError, INSTALL_URL};
use crate::validation::Plan;
use futures_util::StreamExt;
use redrob_engine_client::Record;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinSet;
use tokio_util::codec::{FramedRead, LinesCodec, LinesCodecError};
use tokio_util::sync::CancellationToken;

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_INSTRUCTION_BYTES: usize = 16 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_ACTIVE_PLANS: usize = 128;
const EVENT_CHANNEL_CAPACITY: usize = 256;

const DAEMON_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Clone)]
pub struct Agent {
    client: Option<Arc<RedrobClient>>,
    device: Option<Arc<DeviceClient>>,
    unavailable: Option<UnavailableReason>,
    /// The shared Redrob Code daemon: where plans go and where the Console key lives.
    engine: Arc<Engine>,
    credentials: Arc<Credentials>,
    /// For the daemon's own routes (`/auth/redrob`, `/provider`): loopback, short timeout.
    http: reqwest::Client,
    /// The connection waiting for approval, if any. One at a time: starting again replaces it.
    pending: Arc<Mutex<Option<Authorization>>>,
}

#[derive(Clone, Debug)]
struct UnavailableReason {
    code: &'static str,
    message: String,
}

impl Agent {
    pub fn new(
        client: RedrobClient,
        device: DeviceClient,
        engine: Engine,
        credentials: Credentials,
    ) -> Self {
        Self {
            client: Some(Arc::new(client)),
            device: Some(Arc::new(device)),
            unavailable: None,
            engine: Arc::new(engine),
            credentials: Arc::new(credentials),
            http: daemon_http(),
            pending: Arc::new(Mutex::new(None)),
        }
    }

    /// An agent whose configuration is unusable. It still answers status requests, so the add-on can
    /// say what is wrong instead of failing without a word.
    pub fn unavailable(
        code: &'static str,
        message: String,
        engine: Engine,
        credentials: Credentials,
    ) -> Self {
        Self {
            client: None,
            device: None,
            unavailable: Some(UnavailableReason { code, message }),
            engine: Arc::new(engine),
            credentials: Arc::new(credentials),
            http: daemon_http(),
            pending: Arc::new(Mutex::new(None)),
        }
    }

    fn unavailable_reason(&self) -> UnavailableReason {
        self.unavailable
            .clone()
            .expect("an unavailable agent must include a reason")
    }

    /// Run one daemon operation against the record as cached; if the daemon refused the secret or
    /// nothing answered, find the daemon again once and run it against the fresh record. A key an
    /// earlier version left in the keychain is moved into the daemon the first time one is found.
    async fn with_daemon<T, F, Fut>(&self, operation: F) -> Result<T, AgentError>
    where
        F: Fn(Record) -> Fut,
        Fut: Future<Output = Result<T, DaemonError>>,
    {
        let record = self.engine.record().await?;
        self.credentials
            .migrate(&self.http, &self.engine, &record)
            .await;
        match operation(record).await {
            Ok(value) => Ok(value),
            Err(error) if error.stale() => {
                let record = self.engine.fresh().await?;
                operation(record).await.map_err(AgentError::Daemon)
            }
            Err(error) => Err(AgentError::Daemon(error)),
        }
    }

    /// What the daemon says about the Console credential.
    async fn status(&self) -> Result<Status, AgentError> {
        self.with_daemon(|record| async move {
            self.credentials
                .status(&self.http, &self.engine, &record)
                .await
        })
        .await
    }

    /// Hand a key to the daemon, then report the status as it stands.
    async fn store(&self, key: &str) -> Result<Status, AgentError> {
        let key = crate::credentials::validate(key).map_err(|_| AgentError::InvalidKey)?;
        self.with_daemon(|record| {
            let key = key.clone();
            async move {
                self.credentials
                    .store(&self.http, &self.engine, &record, &key)
                    .await
                    .expect("validated above")?;
                self.credentials
                    .status(&self.http, &self.engine, &record)
                    .await
            }
        })
        .await
    }

    async fn forget(&self) -> Result<Status, AgentError> {
        self.with_daemon(|record| async move {
            self.credentials
                .forget(&self.http, &self.engine, &record)
                .await?;
            self.credentials
                .status(&self.http, &self.engine, &record)
                .await
        })
        .await
    }

    async fn plan(
        &self,
        instruction: &str,
        context: &Value,
        history: &[Turn],
    ) -> Result<Plan, AgentError> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| AgentError::Unavailable(self.unavailable_reason()))?;
        // The daemon decides with what key; this only asks first so "not signed in" is said in
        // those words instead of as a refusal from the Console.
        if !self.status().await?.connected {
            return Err(AgentError::MissingKey);
        }
        client
            .plan(&self.engine, instruction, context, history)
            .await
            .map_err(Into::into)
    }
}

fn daemon_http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DAEMON_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("redrob-shape-agent/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("a loopback HTTP client builds")
}

/// What `health` says about the daemon, next to the connection.
fn engine_block(outcome: &Result<Status, AgentError>) -> Value {
    match outcome {
        Ok(status) => json!({
            "available": true,
            "version": status.engine_version,
        }),
        Err(error) => {
            let mut block = json!({
                "available": false,
                "code": error.code(),
                "message": error.to_string(),
            });
            if matches!(error, AgentError::Engine(EngineError::NotInstalled)) {
                block["install_url"] = json!(INSTALL_URL);
            }
            block
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(u64),
}

impl RequestId {
    fn is_valid(&self) -> bool {
        match self {
            Self::String(value) => {
                !value.is_empty()
                    && value.len() <= MAX_REQUEST_ID_BYTES
                    && !value.chars().any(char::is_control)
            }
            Self::Number(_) => true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestEnvelope {
    version: u32,
    id: RequestId,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyParams {}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanParams {
    instruction: String,
    #[serde(default)]
    context: Value,
    /// Earlier turns of this conversation, oldest first. Absent for a first request.
    #[serde(default)]
    history: Vec<Turn>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelParams {
    request_id: RequestId,
}

/// A key pasted by the user. It goes to the same place a device-flow key does.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoreKeyParams {
    key: String,
}

impl std::fmt::Debug for StoreKeyParams {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("StoreKeyParams { key: [redacted] }")
    }
}

#[derive(Debug)]
enum Request {
    Health(EmptyParams),
    Plan(PlanParams),
    Cancel(CancelParams),
    Shutdown(EmptyParams),
    ConnectionStatus(EmptyParams),
    ConnectStart(EmptyParams),
    ConnectWait(EmptyParams),
    Disconnect(EmptyParams),
    StoreKey(StoreKeyParams),
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    Success {
        version: u32,
        id: Option<RequestId>,
        result: Value,
    },
    Error {
        version: u32,
        id: Option<RequestId>,
        error: ErrorBody,
    },
}

impl Event {
    fn success(id: RequestId, result: Value) -> Self {
        Self::Success {
            version: PROTOCOL_VERSION,
            id: Some(id),
            result,
        }
    }

    fn error(
        id: Option<RequestId>,
        code: &str,
        message: impl Into<String>,
        retryable: bool,
    ) -> Self {
        Self::Error {
            version: PROTOCOL_VERSION,
            id,
            error: ErrorBody {
                code: code.to_owned(),
                message: message.into(),
                retryable,
            },
        }
    }

    fn id(&self) -> Option<RequestId> {
        match self {
            Self::Success { id, .. } | Self::Error { id, .. } => id.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    retryable: bool,
}

pub async fn serve<R, W>(reader: R, writer: W, agent: Agent) -> Result<(), ProtocolIoError>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut lines = FramedRead::new(reader, LinesCodec::new_with_max_length(MAX_REQUEST_BYTES));
    let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
    let writer_task = tokio::spawn(write_events(writer, event_rx));
    let active = Arc::new(Mutex::new(HashMap::<RequestId, CancellationToken>::new()));
    let mut tasks = JoinSet::new();

    while let Some(line_result) = lines.next().await {
        while tasks.try_join_next().is_some() {}

        let line = match line_result {
            Ok(line) => line,
            Err(LinesCodecError::MaxLineLengthExceeded) => {
                send_event(
                    &event_tx,
                    Event::error(None, "request_too_large", "request exceeds 64 KiB", false),
                )
                .await?;
                continue;
            }
            Err(LinesCodecError::Io(error)) => return Err(ProtocolIoError::Read(error)),
        };

        let envelope = match serde_json::from_str::<RequestEnvelope>(&line) {
            Ok(envelope) => envelope,
            Err(_) => {
                send_event(
                    &event_tx,
                    Event::error(None, "invalid_request", "invalid request envelope", false),
                )
                .await?;
                continue;
            }
        };
        let id = envelope.id.clone();
        if !id.is_valid() {
            send_event(
                &event_tx,
                Event::error(Some(id), "invalid_request_id", "invalid request id", false),
            )
            .await?;
            continue;
        }
        if envelope.version != PROTOCOL_VERSION {
            send_event(
                &event_tx,
                Event::error(
                    Some(id),
                    "unsupported_version",
                    "only protocol version 1 is supported",
                    false,
                ),
            )
            .await?;
            continue;
        }
        if active.lock().await.contains_key(&id) {
            send_event(
                &event_tx,
                Event::error(
                    None,
                    "duplicate_request_id",
                    "request id belongs to an active plan",
                    false,
                ),
            )
            .await?;
            continue;
        }

        let request = match decode_request(&envelope.method, envelope.params) {
            Ok(request) => request,
            Err(error) => {
                send_event(
                    &event_tx,
                    Event::error(Some(id), error.code, error.message, false),
                )
                .await?;
                continue;
            }
        };

        match request {
            Request::Health(_params) => {
                let outcome = agent.status().await;
                let status = outcome.as_ref().ok().cloned().unwrap_or_default();
                let mut result = json!({
                    "service": "redrob-shape-agent",
                    "configured": agent.client.is_some() && status.connected,
                    "connection": status,
                    "engine": engine_block(&outcome),
                });
                if let Some(reason) = &agent.unavailable {
                    result["reason"] = json!(reason.code);
                } else if let Err(error) = &outcome {
                    result["reason"] = json!(error.code());
                } else if !status.connected {
                    result["reason"] = json!("missing_api_key");
                }
                send_event(&event_tx, Event::success(id, result)).await?;
            }
            Request::Plan(params) => {
                if params.instruction.trim().is_empty()
                    || params.instruction.len() > MAX_INSTRUCTION_BYTES
                {
                    send_event(
                        &event_tx,
                        Event::error(
                            Some(id),
                            "invalid_params",
                            "instruction must be between 1 byte and 16 KiB",
                            false,
                        ),
                    )
                    .await?;
                    continue;
                }
                if let Err(error) = conversation::validate(&params.history) {
                    send_event(
                        &event_tx,
                        Event::error(Some(id), "invalid_params", error.to_string(), false),
                    )
                    .await?;
                    continue;
                }
                if let Some(reason) = &agent.unavailable {
                    send_event(
                        &event_tx,
                        Event::error(Some(id), reason.code, reason.message.clone(), false),
                    )
                    .await?;
                    continue;
                }
                let token = match register(&active, &id).await {
                    Ok(token) => token,
                    Err(event) => {
                        send_event(&event_tx, event).await?;
                        continue;
                    }
                };

                let task_agent = agent.clone();
                let task_tx = event_tx.clone();
                let task_active = Arc::clone(&active);
                tasks.spawn(async move {
                    let outcome = tokio::select! {
                        _ = token.cancelled() => Err(AgentError::Cancelled),
                        result = task_agent.plan(&params.instruction, &params.context, &params.history) => result,
                    };
                    let event = match outcome {
                        Ok(plan) => match serde_json::to_value(plan) {
                            Ok(result) => Event::success(id.clone(), result),
                            Err(_) => Event::error(
                                Some(id.clone()),
                                "internal_error",
                                "failed to encode validated plan",
                                false,
                            ),
                        },
                        Err(error) => error.into_event(id.clone()),
                    };
                    let _ = task_tx.send(event).await;
                    task_active.lock().await.remove(&id);
                });
            }
            Request::Cancel(params) => {
                if !params.request_id.is_valid() {
                    send_event(
                        &event_tx,
                        Event::error(
                            Some(id),
                            "invalid_params",
                            "invalid target request id",
                            false,
                        ),
                    )
                    .await?;
                    continue;
                }
                let token = active.lock().await.get(&params.request_id).cloned();
                let cancelled = token.is_some();
                if let Some(token) = token {
                    token.cancel();
                }
                send_event(
                    &event_tx,
                    Event::success(
                        id,
                        json!({"request_id": params.request_id, "cancelled": cancelled}),
                    ),
                )
                .await?;
            }
            Request::ConnectionStatus(_params) => {
                let event = match agent.status().await {
                    Ok(status) => Event::success(id, json!(status)),
                    Err(error) => error.into_event(id),
                };
                send_event(&event_tx, event).await?;
            }
            Request::Disconnect(_params) => {
                agent.pending.lock().await.take();
                let event = match agent.forget().await {
                    Ok(status) => Event::success(id, json!(status)),
                    Err(error) => error.into_event(id),
                };
                send_event(&event_tx, event).await?;
            }
            Request::StoreKey(params) => {
                let event = match agent.store(&params.key).await {
                    Ok(status) => Event::success(id, json!(status)),
                    Err(error) => error.into_event(id),
                };
                send_event(&event_tx, event).await?;
            }
            Request::ConnectStart(_params) => {
                let Some(device) = agent.device.clone() else {
                    let reason = agent.unavailable_reason();
                    send_event(
                        &event_tx,
                        Event::error(Some(id), reason.code, reason.message, false),
                    )
                    .await?;
                    continue;
                };
                let task_agent = agent.clone();
                let task_tx = event_tx.clone();
                tasks.spawn(async move {
                    let event = match device.authorize().await {
                        Ok(authorization) => {
                            // The device code stays here; the add-on gets what a person needs.
                            let result = json!({
                                "user_code": authorization.user_code,
                                "verification_uri": authorization.verification_uri,
                                "verification_uri_complete": authorization
                                    .verification_uri_complete
                                    .clone()
                                    .unwrap_or_else(|| authorization.verification_uri.clone()),
                                "expires_in": authorization.expires_in,
                                "interval": authorization.interval,
                            });
                            *task_agent.pending.lock().await = Some(authorization);
                            Event::success(id, result)
                        }
                        Err(error) => device_error_event(id, &error),
                    };
                    let _ = task_tx.send(event).await;
                });
            }
            Request::ConnectWait(_params) => {
                let Some(device) = agent.device.clone() else {
                    let reason = agent.unavailable_reason();
                    send_event(
                        &event_tx,
                        Event::error(Some(id), reason.code, reason.message, false),
                    )
                    .await?;
                    continue;
                };
                let Some(authorization) = agent.pending.lock().await.clone() else {
                    send_event(
                        &event_tx,
                        Event::error(
                            Some(id),
                            "no_pending_connection",
                            "start connecting before waiting for approval",
                            false,
                        ),
                    )
                    .await?;
                    continue;
                };
                let token = match register(&active, &id).await {
                    Ok(token) => token,
                    Err(event) => {
                        send_event(&event_tx, event).await?;
                        continue;
                    }
                };
                let task_agent = agent.clone();
                let task_tx = event_tx.clone();
                let task_active = Arc::clone(&active);
                tasks.spawn(async move {
                    let outcome = device::wait(&authorization, &token, || {
                        device.poll(&authorization.device_code)
                    })
                    .await;
                    {
                        // Whatever happened, this code is spent unless a newer one replaced it.
                        let mut pending = task_agent.pending.lock().await;
                        if pending
                            .as_ref()
                            .is_some_and(|held| held.device_code == authorization.device_code)
                        {
                            pending.take();
                        }
                    }
                    let event = match outcome {
                        Ok(grant) => connected_event(&task_agent, id.clone(), grant).await,
                        Err(error) => device_error_event(id.clone(), &error),
                    };
                    let _ = task_tx.send(event).await;
                    task_active.lock().await.remove(&id);
                });
            }
            Request::Shutdown(_params) => {
                send_event(
                    &event_tx,
                    Event::success(id, json!({"shutting_down": true})),
                )
                .await?;
                break;
            }
        }
    }

    for token in active.lock().await.values() {
        token.cancel();
    }
    while tasks.join_next().await.is_some() {}
    drop(event_tx);
    writer_task.await.map_err(ProtocolIoError::WriterTask)??;
    Ok(())
}

/// Track a long request so `cancel` can reach it.
async fn register(
    active: &Mutex<HashMap<RequestId, CancellationToken>>,
    id: &RequestId,
) -> Result<CancellationToken, Event> {
    let mut guard = active.lock().await;
    if guard.contains_key(id) {
        return Err(Event::error(
            Some(id.clone()),
            "duplicate_request_id",
            "a request with this id is already active",
            false,
        ));
    }
    if guard.len() >= MAX_ACTIVE_PLANS {
        return Err(Event::error(
            Some(id.clone()),
            "busy",
            "too many requests are active",
            true,
        ));
    }
    let token = CancellationToken::new();
    guard.insert(id.clone(), token.clone());
    Ok(token)
}

fn device_error_event(id: RequestId, error: &DeviceError) -> Event {
    Event::error(Some(id), error.code(), error.to_string(), error.retryable())
}

/// Hand the granted key to the daemon and answer with what the add-on may show: never the key.
async fn connected_event(agent: &Agent, id: RequestId, grant: Grant) -> Event {
    let Grant {
        api_key,
        api_key_name,
        account_name,
    } = grant;
    match agent.store(&api_key).await {
        Ok(status) => {
            let mut result = json!(status);
            result["key_name"] = json!(api_key_name);
            result["account_name"] = json!(account_name);
            Event::success(id, result)
        }
        Err(AgentError::InvalidKey) => Event::error(
            Some(id),
            "malformed_response",
            "Redrob Console issued a key this version cannot store",
            false,
        ),
        Err(error) => error.into_event(id),
    }
}

fn decode_request(method: &str, value: Value) -> Result<Request, RequestDecodeError> {
    fn decode_params<T: for<'de> Deserialize<'de>>(value: Value) -> Result<T, RequestDecodeError> {
        serde_json::from_value(value).map_err(|_| RequestDecodeError {
            code: "invalid_params",
            message: "parameters do not match the method schema",
        })
    }

    match method {
        "health" => decode_params(value).map(Request::Health),
        "plan" => decode_params(value).map(Request::Plan),
        "cancel" => decode_params(value).map(Request::Cancel),
        "shutdown" => decode_params(value).map(Request::Shutdown),
        "connection_status" => decode_params(value).map(Request::ConnectionStatus),
        "connect_start" => decode_params(value).map(Request::ConnectStart),
        "connect_wait" => decode_params(value).map(Request::ConnectWait),
        "disconnect" => decode_params(value).map(Request::Disconnect),
        "store_key" => decode_params(value).map(Request::StoreKey),
        _ => Err(RequestDecodeError {
            code: "method_not_found",
            message: "unknown method",
        }),
    }
}

struct RequestDecodeError {
    code: &'static str,
    message: &'static str,
}

async fn send_event(sender: &mpsc::Sender<Event>, event: Event) -> Result<(), ProtocolIoError> {
    sender
        .send(event)
        .await
        .map_err(|_| ProtocolIoError::WriterClosed)
}

async fn write_events<W>(
    mut writer: W,
    mut receiver: mpsc::Receiver<Event>,
) -> Result<(), ProtocolIoError>
where
    W: AsyncWrite + Unpin,
{
    while let Some(event) = receiver.recv().await {
        let mut encoded = serde_json::to_vec(&event).map_err(ProtocolIoError::Serialize)?;
        if encoded.len() > MAX_RESPONSE_BYTES {
            let fallback = Event::error(
                event.id(),
                "response_too_large",
                "response exceeds 256 KiB",
                false,
            );
            encoded = serde_json::to_vec(&fallback).map_err(ProtocolIoError::Serialize)?;
        }
        encoded.push(b'\n');
        writer
            .write_all(&encoded)
            .await
            .map_err(ProtocolIoError::Write)?;
        writer.flush().await.map_err(ProtocolIoError::Write)?;
    }
    Ok(())
}

#[derive(Debug, Error)]
enum AgentError {
    #[error("request was cancelled")]
    Cancelled,
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Unavailable(UnavailableReason),
    /// No daemon: not installed, or it would not start.
    #[error("{0}")]
    Engine(#[from] EngineError),
    /// The daemon is there but this exchange with it failed.
    #[error("{0}")]
    Daemon(DaemonError),
    #[error("a key is 1 to 512 visible characters, with no spaces")]
    InvalidKey,
    #[error("Connect Redrob Console to create plans")]
    MissingKey,
}

impl std::fmt::Display for UnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl AgentError {
    fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::Api(error) => error.code(),
            Self::Unavailable(reason) => reason.code,
            Self::Engine(error) => error.code(),
            Self::Daemon(error) => error.code(),
            Self::InvalidKey => "invalid_params",
            Self::MissingKey => "missing_api_key",
        }
    }

    fn retryable(&self) -> bool {
        match self {
            Self::Api(error) => error.retryable(),
            Self::Engine(error) => error.retryable(),
            Self::Daemon(error) => error.stale(),
            _ => false,
        }
    }

    fn into_event(self, id: RequestId) -> Event {
        let (code, retryable) = (self.code(), self.retryable());
        Event::error(Some(id), code, self.to_string(), retryable)
    }
}

#[derive(Debug, Error)]
pub enum ProtocolIoError {
    #[error("failed to read protocol input: {0}")]
    Read(io::Error),
    #[error("failed to write protocol output: {0}")]
    Write(io::Error),
    #[error("failed to serialize protocol event: {0}")]
    Serialize(serde_json::Error),
    #[error("protocol writer closed unexpectedly")]
    WriterClosed,
    #[error("protocol writer task failed: {0}")]
    WriterTask(tokio::task::JoinError),
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)] // each test owns its runtime; the guard spans the test
mod tests {
    use super::*;
    use crate::config::{Config, DEFAULT_DEVICE_BASE_URL, DEFAULT_MODEL};
    use crate::credentials::{MemoryStore, SecretStore};
    use crate::engine::fake::{ok, Daemon, ENV_LOCK};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const KEY: &str = "rdb_test_0123456789abcdef";

    fn test_agent() -> Agent {
        agent_with(MemoryStore::default())
    }

    fn agent_with(legacy: MemoryStore) -> Agent {
        let config =
            Config::new(DEFAULT_DEVICE_BASE_URL, DEFAULT_MODEL.into()).expect("defaults are valid");
        Agent::new(
            RedrobClient::new(config.clone()).expect("client"),
            DeviceClient::new(config.device_base).expect("device client"),
            Engine::new("blender-test".into()),
            Credentials::new(Arc::new(legacy)),
        )
    }

    /// No engine anywhere: an empty run dir and a binary path that does not exist. Holds the
    /// environment lock for the test's life, since the environment is one per process.
    fn no_engine(name: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "redrob-shape-agent-noengine-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("REDROB_RUN_DIR", &dir);
        std::env::set_var("REDROB_CODE_BIN", dir.join("missing-redrob"));
        (guard, dir)
    }

    /// A running daemon that the engine client will find (an installed stub beside it).
    #[cfg(unix)]
    async fn with_daemon(name: &str) -> (std::sync::MutexGuard<'static, ()>, Daemon) {
        let guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start(name).await;
        daemon.register("secret");
        let stub = crate::engine::fake::stub_binary(&daemon.run_dir, None);
        std::env::set_var("REDROB_RUN_DIR", &daemon.run_dir);
        std::env::set_var("REDROB_CODE_BIN", stub);
        (guard, daemon)
    }

    fn signed_out() -> Value {
        json!({ "all": [{ "id": "redrob", "source": "custom", "options": { "apiKey": "public" } }] })
    }

    fn signed_in(key: &str) -> Value {
        json!({ "all": [{ "id": "redrob", "source": "api", "key": key }] })
    }

    async fn exchange(input: &str) -> Vec<Value> {
        exchange_with(input, test_agent()).await
    }

    /// Like `exchange_with`, but keeps the input open until `expected` events have arrived: a
    /// plan is work in a task, and end of input cancels work in flight, as it does in Blender.
    async fn exchange_until(input: &str, expected: usize, agent: Agent) -> Vec<Value> {
        use tokio::io::AsyncBufReadExt;
        let (mut request_writer, request_reader) = tokio::io::duplex(128 * 1024);
        let (response_writer, response_reader) = tokio::io::duplex(128 * 1024);
        let input = input.to_owned();
        let server =
            tokio::spawn(async move { serve(request_reader, response_writer, agent).await });
        request_writer
            .write_all(input.as_bytes())
            .await
            .expect("request write should succeed");
        let mut lines = tokio::io::BufReader::new(response_reader).lines();
        let mut events = Vec::new();
        while events.len() < expected {
            let line = tokio::time::timeout(std::time::Duration::from_secs(10), lines.next_line())
                .await
                .expect("an event within 10 s")
                .expect("response read should succeed")
                .expect("the agent is still answering");
            events.push(serde_json::from_str(&line).expect("event should be JSON"));
        }
        request_writer
            .shutdown()
            .await
            .expect("shutdown should succeed");
        server
            .await
            .expect("server task should complete")
            .expect("protocol should succeed");
        events
    }

    async fn exchange_with(input: &str, agent: Agent) -> Vec<Value> {
        let (mut request_writer, request_reader) = tokio::io::duplex(128 * 1024);
        let (response_writer, mut response_reader) = tokio::io::duplex(128 * 1024);
        let input = input.to_owned();
        let server =
            tokio::spawn(async move { serve(request_reader, response_writer, agent).await });
        request_writer
            .write_all(input.as_bytes())
            .await
            .expect("request write should succeed");
        request_writer
            .shutdown()
            .await
            .expect("shutdown should succeed");
        let mut output = String::new();
        response_reader
            .read_to_string(&mut output)
            .await
            .expect("response read should succeed");
        server
            .await
            .expect("server task should complete")
            .expect("protocol should succeed");
        output
            .lines()
            .map(|line| serde_json::from_str(line).expect("event should be JSON"))
            .collect()
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn health_and_shutdown_are_versioned_structured_events() {
        let (_env, dir) = no_engine("health");
        let events = exchange(
            "{\"version\":1,\"id\":\"health-1\",\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"shutdown\",\"params\":{}}\n",
        )
        .await;
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["version"], 1);
        assert_eq!(events[0]["id"], "health-1");
        assert_eq!(events[0]["event"], "success");
        assert_eq!(events[0]["result"]["configured"], false);
        assert_eq!(events[1]["event"], "success");
        assert_eq!(events[1]["result"]["shutting_down"], true);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn rejects_unknown_versions_methods_fields_and_params() {
        let (_env, dir) = no_engine("rejects");
        let events = exchange(
            "{\"version\":2,\"id\":1,\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"execute\",\"params\":{}}\n\
             {\"version\":1,\"id\":3,\"method\":\"health\",\"params\":{\"extra\":true}}\n\
             {\"version\":1,\"id\":4,\"method\":\"health\",\"params\":{},\"extra\":true}\n",
        )
        .await;
        let _ = std::fs::remove_dir_all(dir);
        let codes: Vec<&str> = events
            .iter()
            .map(|event| event["error"]["code"].as_str().expect("error code"))
            .collect();
        assert_eq!(
            codes,
            [
                "unsupported_version",
                "method_not_found",
                "invalid_params",
                "invalid_request"
            ]
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_redrob_code_health_and_plan_say_to_install_it() {
        let (_env, dir) = no_engine("missing");
        let events = exchange(
            "{\"version\":1,\"id\":1,\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":\"plan-1\",\"method\":\"plan\",\"params\":{\"instruction\":\"Add a cube\"}}\n\
             {\"version\":1,\"id\":3,\"method\":\"connection_status\",\"params\":{}}\n",
        )
        .await;
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(events.len(), 3);
        let health = &events[0]["result"];
        assert_eq!(health["configured"], false);
        assert_eq!(health["reason"], "engine_not_installed");
        assert_eq!(health["connection"]["connected"], false);
        assert_eq!(health["engine"]["available"], false);
        assert_eq!(health["engine"]["code"], "engine_not_installed");
        assert_eq!(
            health["engine"]["message"],
            "Redrob Code is not installed. Install it with Redrob Launcher."
        );
        assert_eq!(health["engine"]["install_url"], INSTALL_URL);
        let plan = events.iter().find(|e| e["id"] == "plan-1").unwrap();
        assert_eq!(plan["error"]["code"], "engine_not_installed");
        assert_eq!(plan["error"]["retryable"], false);
        assert!(plan["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Redrob Launcher"));
        let status = events.iter().find(|e| e["id"] == 3).unwrap();
        assert_eq!(status["error"]["code"], "engine_not_installed");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plan_without_a_key_in_the_daemon_fails_without_a_chat_request() {
        let (_env, daemon) = with_daemon("plan-no-key").await;
        daemon.provider(ok(signed_out()));
        let events = exchange_until(
            "{\"version\":1,\"id\":\"plan-1\",\"method\":\"plan\",\"params\":{\"instruction\":\"Add a cube\"}}\n",
            1,
            test_agent(),
        )
        .await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["error"]["code"], "missing_api_key");
        assert_eq!(events[0]["error"]["retryable"], false);
        assert!(daemon.chats().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plan_request_carries_a_checked_history() {
        let (_env, daemon) = with_daemon("plan-history").await;
        daemon.provider(ok(signed_out()));
        // A well-formed history gets as far as the key check, like a first request.
        let events = exchange_until(
            "{\"version\":1,\"id\":\"plan-2\",\"method\":\"plan\",\"params\":{\"instruction\":\"Rounder\",\"history\":[{\"instruction\":\"Add a cube\",\"summary\":\"A cube\",\"outcome\":\"applied\"}]}}\n",
            1,
            test_agent(),
        )
        .await;
        assert_eq!(events[0]["error"]["code"], "missing_api_key");

        // A plan riding on a turn that is not pending is refused before anything else.
        let events = exchange(
            "{\"version\":1,\"id\":\"plan-3\",\"method\":\"plan\",\"params\":{\"instruction\":\"Rounder\",\"history\":[{\"instruction\":\"Add a cube\",\"summary\":\"A cube\",\"outcome\":\"applied\",\"plan\":{\"summary\":\"x\",\"actions\":[]}}]}}\n",
        )
        .await;
        assert_eq!(events[0]["error"]["code"], "invalid_params");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plan_goes_through_the_daemon_and_comes_back_validated() {
        let (_env, daemon) = with_daemon("plan-ok").await;
        daemon.provider(ok(signed_in(KEY)));
        daemon.chat(ok(json!({
            "choices": [{ "message": { "content":
                r#"{"summary":"A cube","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","name":"Cube"}}]}"# } }]
        })));
        let events = exchange_until(
            "{\"version\":1,\"id\":\"plan-1\",\"method\":\"plan\",\"params\":{\"instruction\":\"Add a cube\"}}\n",
            1,
            test_agent(),
        )
        .await;
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0]["event"], "success", "{events:?}");
        assert_eq!(events[0]["result"]["summary"], "A cube");
        let chats = daemon.chats();
        assert_eq!(chats.len(), 1);
        assert_eq!(chats[0].header("x-redrob-app"), Some("shape"));
        assert_eq!(chats[0].header("x-redrob-session"), Some("blender-test"));
        assert!(chats[0]
            .header("authorization")
            .unwrap()
            .starts_with("Basic "));
        for event in &events {
            assert!(!event.to_string().contains(KEY), "{event}");
        }
    }

    #[test]
    fn request_id_validation_is_bounded() {
        assert!(RequestId::String("x".repeat(128)).is_valid());
        assert!(!RequestId::String(String::new()).is_valid());
        assert!(!RequestId::String("x".repeat(129)).is_valid());
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn health_without_a_key_names_the_reason_and_the_engine() {
        let (_env, daemon) = with_daemon("health-no-key").await;
        daemon.provider(ok(signed_out()));
        let events =
            exchange("{\"version\":1,\"id\":1,\"method\":\"health\",\"params\":{}}\n").await;
        let health = &events[0]["result"];
        assert_eq!(health["configured"], false);
        assert_eq!(health["reason"], "missing_api_key");
        assert_eq!(health["connection"]["connected"], false);
        assert_eq!(health["connection"]["engine_version"], "0.6.3");
        assert_eq!(health["engine"]["available"], true);
        assert_eq!(health["engine"]["version"], "0.6.3");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_pasted_key_goes_to_the_daemon_disconnect_forgets_and_no_answer_carries_the_key() {
        let (_env, daemon) = with_daemon("store").await;
        daemon.auth(ok(json!(true)));
        // The daemon's view after each step, in order.
        daemon.provider(ok(signed_in(KEY)));
        daemon.provider(ok(signed_in(KEY)));
        daemon.provider(ok(signed_in(KEY)));
        daemon.provider(ok(signed_out()));
        let input = format!(
            "{{\"version\":1,\"id\":1,\"method\":\"store_key\",\"params\":{{\"key\":\"{KEY}\"}}}}\n\
             {{\"version\":1,\"id\":2,\"method\":\"connection_status\",\"params\":{{}}}}\n\
             {{\"version\":1,\"id\":3,\"method\":\"health\",\"params\":{{}}}}\n\
             {{\"version\":1,\"id\":4,\"method\":\"disconnect\",\"params\":{{}}}}\n\
             {{\"version\":1,\"id\":5,\"method\":\"connection_status\",\"params\":{{}}}}\n"
        );
        let events = exchange(&input).await;
        assert_eq!(events.len(), 5, "{events:?}");
        assert_eq!(events[0]["result"]["connected"], true, "{events:?}");
        assert_eq!(events[0]["result"]["source"], "engine");
        assert_eq!(events[0]["result"]["provider_source"], "api");
        assert_eq!(events[1]["result"]["key_hint"], "cdef");
        assert_eq!(events[2]["result"]["configured"], true);
        assert_eq!(events[3]["result"]["connected"], false);
        assert_eq!(events[4]["result"]["connected"], false);
        for event in &events {
            assert!(!event.to_string().contains(KEY), "{event}");
        }
        let auth: Vec<_> = daemon
            .seen()
            .into_iter()
            .filter(|s| s.path == "/auth/redrob")
            .collect();
        assert_eq!(auth.len(), 2);
        assert_eq!(auth[0].method, "PUT");
        assert!(auth[0].body.contains(KEY));
        assert_eq!(auth[1].method, "DELETE");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_key_that_cannot_be_a_key_is_refused_without_echoing_it_or_asking_the_daemon() {
        let (_env, daemon) = with_daemon("bad-key").await;
        let events = exchange(
            "{\"version\":1,\"id\":1,\"method\":\"store_key\",\"params\":{\"key\":\"rdb with space\"}}\n",
        )
        .await;
        assert_eq!(events[0]["error"]["code"], "invalid_params");
        assert!(!events[0].to_string().contains("rdb with space"));
        assert!(daemon.seen().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn waiting_without_a_started_connection_is_refused() {
        let events =
            exchange("{\"version\":1,\"id\":1,\"method\":\"connect_wait\",\"params\":{}}\n").await;
        assert_eq!(events[0]["error"]["code"], "no_pending_connection");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_key_an_earlier_version_kept_in_the_keychain_moves_into_the_daemon_on_first_contact()
    {
        let (_env, daemon) = with_daemon("migrate").await;
        daemon.auth(ok(json!(true)));
        daemon.provider(ok(signed_in(KEY)));
        let keychain = Arc::new(MemoryStore::holding(KEY));
        let config = Config::new(DEFAULT_DEVICE_BASE_URL, DEFAULT_MODEL.into()).unwrap();
        let agent = Agent::new(
            RedrobClient::new(config.clone()).unwrap(),
            DeviceClient::new(config.device_base).unwrap(),
            Engine::new("blender-test".into()),
            Credentials::new(Arc::clone(&keychain) as Arc<dyn SecretStore>),
        );
        let events = exchange_with(
            "{\"version\":1,\"id\":1,\"method\":\"connection_status\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"connection_status\",\"params\":{}}\n",
            agent,
        )
        .await;
        assert_eq!(events[0]["result"]["connected"], true, "{events:?}");
        assert_eq!(events[0]["result"]["source"], "engine");
        assert_eq!(keychain.get().unwrap(), None, "the keychain copy is gone");
        let puts = daemon
            .seen()
            .into_iter()
            .filter(|s| s.method == "PUT")
            .count();
        assert_eq!(puts, 1, "moved once, not per request");
        for event in &events {
            assert!(!event.to_string().contains(KEY), "{event}");
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_that_restarted_is_found_again_for_the_connection_status() {
        let (_env, daemon) = with_daemon("restart").await;
        daemon.provider(ok(signed_in(KEY)));
        let agent = test_agent();
        let first = exchange_with(
            "{\"version\":1,\"id\":1,\"method\":\"connection_status\",\"params\":{}}\n",
            agent.clone(),
        )
        .await;
        assert_eq!(first[0]["result"]["connected"], true, "{first:?}");
        daemon.register("rotated");
        let second = exchange_with(
            "{\"version\":1,\"id\":2,\"method\":\"connection_status\",\"params\":{}}\n",
            agent,
        )
        .await;
        assert_eq!(second[0]["result"]["connected"], true, "{second:?}");
        let providers: Vec<_> = daemon
            .seen()
            .into_iter()
            .filter(|s| s.path == "/provider")
            .collect();
        assert_eq!(
            providers.len(),
            3,
            "first, refused, repeated with the new secret"
        );
        assert_eq!(
            providers[2].header("authorization"),
            Some(redrob_engine_client::authorization("rotated").as_str())
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_agent_with_unusable_configuration_still_answers_health_with_the_reason() {
        let (_env, dir) = no_engine("unusable");
        let agent = Agent::unavailable(
            "invalid_configuration",
            "REDROB_MODEL must be between 1 and 128 bytes".into(),
            Engine::new("blender-test".into()),
            Credentials::new(Arc::new(MemoryStore::default())),
        );
        let events = exchange_with(
            "{\"version\":1,\"id\":1,\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"plan\",\"params\":{\"instruction\":\"Add a cube\"}}\n",
            agent,
        )
        .await;
        let _ = std::fs::remove_dir_all(dir);
        assert_eq!(events[0]["result"]["configured"], false);
        assert_eq!(events[0]["result"]["reason"], "invalid_configuration");
        let plan = events.iter().find(|e| e["id"] == 2).unwrap();
        assert_eq!(plan["error"]["code"], "invalid_configuration");
    }
}
