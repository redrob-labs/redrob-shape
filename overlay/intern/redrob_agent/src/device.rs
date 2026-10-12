// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The client half of Redrob Console's device flow (RFC 8628), for product `shape`.
//!
//! Every shape here is the producer's, read from redrob-console's own source
//! (`apps/api/src/device/device.controller.ts`, `device.service.ts`, `dto/device.dto.ts`):
//!
//! ```text
//! POST {base}/device/authorize {"product":"shape"}
//!   200 {deviceCode, userCode, verificationUri, verificationUriComplete, expiresIn, interval}
//! POST {base}/device/token {"deviceCode"}
//!   200 {apiKey, apiKeyId, apiKeyName, accountId, accountName, product, apiBaseUrl}
//!   failure body {"error": <code>, "error_description", "statusCode"}, HTTP 400 unless noted:
//!     authorization_pending, slow_down        keep polling (slow_down: back off first)
//!     access_denied (403), expired_token,
//!     invalid_grant                            stop
//!   429 from the rate limit guard              back off, like slow_down
//! ```
//!
//! The device code is a bearer for the key until it is collected, so it never leaves this process:
//! the add-on is shown the user code and the page to type it into, and nothing else.

use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;
use std::future::Future;
use std::time::Duration;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

pub const PRODUCT: &str = "shape";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 64 * 1024;
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const MAX_INTERVAL: Duration = Duration::from_secs(60);
const MAX_WAIT: Duration = Duration::from_secs(30 * 60);
/// RFC 8628 section 3.5: on `slow_down`, add five seconds to the interval for this and later polls.
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
/// A dropped Wi-Fi for a moment should not throw away a ten-minute window.
const MAX_CONSECUTIVE_TRANSPORT_FAILURES: u32 = 3;
const MAX_USER_CODE_BYTES: usize = 32;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Authorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(default)]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub api_key: String,
    #[serde(default)]
    pub api_key_name: String,
    #[serde(default)]
    pub account_name: String,
}

#[derive(Debug)]
pub enum Poll {
    Granted(Grant),
    Pending,
    SlowDown,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DeviceError {
    #[error("the connection was declined in Redrob Console")]
    Denied,
    #[error("the code expired before it was approved; connect again")]
    Expired,
    #[error("Redrob Console no longer recognises this code; connect again")]
    InvalidGrant,
    #[error("Redrob Console could not be reached")]
    Transport,
    #[error("Redrob Console did not answer in time")]
    Timeout,
    #[error("Redrob Console answered with HTTP status {0}")]
    HttpStatus(u16),
    #[error("Redrob Console sent a response this version does not understand")]
    Malformed,
    #[error("connecting was cancelled")]
    Cancelled,
}

impl DeviceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Denied => "access_denied",
            Self::Expired => "expired_token",
            Self::InvalidGrant => "invalid_grant",
            Self::Transport => "transport_error",
            Self::Timeout => "timeout",
            Self::HttpStatus(_) => "http_error",
            Self::Malformed => "malformed_response",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Timeout | Self::Expired | Self::InvalidGrant
        ) || matches!(self, Self::HttpStatus(status) if *status >= 500)
    }
}

/// What one token response means. Pure, so it is tested against the producer's shapes.
pub fn classify(status: u16, body: &[u8]) -> Result<Poll, DeviceError> {
    if status == 200 {
        let grant: Grant = serde_json::from_slice(body).map_err(|_| DeviceError::Malformed)?;
        if grant.api_key.trim().is_empty() {
            return Err(DeviceError::Malformed);
        }
        return Ok(Poll::Granted(grant));
    }
    if status == 429 {
        return Ok(Poll::SlowDown);
    }
    #[derive(Deserialize)]
    struct Failure {
        error: Option<String>,
    }
    let code = serde_json::from_slice::<Failure>(body)
        .ok()
        .and_then(|failure| failure.error);
    match code.as_deref() {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("access_denied") => Err(DeviceError::Denied),
        Some("expired_token") => Err(DeviceError::Expired),
        Some("invalid_grant") => Err(DeviceError::InvalidGrant),
        _ => Err(DeviceError::HttpStatus(status)),
    }
}

/// Refuse an authorization that would send the user somewhere other than the console we asked,
/// or that carries values no real console sends.
pub fn checked(authorization: Authorization, base: &Url) -> Result<Authorization, DeviceError> {
    let same_console = |value: &str| {
        Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https" && url.host_str().is_some() && url.host_str() == base.host_str()
        })
    };
    let user_code = authorization.user_code.trim();
    let plausible = !authorization.device_code.trim().is_empty()
        && !user_code.is_empty()
        && user_code.len() <= MAX_USER_CODE_BYTES
        && !user_code.chars().any(char::is_control)
        && same_console(&authorization.verification_uri)
        && authorization
            .verification_uri_complete
            .as_deref()
            .is_none_or(same_console)
        && authorization.expires_in > 0;
    if plausible {
        Ok(authorization)
    } else {
        Err(DeviceError::Malformed)
    }
}

/// Poll until the person decides, the code runs out, or the caller cancels.
pub async fn wait<F, Fut>(
    authorization: &Authorization,
    cancel: &CancellationToken,
    mut poll: F,
) -> Result<Grant, DeviceError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Poll, DeviceError>>,
{
    let mut interval =
        Duration::from_secs(authorization.interval).clamp(MIN_INTERVAL, MAX_INTERVAL);
    let window = Duration::from_secs(authorization.expires_in).min(MAX_WAIT);
    let deadline = tokio::time::Instant::now() + window;
    let mut transport_failures = 0;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err(DeviceError::Cancelled),
            _ = tokio::time::sleep(interval) => {}
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(DeviceError::Expired);
        }
        let outcome = tokio::select! {
            _ = cancel.cancelled() => return Err(DeviceError::Cancelled),
            outcome = poll() => outcome,
        };
        match outcome {
            Ok(Poll::Granted(grant)) => return Ok(grant),
            Ok(Poll::Pending) => transport_failures = 0,
            Ok(Poll::SlowDown) => {
                transport_failures = 0;
                interval = (interval + SLOW_DOWN_STEP).min(MAX_INTERVAL);
            }
            Err(DeviceError::Transport | DeviceError::Timeout)
                if transport_failures + 1 < MAX_CONSECUTIVE_TRANSPORT_FAILURES =>
            {
                transport_failures += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Clone)]
pub struct DeviceClient {
    client: Client,
    base: Url,
}

impl DeviceClient {
    pub fn new(base: Url) -> Result<Self, DeviceError> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("redrob-shape-agent/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| DeviceError::Transport)?;
        Ok(Self { client, base })
    }

    pub async fn authorize(&self) -> Result<Authorization, DeviceError> {
        let url = self
            .base
            .join("device/authorize")
            .map_err(|_| DeviceError::Malformed)?;
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({ "product": PRODUCT }))
            .send()
            .await
            .map_err(map_error)?;
        let status = response.status();
        let body = read_bounded(response).await?;
        if status != StatusCode::OK {
            return Err(DeviceError::HttpStatus(status.as_u16()));
        }
        let authorization: Authorization =
            serde_json::from_slice(&body).map_err(|_| DeviceError::Malformed)?;
        checked(authorization, &self.base)
    }

    pub async fn poll(&self, device_code: &str) -> Result<Poll, DeviceError> {
        let url = self
            .base
            .join("device/token")
            .map_err(|_| DeviceError::Malformed)?;
        let response = self
            .client
            .post(url)
            .json(&serde_json::json!({ "deviceCode": device_code }))
            .send()
            .await
            .map_err(map_error)?;
        let status = response.status().as_u16();
        let body = read_bounded(response).await?;
        classify(status, &body)
    }
}

async fn read_bounded(mut response: reqwest::Response) -> Result<Vec<u8>, DeviceError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_error)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(DeviceError::Malformed);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn map_error(error: reqwest::Error) -> DeviceError {
    if error.is_timeout() {
        DeviceError::Timeout
    } else {
        DeviceError::Transport
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    const BASE: &str = "https://console.redrob.ai/api/backend/v1/";

    /// The production response to `{"product":"shape"}` on 2026-10-08, codes replaced.
    fn production_authorization() -> &'static str {
        r#"{"deviceCode":"0000000000000000000000000000000000000000000000000000000000000000","userCode":"TY78-Y8VS","verificationUri":"https://console.redrob.ai/connect","verificationUriComplete":"https://console.redrob.ai/connect?code=TY78-Y8VS","expiresIn":600,"interval":5}"#
    }

    fn authorization(expires_in: u64, interval: u64) -> Authorization {
        let mut parsed: Authorization = serde_json::from_str(production_authorization()).unwrap();
        parsed.expires_in = expires_in;
        parsed.interval = interval;
        parsed
    }

    fn failure(code: &str, status: u16) -> String {
        // DeviceTokenException: super({ error: code, error_description: description, statusCode }).
        format!(
            r#"{{"error":"{code}","error_description":"from the console","statusCode":{status}}}"#
        )
    }

    #[test]
    fn the_production_authorization_is_accepted() {
        let parsed = serde_json::from_str(production_authorization()).unwrap();
        let accepted = checked(parsed, &Url::parse(BASE).unwrap()).expect("accepted");
        assert_eq!(accepted.user_code, "TY78-Y8VS");
        assert_eq!(accepted.interval, 5);
    }

    #[test]
    fn an_approval_page_on_another_host_is_refused() {
        let base = Url::parse(BASE).unwrap();
        let mut elsewhere = authorization(600, 5);
        elsewhere.verification_uri = "https://console.redrob.ai.example.com/connect".into();
        assert_eq!(
            checked(elsewhere, &base).unwrap_err(),
            DeviceError::Malformed
        );
        let mut plain = authorization(600, 5);
        plain.verification_uri_complete = Some("http://console.redrob.ai/connect?code=X".into());
        assert_eq!(checked(plain, &base).unwrap_err(), DeviceError::Malformed);
    }

    #[test]
    fn token_responses_mean_what_the_console_says_they_mean() {
        // DeviceTokenResponseDto, with the DTO's own examples and a stand-in key.
        let granted = r#"{"apiKey":"rdb_test_0123456789abcdef","apiKeyId":"key_1","apiKeyName":"Redrob Shape (2026-10-08)","accountId":"acc_1","accountName":"Acme","product":"shape","apiBaseUrl":"https://console.redrob.ai/api/backend/v1"}"#;
        match classify(200, granted.as_bytes()).unwrap() {
            Poll::Granted(grant) => {
                assert_eq!(grant.api_key, "rdb_test_0123456789abcdef");
                assert_eq!(grant.api_key_name, "Redrob Shape (2026-10-08)");
                assert_eq!(grant.account_name, "Acme");
            }
            other => panic!("expected a grant, got {other:?}"),
        }
        assert!(matches!(
            classify(400, failure("authorization_pending", 400).as_bytes()),
            Ok(Poll::Pending)
        ));
        assert!(matches!(
            classify(400, failure("slow_down", 400).as_bytes()),
            Ok(Poll::SlowDown)
        ));
        assert!(matches!(classify(429, b"{}"), Ok(Poll::SlowDown)));
        assert_eq!(
            classify(403, failure("access_denied", 403).as_bytes()).unwrap_err(),
            DeviceError::Denied
        );
        assert_eq!(
            classify(400, failure("expired_token", 400).as_bytes()).unwrap_err(),
            DeviceError::Expired
        );
        assert_eq!(
            classify(400, failure("invalid_grant", 400).as_bytes()).unwrap_err(),
            DeviceError::InvalidGrant
        );
        // Nest's own validation failure: `error` is the reason phrase, not a device code.
        let validation =
            r#"{"message":["deviceCode must be a string"],"error":"Bad Request","statusCode":400}"#;
        assert_eq!(
            classify(400, validation.as_bytes()).unwrap_err(),
            DeviceError::HttpStatus(400)
        );
        assert_eq!(
            classify(200, br#"{"apiKey":""}"#).unwrap_err(),
            DeviceError::Malformed
        );
    }

    fn scripted(
        outcomes: Vec<Result<Poll, DeviceError>>,
    ) -> RefCell<VecDeque<Result<Poll, DeviceError>>> {
        RefCell::new(outcomes.into())
    }

    fn grant() -> Poll {
        Poll::Granted(Grant {
            api_key: "rdb_test_0123456789abcdef".into(),
            api_key_name: String::new(),
            account_name: String::new(),
        })
    }

    #[tokio::test(start_paused = true)]
    async fn waits_while_pending_and_returns_the_grant() {
        let outcomes = scripted(vec![Ok(Poll::Pending), Ok(Poll::Pending), Ok(grant())]);
        let started = tokio::time::Instant::now();
        let result = wait(&authorization(600, 5), &CancellationToken::new(), || {
            let next = outcomes.borrow_mut().pop_front().expect("polled too often");
            async move { next }
        })
        .await;
        assert_eq!(result.unwrap().api_key, "rdb_test_0123456789abcdef");
        assert_eq!(started.elapsed(), Duration::from_secs(15));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_down_adds_five_seconds_to_every_later_poll() {
        let outcomes = scripted(vec![Ok(Poll::SlowDown), Ok(Poll::Pending), Ok(grant())]);
        let started = tokio::time::Instant::now();
        wait(&authorization(600, 5), &CancellationToken::new(), || {
            let next = outcomes.borrow_mut().pop_front().expect("polled too often");
            async move { next }
        })
        .await
        .unwrap();
        // 5 before the first poll, then 10 and 10.
        assert_eq!(started.elapsed(), Duration::from_secs(25));
    }

    #[tokio::test(start_paused = true)]
    async fn a_decision_stops_the_wait() {
        let outcomes = scripted(vec![Ok(Poll::Pending), Err(DeviceError::Denied)]);
        let result = wait(&authorization(600, 5), &CancellationToken::new(), || {
            let next = outcomes.borrow_mut().pop_front().expect("polled too often");
            async move { next }
        })
        .await;
        assert_eq!(result.unwrap_err(), DeviceError::Denied);
    }

    #[tokio::test(start_paused = true)]
    async fn rides_out_two_network_failures_but_not_three() {
        let recovered = scripted(vec![
            Err(DeviceError::Transport),
            Err(DeviceError::Timeout),
            Ok(grant()),
        ]);
        assert!(wait(&authorization(600, 5), &CancellationToken::new(), || {
            let next = recovered
                .borrow_mut()
                .pop_front()
                .expect("polled too often");
            async move { next }
        })
        .await
        .is_ok());

        let lost = scripted(vec![
            Err(DeviceError::Transport),
            Err(DeviceError::Transport),
            Err(DeviceError::Transport),
        ]);
        let result = wait(&authorization(600, 5), &CancellationToken::new(), || {
            let next = lost.borrow_mut().pop_front().expect("polled too often");
            async move { next }
        })
        .await;
        assert_eq!(result.unwrap_err(), DeviceError::Transport);
    }

    #[tokio::test(start_paused = true)]
    async fn stops_when_the_code_runs_out() {
        let polls = RefCell::new(0);
        let result = wait(&authorization(12, 5), &CancellationToken::new(), || {
            *polls.borrow_mut() += 1;
            async { Ok(Poll::Pending) }
        })
        .await;
        assert_eq!(result.unwrap_err(), DeviceError::Expired);
        assert_eq!(*polls.borrow(), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn cancelling_ends_the_wait_without_another_poll() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let result = wait(&authorization(600, 5), &cancel, || async {
            panic!("must not poll after cancellation")
        })
        .await;
        assert_eq!(result.unwrap_err(), DeviceError::Cancelled);
    }
}
