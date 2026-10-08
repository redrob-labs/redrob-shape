// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use redrob_shape_agent::api::RedrobClient;
use redrob_shape_agent::config::Config;
use redrob_shape_agent::protocol::{serve, Agent};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let agent = match Config::from_env() {
        Ok(config) => match RedrobClient::new(config) {
            Ok(client) => Agent::configured(client),
            Err(error) => Agent::unavailable("client_configuration", error.to_string()),
        },
        Err(error) => Agent::unavailable(
            match error {
                redrob_shape_agent::config::ConfigError::MissingApiKey => "missing_api_key",
                _ => "invalid_configuration",
            },
            error.to_string(),
        ),
    };

    match serve(tokio::io::stdin(), tokio::io::stdout(), agent).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("redrob-shape-agent: protocol I/O failed: {error}");
            ExitCode::FAILURE
        }
    }
}
