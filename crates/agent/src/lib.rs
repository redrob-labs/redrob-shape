//! MCP tool surface and shared-daemon connection for the agent.
//!
//! Three pieces, all without a window:
//! - [`tools`]: what the agent can do to a [`shape_scene::Scene`], as plain functions;
//! - [`control`]: the loopback channel between the window and the MCP server it spawns;
//! - [`mcp`]: the stdio MCP server the shared Redrob Code daemon talks to.
//!
//! The agent process itself (`redrob acp --attach`) is driven by the desktop app over ACP; see
//! `apps/redrob-shape/src/acp.rs`.

pub mod control;
pub mod mcp;
pub mod script;
pub mod tools;

pub use control::{CONTROL_TOKEN_ENV, ControlRequest, ControlServer, Remote};
pub use mcp::Server;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-agent";

/// Run the MCP server on stdio against a window at `addr` (token from [`CONTROL_TOKEN_ENV`]).
pub fn serve_stdio(addr: &str) -> std::io::Result<()> {
    let token = std::env::var(CONTROL_TOKEN_ENV).unwrap_or_default();
    let remote = Remote::connect(addr, &token)?;
    let mut server = Server::new(Box::new(remote));
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    server.serve(stdin.lock(), stdout.lock())
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name() {
        assert_eq!(super::CRATE, "shape-agent");
    }
}
