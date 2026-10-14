// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use reqwest::Url;
use std::env;
use thiserror::Error;

/// The console's device-flow endpoints. The device flow is how a person obtains a Console key; the
/// key then goes to the shared Redrob Code daemon, which is the only thing that talks to the
/// Console for plans. See `engine`.
pub const DEFAULT_DEVICE_BASE_URL: &str = "https://console.redrob.ai/api/backend/v1";
/// The model as the daemon names it: `<provider>/<model>`. `redrob/auto` routes through the Console.
pub const DEFAULT_MODEL: &str = "redrob/auto";
const MAX_MODEL_LENGTH: usize = 128;

/// Where connecting goes, and what to ask for. Where plans go is not configuration: it is whatever
/// daemon record `engine` finds at the moment of the request.
#[derive(Clone, Debug)]
pub struct Config {
    /// Always ends in `/`, so `join("device/token")` appends rather than replacing the last segment.
    pub device_base: Url,
    pub model: String,
}

#[derive(Debug, Error, Clone)]
pub enum ConfigError {
    #[error("REDROB_DEVICE_BASE_URL must be a valid HTTPS URL without credentials or a fragment")]
    InvalidDeviceBaseUrl,
    #[error("REDROB_MODEL must be between 1 and {MAX_MODEL_LENGTH} bytes")]
    InvalidModel,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let device_base = env::var("REDROB_DEVICE_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_DEVICE_BASE_URL.to_owned());
        let model = env::var("REDROB_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        Self::new(&device_base, model)
    }

    pub fn new(device_base: &str, model: String) -> Result<Self, ConfigError> {
        let mut device_base = https_url(device_base).ok_or(ConfigError::InvalidDeviceBaseUrl)?;
        if !device_base.path().ends_with('/') {
            let path = format!("{}/", device_base.path());
            device_base.set_path(&path);
        }
        if model.is_empty() || model.len() > MAX_MODEL_LENGTH || model.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidModel);
        }
        Ok(Self { device_base, model })
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
                "http://console.redrob.ai/api/backend/v1",
                "redrob/auto".into()
            ),
            Err(ConfigError::InvalidDeviceBaseUrl)
        ));
        assert!(matches!(
            Config::new("https://user:pass@example.com/v1", "redrob/auto".into()),
            Err(ConfigError::InvalidDeviceBaseUrl)
        ));
        assert!(matches!(
            Config::new(DEFAULT_DEVICE_BASE_URL, String::new()),
            Err(ConfigError::InvalidModel)
        ));
    }

    #[test]
    fn device_endpoints_are_appended_under_v1() {
        let config =
            Config::new(DEFAULT_DEVICE_BASE_URL, DEFAULT_MODEL.into()).expect("defaults are valid");
        assert_eq!(
            config.device_base.join("device/token").unwrap().as_str(),
            "https://console.redrob.ai/api/backend/v1/device/token"
        );
        assert_eq!(config.model, "redrob/auto");
    }
}
