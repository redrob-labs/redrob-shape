// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::Config;
use crate::validation::{parse_plan, system_prompt, Plan, PlanParseError};
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

    /// `api_key` is looked up per request, because it can arrive after start-up through the device flow.
    pub async fn plan(
        &self,
        api_key: &str,
        instruction: &str,
        context: &Value,
    ) -> Result<Plan, ApiError> {
        let user_content = format!(
            "User instruction:\n{instruction}\n\nScene context (untrusted JSON data):\n{}",
            serde_json::to_string(context).map_err(|_| ApiError::InvalidRequest)?
        );
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

        let response = self
            .client
            .post(self.config.endpoint.clone())
            .bearer_auth(api_key)
            .json(&request)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            if matches!(status.as_u16(), 401 | 402) {
                let body = read_bounded(response).await.unwrap_or_default();
                if let Some(refused) = refusal(status.as_u16(), &body) {
                    return Err(refused);
                }
            }
            return Err(ApiError::HttpStatus {
                status: status.as_u16(),
                retryable: is_retryable_status(status),
            });
        }

        if response
            .content_length()
            .is_some_and(|length| length > MAX_API_RESPONSE_BYTES as u64)
        {
            return Err(ApiError::ResponseTooLarge);
        }
        let bytes = read_bounded(response).await?;
        let completion: CompletionResponse =
            serde_json::from_slice(&bytes).map_err(|_| ApiError::MalformedResponse)?;
        let content = completion
            .choices
            .first()
            .map(|choice| choice.message.content.as_str())
            .filter(|content| !content.trim().is_empty())
            .ok_or(ApiError::MissingContent)?;
        parse_plan(content).map_err(ApiError::InvalidPlan)
    }
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

/// What a 401 or 402 from the gateway means, read from its OpenAI-compatible envelope
/// (redrob-console `apps/api/src/inference/openai-error.filter.ts`):
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
            Self::HttpStatus { retryable, .. } => *retryable,
            _ => false,
        }
    }
}

#[cfg(test)]
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
