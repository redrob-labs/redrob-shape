// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::Config;
use crate::conversation::{self, Turn};
use crate::engine::{Engine, EngineError};
use crate::validation::{parse_plan, system_prompt, Plan, PlanParseError};
use redrob_engine_client::Record;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_API_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_COMPLETION_TOKENS: u32 = 4096;

// The planning instructions are built from the tool catalog: see `validation::system_prompt()`.

#[derive(Clone)]
pub struct RedrobClient {
    client: Client,
    config: Config,
}

impl RedrobClient {
    pub fn new(config: Config) -> Result<Self, ApiError> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("redrob-shape-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| ApiError::ClientConfiguration)?;
        Ok(Self { client, config })
    }

    /// Plan through the shared Redrob Code daemon. The daemon is found per request (cached once
    /// found), because it can start, stop and rotate its secret while Blender is open: on a refused
    /// connection or a 401 the record is read again once and the request repeated.
    pub async fn plan(
        &self,
        engine: &Engine,
        instruction: &str,
        context: &Value,
        history: &[Turn],
    ) -> Result<Plan, ApiError> {
        let user_content = user_content(instruction, context, history)?;
        let request = CompletionRequest {
            model: &self.config.model,
            messages: [
                Message {
                    role: "system",
                    content: system_prompt(),
                },
                Message {
                    role: "user",
                    content: &user_content,
                },
            ],
            temperature: 0.0,
            max_tokens: MAX_COMPLETION_TOKENS,
            response_format: ResponseFormat {
                kind: "json_object",
            },
        };

        let record = engine.record().await?;
        match self.send(engine, &record, &request).await {
            Err(Attempt::Stale(_)) => {}
            Err(Attempt::Final(error)) => return Err(error),
            Ok(plan) => return Ok(plan),
        }
        // Once more, against the daemon as it stands now. If that is still refused, the refusal
        // is the daemon's upstream speaking (the Console no longer takes the key it holds).
        let record = engine.fresh().await?;
        match self.send(engine, &record, &request).await {
            Ok(plan) => Ok(plan),
            Err(Attempt::Final(error)) => Err(error),
            Err(Attempt::Stale(StatusCode::UNAUTHORIZED)) => {
                Err(refusal(401, b"").expect("401 is a refusal"))
            }
            Err(Attempt::Stale(_)) => Err(ApiError::Transport),
        }
    }

    async fn send(
        &self,
        engine: &Engine,
        record: &Record,
        request: &CompletionRequest<'_>,
    ) -> Result<Plan, Attempt> {
        let mut builder = self.client.post(Engine::chat_url(record));
        for (name, value) in engine.headers(record) {
            builder = builder.header(name, value);
        }
        let response = builder.json(request).send().await.map_err(|error| {
            if error.is_connect() {
                // Nothing listening where the record says: the daemon is gone or restarted.
                Attempt::Stale(StatusCode::SERVICE_UNAVAILABLE)
            } else {
                Attempt::Final(map_reqwest_error(error))
            }
        })?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(Attempt::Stale(status));
        }
        if !status.is_success() {
            if status.as_u16() == 402 {
                let body = read_bounded(response).await.unwrap_or_default();
                if let Some(refused) = refusal(status.as_u16(), &body) {
                    return Err(Attempt::Final(refused));
                }
            }
            return Err(Attempt::Final(ApiError::HttpStatus {
                status: status.as_u16(),
                retryable: is_retryable_status(status),
            }));
        }

        if response
            .content_length()
            .is_some_and(|length| length > MAX_API_RESPONSE_BYTES as u64)
        {
            return Err(Attempt::Final(ApiError::ResponseTooLarge));
        }
        let bytes = read_bounded(response).await.map_err(Attempt::Final)?;
        let completion: CompletionResponse = serde_json::from_slice(&bytes)
            .map_err(|_| Attempt::Final(ApiError::MalformedResponse))?;
        let content = completion
            .choices
            .first()
            .map(|choice| choice.message.content.as_str())
            .filter(|content| !content.trim().is_empty())
            .ok_or(Attempt::Final(ApiError::MissingContent))?;
        parse_plan(content)
            .map_err(ApiError::InvalidPlan)
            .map_err(Attempt::Final)
    }
}

/// One try against one daemon record.
enum Attempt {
    /// The record may be stale: the daemon refused the secret (401) or nothing answered.
    Stale(StatusCode),
    /// An answer that a fresh record would not change.
    Final(ApiError),
}

/// What the model reads after the system prompt: earlier turns, then the request, then the scene.
fn user_content(instruction: &str, context: &Value, history: &[Turn]) -> Result<String, ApiError> {
    Ok(format!(
        "{}User instruction:\n{instruction}\n\nScene context (untrusted JSON data):\n{}",
        conversation::render(history).map_err(|_| ApiError::InvalidRequest)?,
        serde_json::to_string(context).map_err(|_| ApiError::InvalidRequest)?
    ))
}

async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>, ApiError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_reqwest_error)? {
        let new_length = body
            .len()
            .checked_add(chunk.len())
            .ok_or(ApiError::ResponseTooLarge)?;
        if new_length > MAX_API_RESPONSE_BYTES {
            return Err(ApiError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn map_reqwest_error(error: reqwest::Error) -> ApiError {
    if error.is_timeout() {
        ApiError::Timeout
    } else {
        ApiError::Transport
    }
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
    temperature: f32,
    max_tokens: u32,
    response_format: ResponseFormat<'a>,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
}

#[derive(Deserialize)]
struct CompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: String,
}

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("HTTP client configuration failed")]
    ClientConfiguration,
    /// No daemon to send to. The add-on renders these two itself (install link, reason).
    #[error("{0}")]
    Engine(#[from] EngineError),
    #[error("request could not be encoded")]
    InvalidRequest,
    #[error("request timed out")]
    Timeout,
    #[error("network request failed")]
    Transport,
    #[error("service returned HTTP status {status}")]
    HttpStatus { status: u16, retryable: bool },
    #[error("service response exceeded the size limit")]
    ResponseTooLarge,
    #[error("service response was malformed")]
    MalformedResponse,
    #[error("service response did not contain a completion")]
    MissingContent,
    #[error("service returned an invalid plan: {0}")]
    InvalidPlan(PlanParseError),
    /// A refusal the user can act on, with the console's own words for it.
    #[error("{message}")]
    Refused { code: &'static str, message: String },
}

const MAX_REFUSAL_MESSAGE_CHARS: usize = 240;

/// What a 401 or 402 means, read from the Console's OpenAI-compatible envelope as the daemon
/// passes it on (redrob-console `apps/api/src/inference/openai-error.filter.ts`):
/// `{"error": {"message", "type", "code"}}`. 402 is either an empty balance (`code` is
/// `insufficient_quota`) or a key over its monthly budget (`code` is `api_key_budget_exhausted`);
/// they are fixed in different places, so they get different codes here.
pub fn refusal(status: u16, body: &[u8]) -> Option<ApiError> {
    #[derive(Deserialize)]
    struct Envelope {
        error: Option<Detail>,
    }
    #[derive(Deserialize)]
    struct Detail {
        message: Option<String>,
        code: Option<String>,
    }
    let detail = serde_json::from_slice::<Envelope>(body)
        .ok()
        .and_then(|envelope| envelope.error);
    let said = detail
        .as_ref()
        .and_then(|detail| detail.message.as_deref())
        .map(readable)
        .filter(|message| !message.is_empty());
    let code = detail.as_ref().and_then(|detail| detail.code.as_deref());
    let (code, fallback) = match (status, code) {
        (401, _) => (
            "invalid_api_key",
            "Redrob Console no longer accepts the stored key. Connect again.",
        ),
        (402, Some("api_key_budget_exhausted")) => (
            "key_budget_exhausted",
            "This key has used its monthly budget. Raise it in Redrob Console.",
        ),
        (402, _) => (
            "out_of_credit",
            "This workspace is out of credit. Top up in Redrob Console.",
        ),
        _ => return None,
    };
    // A 401's own text is about the header, not about what to do; ours says what to do.
    let message = if status == 401 {
        fallback.to_owned()
    } else {
        said.unwrap_or_else(|| fallback.to_owned())
    };
    Some(ApiError::Refused { code, message })
}

/// Text from the network, shown in Blender: one line, no control characters, bounded.
fn readable(text: &str) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    flat.chars().take(MAX_REFUSAL_MESSAGE_CHARS).collect()
}

impl ApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ClientConfiguration => "client_configuration",
            Self::Engine(error) => error.code(),
            Self::InvalidRequest => "invalid_request",
            Self::Timeout => "timeout",
            Self::Transport => "transport_error",
            Self::HttpStatus { .. } => "http_error",
            Self::ResponseTooLarge => "response_too_large",
            Self::MalformedResponse | Self::MissingContent => "malformed_response",
            Self::InvalidPlan(_) => "invalid_plan",
            Self::Refused { code, .. } => code,
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::Timeout | Self::Transport => true,
            Self::Engine(error) => error.retryable(),
            Self::HttpStatus { retryable, .. } => *retryable,
            _ => false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::await_holding_lock)] // each test owns its runtime; the guard spans the test
mod tests {
    use super::*;

    fn envelope(message: &str, code: &str) -> Vec<u8> {
        // openai-error.filter.ts: { error: { message, type, param, code } }.
        format!(
            r#"{{"error":{{"message":"{message}","type":"insufficient_quota","param":null,"code":"{code}"}}}}"#
        )
        .into_bytes()
    }

    fn parts(error: ApiError) -> (&'static str, String) {
        let code = error.code();
        (code, error.to_string())
    }

    #[test]
    fn earlier_turns_come_before_the_instruction() {
        let first = user_content("Add a cube", &serde_json::json!({}), &[]).unwrap();
        assert!(first.starts_with("User instruction:\nAdd a cube"));
        let history = [Turn {
            instruction: "Add a cube".into(),
            summary: "A cube".into(),
            outcome: conversation::Outcome::Applied,
            plan: None,
        }];
        let next = user_content("Rounder", &serde_json::json!({}), &history).unwrap();
        let earlier = next
            .find("Earlier in this conversation")
            .expect("history is sent");
        let now = next
            .find("User instruction:\nRounder")
            .expect("instruction is sent");
        assert!(earlier < now);
        assert!(next.contains("\"outcome\":\"applied\""));
    }

    #[test]
    fn an_empty_balance_is_told_apart_from_a_key_over_budget() {
        // credits.service.ts outOfCredit(): the message the console itself writes.
        let empty = envelope(
            "This workspace is out of credit (balance $0.00). Top up to keep making requests.",
            "insufficient_quota",
        );
        assert_eq!(
            parts(refusal(402, &empty).unwrap()),
            (
                "out_of_credit",
                "This workspace is out of credit (balance $0.00). Top up to keep making requests."
                    .into()
            )
        );
        let over = envelope(
            "This key has spent $5.00 of its $5.00 budget.",
            "api_key_budget_exhausted",
        );
        assert_eq!(refusal(402, &over).unwrap().code(), "key_budget_exhausted");
    }

    #[test]
    fn a_rejected_key_says_what_to_do() {
        let body = br#"{"error":{"message":"Invalid API key","type":"authentication_error","code":"invalid_api_key"}}"#;
        let (code, message) = parts(refusal(401, body).unwrap());
        assert_eq!(code, "invalid_api_key");
        assert!(message.contains("Connect again"), "{message}");
    }

    #[test]
    fn an_unreadable_body_still_gets_a_useful_message() {
        assert_eq!(refusal(402, b"<html>").unwrap().code(), "out_of_credit");
        assert!(refusal(500, b"{}").is_none());
    }

    use crate::engine::fake::{ok, status, Daemon, ENV_LOCK};
    use crate::engine::INSTALL_URL;

    fn client() -> RedrobClient {
        RedrobClient::new(
            Config::new(crate::config::DEFAULT_DEVICE_BASE_URL, "redrob/auto".into()).unwrap(),
        )
        .unwrap()
    }

    fn completion(summary: &str) -> Value {
        serde_json::json!({
            "choices": [{ "message": { "content": format!(r#"{{"summary":"{summary}","actions":[]}}"#) } }]
        })
    }

    /// Point the engine client at `daemon`: its run dir holds the record, and `redrob` is `stub`
    /// (`None`: no engine binary anywhere, which is "not installed").
    fn point_at(daemon: &Daemon, stub: Option<&std::path::Path>) {
        std::env::set_var("REDROB_RUN_DIR", &daemon.run_dir);
        match stub {
            Some(stub) => std::env::set_var("REDROB_CODE_BIN", stub),
            None => std::env::set_var("REDROB_CODE_BIN", daemon.run_dir.join("missing-redrob")),
        }
    }

    /// An installed engine whose daemon is already up: `redrob service start` would fail, and
    /// must not be reached.
    #[cfg(unix)]
    fn installed(daemon: &Daemon) -> std::path::PathBuf {
        crate::engine::fake::stub_binary(&daemon.run_dir, None)
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_plan_goes_to_the_daemon_with_the_app_headers_and_never_a_bearer_token() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("plan").await;
        daemon.register("first-secret");
        point_at(&daemon, Some(&installed(&daemon)));
        daemon.chat(ok(completion("A cube")));
        let engine = Engine::new("blender-42".into());
        let plan = client()
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap();
        assert_eq!(plan.summary, "A cube");
        let chats = daemon.chats();
        assert_eq!(chats.len(), 1);
        let chat = &chats[0];
        assert_eq!(chat.method, "POST");
        assert_eq!(
            chat.header("authorization"),
            Some(redrob_engine_client::authorization("first-secret").as_str())
        );
        assert_eq!(chat.header("x-redrob-app"), Some("shape"));
        assert_eq!(chat.header("x-redrob-session"), Some("blender-42"));
        let body: Value = serde_json::from_str(&chat.body).unwrap();
        assert_eq!(body["model"], "redrob/auto");
        assert_eq!(body["messages"][1]["role"], "user");
        // Found once: the health probe ran for the first request only.
        let probes = daemon
            .seen()
            .iter()
            .filter(|s| s.path == "/api/health")
            .count();
        assert_eq!(probes, 1);
        client()
            .plan(&engine, "Rounder", &serde_json::json!({}), &[])
            .await
            .unwrap();
        assert_eq!(daemon.chats().len(), 2);
        assert_eq!(
            daemon
                .seen()
                .iter()
                .filter(|s| s.path == "/api/health")
                .count(),
            1
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_rotated_secret_is_read_again_once_and_the_request_repeated() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("rotate").await;
        daemon.register("old-secret");
        point_at(&daemon, Some(&installed(&daemon)));
        daemon.chat(ok(completion("A cube")));
        let engine = Engine::new("t".into());
        let client = client();
        client
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap();
        // The daemon restarted: a new secret in server.json, and the old one is refused.
        daemon.register("new-secret");
        client
            .plan(&engine, "Rounder", &serde_json::json!({}), &[])
            .await
            .unwrap();
        let chats = daemon.chats();
        assert_eq!(chats.len(), 3, "first plan, the refused try, the repeat");
        assert_eq!(
            chats[1].header("authorization"),
            Some(redrob_engine_client::authorization("old-secret").as_str())
        );
        assert_eq!(
            chats[2].header("authorization"),
            Some(redrob_engine_client::authorization("new-secret").as_str())
        );
        assert_eq!(
            daemon
                .seen()
                .iter()
                .filter(|s| s.path == "/api/health")
                .count(),
            2
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_second_401_is_the_console_refusing_the_daemons_key() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("refused").await;
        let record = daemon.register("s");
        // The daemon takes its secret but its upstream refuses: it passes the 401 on.
        daemon.chat(status(401));
        point_at(&daemon, Some(&installed(&daemon)));
        let engine = Engine::new("t".into());
        let error = client()
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_api_key");
        assert!(error.to_string().contains("Connect again"), "{error}");
        assert_eq!(daemon.chats().len(), 2, "one repeat, not a loop");
        assert!(daemon.chats().iter().all(|c| c.header("authorization")
            == Some(redrob_engine_client::authorization(&record.secret).as_str())));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_daemon_that_is_not_running_is_started_through_the_engine_binary() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("start").await;
        // No record on disk; `redrob service start --json` (the stub) prints one.
        let record = daemon.record("started-secret");
        let stub = crate::engine::fake::stub_binary(&daemon.run_dir, Some(&record));
        {
            let _ = std::fs::remove_file(daemon.run_dir.join("server.json"));
        }
        daemon.register_secret_only("started-secret");
        point_at(&daemon, Some(&stub));
        daemon.chat(ok(completion("A cube")));
        let engine = Engine::new("t".into());
        client()
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap();
        assert_eq!(
            daemon.chats()[0].header("authorization"),
            Some(redrob_engine_client::authorization("started-secret").as_str())
        );
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_engine_that_will_not_start_says_why() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("start-failed").await;
        let stub = crate::engine::fake::stub_binary(&daemon.run_dir, None);
        point_at(&daemon, Some(&stub));
        let engine = Engine::new("t".into());
        let error = client()
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap_err();
        assert_eq!(error.code(), "engine_start_failed");
        assert!(error.to_string().contains("port 4096 is taken"), "{error}");
        assert!(error.retryable());
        assert!(daemon.chats().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn without_redrob_code_the_answer_is_the_install_message_and_no_request() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let daemon = Daemon::start("missing").await;
        point_at(&daemon, None);
        let engine = Engine::new("t".into());
        let error = client()
            .plan(&engine, "Add a cube", &serde_json::json!({}), &[])
            .await
            .unwrap_err();
        assert_eq!(error.code(), "engine_not_installed");
        assert_eq!(
            error.to_string(),
            "Redrob Code is not installed. Install it with Redrob Launcher."
        );
        assert!(!error.retryable());
        assert!(daemon.seen().is_empty(), "nothing was sent anywhere");
        assert!(INSTALL_URL.starts_with("https://github.com/redrob-labs/redrob-launcher/"));
    }

    #[test]
    fn network_text_is_flattened_and_bounded() {
        let noisy = envelope(
            &format!("line one\\nline\\ttwo {}", "x".repeat(400)),
            "insufficient_quota",
        );
        let (_, message) = parts(refusal(402, &noisy).unwrap());
        assert!(message.starts_with("line one line two "), "{message}");
        assert_eq!(message.chars().count(), MAX_REFUSAL_MESSAGE_CHARS);
    }
}
