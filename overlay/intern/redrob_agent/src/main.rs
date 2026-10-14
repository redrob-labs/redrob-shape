// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use redrob_shape_agent::api::RedrobClient;
use redrob_shape_agent::config::Config;
use redrob_shape_agent::credentials::{Credentials, KeychainStore};
use redrob_shape_agent::device::DeviceClient;
use redrob_shape_agent::engine::Engine;
use redrob_shape_agent::protocol::{serve, Agent};
use std::process::ExitCode;
use std::sync::Arc;

#[tokio::main]
async fn main() -> ExitCode {
    // No key is held here. The shared Redrob Code daemon has it; the keychain is only read once to
    // move a key an earlier version left there.
    let engine = Engine::from_env();
    let credentials = Credentials::new(Arc::new(KeychainStore));
    let agent = match Config::from_env() {
        Ok(config) => match (
            RedrobClient::new(config.clone()),
            DeviceClient::new(config.device_base.clone()),
        ) {
            (Ok(client), Ok(device)) => Agent::new(client, device, engine, credentials),
            _ => Agent::unavailable(
                "client_configuration",
                "HTTP client configuration failed".into(),
                engine,
                credentials,
            ),
        },
        Err(error) => Agent::unavailable(
            "invalid_configuration",
            error.to_string(),
            engine,
            credentials,
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
