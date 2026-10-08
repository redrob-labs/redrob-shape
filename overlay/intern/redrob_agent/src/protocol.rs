// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::api::{ApiError, RedrobClient};
use crate::validation::Plan;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
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

#[derive(Clone)]
pub struct Agent {
    client: Option<Arc<RedrobClient>>,
    unavailable: Option<UnavailableReason>,
}

#[derive(Clone, Debug)]
struct UnavailableReason {
    code: &'static str,
    message: String,
}

impl Agent {
    pub fn configured(client: RedrobClient) -> Self {
        Self {
            client: Some(Arc::new(client)),
            unavailable: None,
        }
    }

    pub fn unavailable(code: &'static str, message: String) -> Self {
        Self {
            client: None,
            unavailable: Some(UnavailableReason { code, message }),
        }
    }

    pub fn unconfigured_for_test() -> Self {
        Self::unavailable("missing_api_key", "REDROB_API_KEY is not configured".into())
    }

    async fn plan(&self, instruction: &str, context: &Value) -> Result<Plan, AgentError> {
        let client = self.client.as_ref().ok_or_else(|| {
            let reason = self
                .unavailable
                .clone()
                .expect("an unavailable agent must include a reason");
            AgentError::Unavailable(reason)
        })?;
        client.plan(instruction, context).await.map_err(Into::into)
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
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelParams {
    request_id: RequestId,
}

#[derive(Debug)]
enum Request {
    Health(EmptyParams),
    Plan(PlanParams),
    Cancel(CancelParams),
    Shutdown(EmptyParams),
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
                let result = if agent.client.is_some() {
                    json!({"service": "redrob-shape-agent", "configured": true})
                } else {
                    let reason = agent
                        .unavailable
                        .as_ref()
                        .expect("an unavailable agent must include a reason");
                    json!({
                        "service": "redrob-shape-agent",
                        "configured": false,
                        "reason": reason.code
                    })
                };
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
                if let Some(reason) = &agent.unavailable {
                    send_event(
                        &event_tx,
                        Event::error(Some(id), reason.code, reason.message.clone(), false),
                    )
                    .await?;
                    continue;
                }

                let token = CancellationToken::new();
                {
                    let mut guard = active.lock().await;
                    if guard.contains_key(&id) {
                        drop(guard);
                        send_event(
                            &event_tx,
                            Event::error(
                                Some(id),
                                "duplicate_request_id",
                                "a plan with this request id is already active",
                                false,
                            ),
                        )
                        .await?;
                        continue;
                    }
                    if guard.len() >= MAX_ACTIVE_PLANS {
                        drop(guard);
                        send_event(
                            &event_tx,
                            Event::error(Some(id), "busy", "too many plans are active", true),
                        )
                        .await?;
                        continue;
                    }
                    guard.insert(id.clone(), token.clone());
                }

                let task_agent = agent.clone();
                let task_tx = event_tx.clone();
                let task_active = Arc::clone(&active);
                tasks.spawn(async move {
                    let outcome = tokio::select! {
                        _ = token.cancelled() => Err(AgentError::Cancelled),
                        result = task_agent.plan(&params.instruction, &params.context) => result,
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
}

impl std::fmt::Display for UnavailableReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl AgentError {
    fn into_event(self, id: RequestId) -> Event {
        match self {
            Self::Cancelled => Event::error(Some(id), "cancelled", "request was cancelled", false),
            Self::Api(error) => {
                let code = error.code();
                let retryable = error.retryable();
                Event::error(Some(id), code, error.to_string(), retryable)
            }
            Self::Unavailable(reason) => Event::error(Some(id), reason.code, reason.message, false),
        }
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
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn exchange(input: &str) -> Vec<Value> {
        let (mut request_writer, request_reader) = tokio::io::duplex(128 * 1024);
        let (response_writer, mut response_reader) = tokio::io::duplex(128 * 1024);
        let input = input.to_owned();
        let server = tokio::spawn(async move {
            serve(
                request_reader,
                response_writer,
                Agent::unconfigured_for_test(),
            )
            .await
        });
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

    #[tokio::test]
    async fn health_and_shutdown_are_versioned_structured_events() {
        let events = exchange(
            "{\"version\":1,\"id\":\"health-1\",\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"shutdown\",\"params\":{}}\n",
        )
        .await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["version"], 1);
        assert_eq!(events[0]["id"], "health-1");
        assert_eq!(events[0]["event"], "success");
        assert_eq!(events[0]["result"]["configured"], false);
        assert_eq!(events[1]["event"], "success");
        assert_eq!(events[1]["result"]["shutting_down"], true);
    }

    #[tokio::test]
    async fn rejects_unknown_versions_methods_fields_and_params() {
        let events = exchange(
            "{\"version\":2,\"id\":1,\"method\":\"health\",\"params\":{}}\n\
             {\"version\":1,\"id\":2,\"method\":\"execute\",\"params\":{}}\n\
             {\"version\":1,\"id\":3,\"method\":\"health\",\"params\":{\"extra\":true}}\n\
             {\"version\":1,\"id\":4,\"method\":\"health\",\"params\":{},\"extra\":true}\n",
        )
        .await;
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

    #[tokio::test]
    async fn plan_without_key_fails_without_network_access() {
        let events = exchange(
            "{\"version\":1,\"id\":\"plan-1\",\"method\":\"plan\",\"params\":{\"instruction\":\"Add a cube\"}}\n",
        )
        .await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["error"]["code"], "missing_api_key");
        assert_eq!(events[0]["error"]["retryable"], false);
    }

    #[test]
    fn request_id_validation_is_bounded() {
        assert!(RequestId::String("x".repeat(128)).is_valid());
        assert!(!RequestId::String(String::new()).is_valid());
        assert!(!RequestId::String("x".repeat(129)).is_valid());
    }
}
