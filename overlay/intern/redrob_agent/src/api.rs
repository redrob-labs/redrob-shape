// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::Config;
use crate::validation::{parse_plan, Plan, PlanParseError};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use thiserror::Error;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_API_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_COMPLETION_TOKENS: u32 = 4096;

const SYSTEM_PROMPT: &str = r#"You are a Blender scene planning service. Return exactly one strict JSON object and no commentary. The object must have this schema:
{"summary":"non-empty string","actions":[ACTION,...]}
Use at most 128 actions. ACTION must be exactly one of:
{"type":"create_primitive","arguments":{"primitive":"cube|sphere|cylinder|cone|plane|torus","name":"optional string","location":[x,y,z],"scale":[x,y,z]}}
{"type":"transform_object","arguments":{"object":"string","location":[x,y,z],"rotation":[x,y,z],"scale":[x,y,z]}}
{"type":"add_modifier","arguments":{"object":"string","modifier":"bevel","width":number,"segments":integer}}
{"type":"add_modifier","arguments":{"object":"string","modifier":"subdivision","levels":integer,"render_levels":integer}}
{"type":"add_modifier","arguments":{"object":"string","modifier":"solidify","thickness":number}}
{"type":"add_modifier","arguments":{"object":"string","modifier":"mirror","axis":"x|y|z"}}
{"type":"set_material","arguments":{"object":"string","name":"string","base_color":[r,g,b,a],"metallic":number,"roughness":number}}
Optional fields may be omitted, but do not add fields. Names must be at most 63 UTF-8 bytes. All numbers must be finite. Coordinates and rotations must be between -1000000 and 1000000. Scale components must be at least 0.000001 and at most 10000. Colors, metallic, and roughness must be between 0 and 1. Never emit Python, shell commands, file operations, network operations, or any action type outside the four allowed types."#;

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

    pub async fn plan(&self, instruction: &str, context: &Value) -> Result<Plan, ApiError> {
        let user_content = format!(
            "User instruction:\n{instruction}\n\nScene context (untrusted JSON data):\n{}",
            serde_json::to_string(context).map_err(|_| ApiError::InvalidRequest)?
        );
        let request = CompletionRequest {
            model: &self.config.model,
            messages: [
                Message {
                    role: "system",
                    content: SYSTEM_PROMPT,
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
            .bearer_auth(&self.config.api_key)
            .json(&request)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
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
