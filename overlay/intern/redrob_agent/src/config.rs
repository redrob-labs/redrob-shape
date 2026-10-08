// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use reqwest::Url;
use std::env;
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://console.redrob.ai/api/backend/v1/chat/completions";
/// The console's device-flow endpoints, under the same `/v1` as the completion endpoint.
pub const DEFAULT_DEVICE_BASE_URL: &str = "https://console.redrob.ai/api/backend/v1";
pub const DEFAULT_MODEL: &str = "auto";
const MAX_MODEL_LENGTH: usize = 128;

/// Where plans and connections go.
///
/// The key is deliberately not part of it. It can arrive after start-up, through the device flow,
/// so it is looked up for every request instead -- see `credentials`.
#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: Url,
    /// Always ends in `/`, so `join("device/token")` appends rather than replacing the last segment.
    pub device_base: Url,
    pub model: String,
}

#[derive(Debug, Error, Clone)]
pub enum ConfigError {
    #[error("REDROB_BASE_URL must be a valid HTTPS URL without credentials or a fragment")]
    InvalidBaseUrl,
    #[error("REDROB_DEVICE_BASE_URL must be a valid HTTPS URL without credentials or a fragment")]
    InvalidDeviceBaseUrl,
    #[error("REDROB_MODEL must be between 1 and {MAX_MODEL_LENGTH} bytes")]
    InvalidModel,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let base_url = env::var("REDROB_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let device_base = env::var("REDROB_DEVICE_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_DEVICE_BASE_URL.to_owned());
        let model = env::var("REDROB_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        Self::new(&base_url, &device_base, model)
    }

    /// `REDROB_API_KEY`, when set, wins over the keychain. It is how CI and development run without
    /// connecting, and the connection status names it as the environment so nobody mistakes it for
    /// a stored key.
    pub fn environment_key() -> Option<String> {
        env::var("REDROB_API_KEY")
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    }

    pub fn new(base_url: &str, device_base: &str, model: String) -> Result<Self, ConfigError> {
        let endpoint = https_url(base_url).ok_or(ConfigError::InvalidBaseUrl)?;
        let mut device_base = https_url(device_base).ok_or(ConfigError::InvalidDeviceBaseUrl)?;
        if !device_base.path().ends_with('/') {
            let path = format!("{}/", device_base.path());
            device_base.set_path(&path);
        }
        if model.is_empty() || model.len() > MAX_MODEL_LENGTH || model.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidModel);
        }
        Ok(Self {
            endpoint,
            device_base,
            model,
        })
    }
}

fn https_url(value: &str) -> Option<Url> {
    let url = Url::parse(value).ok()?;
    let acceptable = url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none();
    acceptable.then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_or_credentialed_urls() {
        assert!(matches!(
            Config::new(
                "http://example.com/v1",
                DEFAULT_DEVICE_BASE_URL,
                "auto".into()
            ),
            Err(ConfigError::InvalidBaseUrl)
        ));
        assert!(matches!(
            Config::new(
                "https://user:pass@example.com/v1",
                DEFAULT_DEVICE_BASE_URL,
                "auto".into()
            ),
            Err(ConfigError::InvalidBaseUrl)
        ));
        assert!(matches!(
            Config::new(
                DEFAULT_BASE_URL,
                "http://console.redrob.ai/api/backend/v1",
                "auto".into()
            ),
            Err(ConfigError::InvalidDeviceBaseUrl)
        ));
    }

    #[test]
    fn device_endpoints_are_appended_under_v1() {
        let config = Config::new(DEFAULT_BASE_URL, DEFAULT_DEVICE_BASE_URL, "auto".into())
            .expect("defaults are valid");
        assert_eq!(
            config.device_base.join("device/token").unwrap().as_str(),
            "https://console.redrob.ai/api/backend/v1/device/token"
        );
    }
}
