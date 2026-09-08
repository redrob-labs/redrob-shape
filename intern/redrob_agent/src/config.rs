// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use reqwest::Url;
use std::env;
use thiserror::Error;

pub const DEFAULT_BASE_URL: &str = "https://console.redrob.ai/api/backend/v1/chat/completions";
pub const DEFAULT_MODEL: &str = "auto";
const MAX_MODEL_LENGTH: usize = 128;

#[derive(Clone, Debug)]
pub struct Config {
    pub endpoint: Url,
    pub model: String,
    pub api_key: String,
}

#[derive(Debug, Error, Clone)]
pub enum ConfigError {
    #[error("REDROB_API_KEY is not configured")]
    MissingApiKey,
    #[error("REDROB_BASE_URL must be a valid HTTPS URL without credentials or a fragment")]
    InvalidBaseUrl,
    #[error("REDROB_MODEL must be between 1 and {MAX_MODEL_LENGTH} bytes")]
    InvalidModel,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let api_key = env::var("REDROB_API_KEY")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingApiKey)?;
        let base_url = env::var("REDROB_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let model = env::var("REDROB_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
        Self::new(api_key, &base_url, model)
    }

    pub fn new(api_key: String, base_url: &str, model: String) -> Result<Self, ConfigError> {
        let endpoint = Url::parse(base_url).map_err(|_| ConfigError::InvalidBaseUrl)?;
        if endpoint.scheme() != "https"
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(ConfigError::InvalidBaseUrl);
        }
        if model.is_empty() || model.len() > MAX_MODEL_LENGTH || model.chars().any(char::is_control)
        {
            return Err(ConfigError::InvalidModel);
        }
        Ok(Self {
            endpoint,
            model,
            api_key,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_insecure_or_credentialed_urls() {
        assert!(matches!(
            Config::new("secret".into(), "http://example.com/v1", "auto".into()),
            Err(ConfigError::InvalidBaseUrl)
        ));
        assert!(matches!(
            Config::new(
                "secret".into(),
                "https://user:pass@example.com/v1",
                "auto".into()
            ),
            Err(ConfigError::InvalidBaseUrl)
        ));
    }
}
