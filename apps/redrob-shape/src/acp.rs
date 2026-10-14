//! The built-in agent: Redrob Code over the Agent Client Protocol (`redrob acp --attach`,
//! JSON-RPC 2.0, one message per line on the child's stdio). `--attach` keeps the process a thin
//! client of the one shared Redrob Code daemon rather than an engine of its own.
//!
//! Only the handful of ACP methods the Agent palette needs are typed here; anything else is
//! ignored (notifications) or declined (requests). The scene tools reach the agent as an MCP
//! server named in `session/new`: this same executable, run as `redrob-shape mcp --connect`, talking
//! to the window's token-gated control port. Nothing is written to the user's Redrob Code config,
//! and keys and subscriptions stay in Redrob Code's own sign-in (`redrob auth login`).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use shape_ui_egui::agent::{AgentEvent, AgentHost, PermissionOption};

/// Overrides where Redrob Code is looked up.
pub const BINARY_ENV: &str = "REDROB_CODE_PATH";
pub const INSTALL_HINT: &str = "Redrob Code is not installed. Install it from https://code.redrob.ai, then send again.";
pub const SIGN_IN_HINT: &str = "Redrob Code needs a Redrob workspace. Connect one above, then send again.";
/// Overrides the model the agent uses; otherwise Redrob's own router.
pub const MODEL_ENV: &str = "REDROB_SHAPE_MODEL";
pub const DEFAULT_MODEL: &str = "redrob/auto";
/// ACP's code for "authenticate first".
const AUTH_REQUIRED: i64 = -32000;

/// How the agent reaches this window's scene tools.
#[derive(Clone, Debug)]
pub struct McpLink {
    /// `127.0.0.1:<port>` of the control server.
    pub addr: String,
    pub token: Option<String>,
}

/// Find `redrob`: the override, then `PATH`, then the usual install directories (a GUI app on
/// macOS starts with a minimal `PATH`).
pub fn find_binary() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os(BINARY_ENV).map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = if cfg!(windows) { "redrob.exe" } else { "redrob" };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from) {
        dirs.extend([home.join(".redrob/bin"), home.join(".local/bin"), home.join(".bun/bin")]);
    }
    // Where Redrob Code's Windows installer puts it (per-user, and what the Redrob Shape setup installs).
    // PATH would find it too, but only for processes started after that installer updated PATH.
    if cfg!(windows)
        && let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    {
        dirs.push(local.join("Redrob").join("bin"));
    }
    dirs.extend([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")]);
    dirs.into_iter().map(|d| d.join(exe)).find(|p| p.is_file())
}

/// What a response to one of our requests means.
enum Pending {
    Initialize,
    NewSession,
    SetModel,
    Prompt,
}

/// The model to ask for, when the session offers it. Redrob Code's own default can land on a
/// model that cannot chat (an image model), so the Shape agent always names one.
fn wanted_model(options: &Value) -> Option<String> {
    let want = std::env::var(MODEL_ENV).ok().filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let opt = options.as_array()?.iter().find(|o| o["id"] == "model" || o["category"] == "model")?;
    if opt["currentValue"].as_str() == Some(want.as_str()) {
        return None;
    }
    fn has(list: &Value, v: &str) -> bool {
        list.as_array().into_iter().flatten().any(|o| o["value"].as_str() == Some(v) || has(&o["options"], v))
    }
    has(&opt["options"], &want).then_some(want)
}

struct Shared {
    stdin: ChildStdin,
    next_id: u64,
    pending: HashMap<u64, Pending>,
    session: Option<String>,
    /// The session id while its model is being set.
    opening: Option<String>,
    queued: Vec<String>,
    /// Our permission request number -> the agent's JSON-RPC id.
    questions: HashMap<u64, Value>,
    next_question: u64,
    cwd: String,
    mcp: McpLink,
}

impl Shared {
    fn write(&mut self, msg: &Value) {
        let _ = writeln!(self.stdin, "{msg}").and_then(|_| self.stdin.flush());
    }
    fn request(&mut self, method: &str, params: Value, kind: Pending) {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, kind);
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
    }
    fn prompt(&mut self, text: &str) {
        let Some(sid) = self.session.clone() else {
            self.queued.push(text.to_string());
            return;
        };
        self.request("session/prompt", json!({"sessionId": sid, "prompt": [{"type": "text", "text": text}]}), Pending::Prompt);
    }
    fn mcp_servers(&self) -> Value {
        let Ok(exe) = std::env::current_exe() else { return json!([]) };
        let env: Vec<Value> = self.mcp.token.iter().map(|t| json!({"name": shape_agent::CONTROL_TOKEN_ENV, "value": t})).collect();
        json!([{
            "name": "redrob-shape",
            "command": exe.to_string_lossy(),
            "args": ["mcp", "--connect", self.mcp.addr],
            "env": env,
        }])
    }
}

pub struct AcpHost {
    child: Child,
    shared: Arc<Mutex<Shared>>,
    events: Receiver<AgentEvent>,
}

/// Arguments for the agent process. `--attach` makes it a thin client of the one shared
/// Redrob Code daemon (started on demand, shared with every other Redrob app, gone after ten
/// idle minutes) instead of a full engine of its own per window.
fn acp_args(cwd: &str) -> Vec<String> {
    vec!["acp".into(), "--attach".into(), "--cwd".into(), cwd.into()]
}

impl AcpHost {
    /// Spawn `redrob acp --attach` in `cwd` and start the handshake; the session opens in the
    /// background and any prompt sent before it is ready is queued.
    pub fn start(binary: &Path, cwd: &str, mcp: McpLink, env: &[(&str, String)], ctx: egui::Context) -> Result<Self, String> {
        let mut child = Command::new(binary)
            .args(acp_args(cwd))
            .envs(env.iter().map(|(k, v)| (*k, v.as_str())))
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Could not start Redrob Code ({}): {e}", binary.display()))?;
        let (Some(stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
            let _ = child.kill();
            return Err("Could not open Redrob Code's input and output.".into());
        };
        let shared = Arc::new(Mutex::new(Shared {
            stdin,
            next_id: 1,
            pending: HashMap::new(),
            session: None,
            opening: None,
            queued: Vec::new(),
            questions: HashMap::new(),
            next_question: 1,
            cwd: cwd.to_string(),
            mcp,
        }));
        let (tx, events) = channel();
        let tail = Arc::new(Mutex::new(String::new()));
        {
            let tail = tail.clone();
            std::thread::spawn(move || {
                // Keep the last few KB of stderr for the exit message; never block the agent.
                let mut buf = [0u8; 4096];
                let mut r = stderr;
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut t) = tail.lock() {
                        t.push_str(&String::from_utf8_lossy(&buf[..n]));
                        if t.len() > 4000 {
                            let cut = t.len() - 4000;
                            let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(t.len());
                            t.drain(..cut);
                        }
                    }
                }
            });
        }
        {
            let shared = shared.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                    for ev in handle(&shared, &msg) {
                        let _ = tx.send(ev);
                    }
                    ctx.request_repaint();
                }
                let detail = tail.lock().map(|t| last_line(&t)).unwrap_or_default();
                let msg = if detail.is_empty() { "Redrob Code exited.".to_string() } else { format!("Redrob Code exited: {detail}") };
                let _ = tx.send(AgentEvent::Error(msg));
                ctx.request_repaint();
            });
        }
        if let Ok(mut s) = shared.lock() {
            s.request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientInfo": {"name": "redrob_shape", "title": "Redrob Shape", "version": env!("CARGO_PKG_VERSION")},
                    "clientCapabilities": {},
                }),
                Pending::Initialize,
            );
        }
        Ok(Self { child, shared, events })
    }
}

fn last_line(s: &str) -> String {
    s.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").chars().take(300).collect()
}

/// Turn one message from the agent into palette events, answering what must be answered.
fn handle(shared: &Arc<Mutex<Shared>>, msg: &Value) -> Vec<AgentEvent> {
    let Ok(mut s) = shared.lock() else { return Vec::new() };
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").cloned();
    match (method, id) {
        // A request from the agent.
        (Some("session/request_permission"), Some(id)) => {
            let p = &msg["params"];
            let title = p["toolCall"]["title"].as_str().unwrap_or("this tool").to_string();
            let options: Vec<PermissionOption> = p["options"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|o| {
                    Some(PermissionOption {
                        id: o["optionId"].as_str()?.to_string(),
                        name: o["name"].as_str().unwrap_or("?").to_string(),
                        kind: o["kind"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect();
            let q = s.next_question;
            s.next_question += 1;
            s.questions.insert(q, id);
            vec![AgentEvent::Permission { request: q, title, options }]
        }
        (Some(other), Some(id)) => {
            s.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("Unsupported request {other}")}}));
            Vec::new()
        }
        // A notification.
        (Some("session/update"), None) => update_events(&msg["params"]["update"]),
        (Some(_), None) => Vec::new(),
        // A response to one of ours.
        (None, Some(id)) => {
            let Some(kind) = id.as_u64().and_then(|n| s.pending.remove(&n)) else { return Vec::new() };
            if let (Pending::SetModel, Some(err)) = (&kind, msg.get("error")) {
                // Keep the agent's own default rather than failing the session.
                s.session = s.opening.take();
                for text in std::mem::take(&mut s.queued) {
                    s.prompt(&text);
                }
                return vec![AgentEvent::Notice(format!("Could not choose the model: {}", err["message"].as_str().unwrap_or("error")))];
            }
            if let Some(err) = msg.get("error") {
                let code = err["code"].as_i64().unwrap_or(0);
                let text = err["message"].as_str().unwrap_or("unknown error");
                let auth = code == AUTH_REQUIRED || text.to_ascii_lowercase().contains("auth");
                return vec![AgentEvent::Error(if auth { SIGN_IN_HINT.to_string() } else { format!("Redrob Code: {text}") })];
            }
            let r = &msg["result"];
            match kind {
                Pending::Initialize => {
                    let info = &r["agentInfo"];
                    let name =
                        [info["title"].as_str().or(info["name"].as_str()).unwrap_or("Redrob Code"), info["version"].as_str().unwrap_or("")].join(" ");
                    let params = json!({"cwd": s.cwd, "mcpServers": s.mcp_servers()});
                    s.request("session/new", params, Pending::NewSession);
                    vec![AgentEvent::Ready { name: name.trim().to_string() }]
                }
                Pending::NewSession => {
                    let Some(sid) = r["sessionId"].as_str() else { return vec![AgentEvent::Error("Redrob Code did not open a session.".into())] };
                    let Some(model) = wanted_model(&r["configOptions"]) else {
                        s.session = Some(sid.to_string());
                        for text in std::mem::take(&mut s.queued) {
                            s.prompt(&text);
                        }
                        return Vec::new();
                    };
                    // Prompts wait until the model is set, so the first turn uses it too.
                    s.request("session/set_config_option", json!({"sessionId": sid, "configId": "model", "value": model}), Pending::SetModel);
                    s.opening = Some(sid.to_string());
                    vec![AgentEvent::Notice(format!("Model: {model}"))]
                }
                Pending::SetModel => {
                    s.session = s.opening.take();
                    for text in std::mem::take(&mut s.queued) {
                        s.prompt(&text);
                    }
                    Vec::new()
                }
                Pending::Prompt => vec![AgentEvent::TurnEnd { stop: r["stopReason"].as_str().unwrap_or("end_turn").to_string() }],
            }
        }
        (None, None) => Vec::new(),
    }
}

/// `session/update` payloads the palette shows.
pub fn update_events(u: &Value) -> Vec<AgentEvent> {
    let text = || u["content"]["text"].as_str().unwrap_or("").to_string();
    match u["sessionUpdate"].as_str() {
        Some("agent_message_chunk") => vec![AgentEvent::Text(text())],
        Some("agent_thought_chunk") => vec![AgentEvent::Thought(text())],
        Some("tool_call" | "tool_call_update") => vec![AgentEvent::Tool {
            id: u["toolCallId"].as_str().unwrap_or("").to_string(),
            title: u["title"].as_str().unwrap_or("").to_string(),
            status: u["status"].as_str().unwrap_or(if u["sessionUpdate"] == "tool_call" { "pending" } else { "" }).to_string(),
        }],
        _ => Vec::new(),
    }
}

impl AgentHost for AcpHost {
    fn prompt(&mut self, text: &str) {
        if let Ok(mut s) = self.shared.lock() {
            s.prompt(text);
        }
    }
    fn cancel(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(sid) = s.session.clone() {
                s.write(&json!({"jsonrpc": "2.0", "method": "session/cancel", "params": {"sessionId": sid}}));
            }
            // A question still open is answered as cancelled, which the protocol requires.
            for (_, id) in std::mem::take(&mut s.questions) {
                s.write(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": {"outcome": "cancelled"}}}));
            }
        }
    }
    fn answer(&mut self, request: u64, option: Option<&str>) {
        if let Ok(mut s) = self.shared.lock() {
            let Some(id) = s.questions.remove(&request) else { return };
            let outcome = match option {
                Some(o) => json!({"outcome": "selected", "optionId": o}),
                None => json!({"outcome": "cancelled"}),
            };
            s.write(&json!({"jsonrpc": "2.0", "id": id, "result": {"outcome": outcome}}));
        }
    }
    fn poll(&mut self) -> Vec<AgentEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for AcpHost {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_session_updates() {
        assert_eq!(
            update_events(&json!({"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "hi"}})),
            vec![AgentEvent::Text("hi".into())]
        );
        assert_eq!(
            update_events(&json!({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "redrob-shape_add_primitive"})),
            vec![AgentEvent::Tool { id: "c1".into(), title: "redrob-shape_add_primitive".into(), status: "pending".into() }]
        );
        assert_eq!(
            update_events(&json!({"sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed"})),
            vec![AgentEvent::Tool { id: "c1".into(), title: String::new(), status: "completed".into() }]
        );
        assert!(update_events(&json!({"sessionUpdate": "plan", "entries": []})).is_empty());
    }

    #[test]
    fn asks_for_the_router_only_when_offered_and_not_current() {
        let opts = |cur: &str| json!([{"id": "model", "currentValue": cur, "options": [{"value": "a/b"}, {"name": "g", "options": [{"value": "redrob/auto"}]}]}]);
        if std::env::var_os(MODEL_ENV).is_none() {
            assert_eq!(wanted_model(&opts("google/x-image-preview")), Some("redrob/auto".into()));
            assert_eq!(wanted_model(&opts("redrob/auto")), None);
            assert_eq!(wanted_model(&json!([{"id": "model", "currentValue": "a/b", "options": [{"value": "a/b"}]}])), None);
        }
        assert_eq!(wanted_model(&Value::Null), None);
    }

    #[test]
    fn last_line_skips_blank_tail() {
        assert_eq!(last_line("a\nerror: no provider\n\n"), "error: no provider");
        assert_eq!(last_line(""), "");
    }

    #[test]
    fn attaches_to_the_shared_daemon() {
        assert_eq!(acp_args("/w"), ["acp", "--attach", "--cwd", "/w"]);
    }

    /// The whole client against a scripted agent: handshake, the MCP server it hands over,
    /// a streamed reply, a permission question and its answer, the end of the turn.
    #[cfg(unix)]
    #[test]
    fn talks_acp_to_a_scripted_agent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("redrob-shape-acp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let script = dir.join("redrob");
        let new_log = dir.join("new.json");
        let ans_log = dir.join("answer.json");
        let body = format!(
            r#"#!/bin/sh
read l; echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"fake","version":"9"}}}}}}'
read l; printf '%s' "$l" > '{new}'; echo '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"s1"}}}}'
read l; echo '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"ok"}}}}}}}}'
echo '{{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{{"sessionId":"s1","toolCall":{{"toolCallId":"c1","title":"command_line"}},"options":[{{"optionId":"a","name":"Allow","kind":"allow_once"}}]}}}}'
read l; printf '%s' "$l" > '{ans}'
echo '{{"jsonrpc":"2.0","id":3,"result":{{"stopReason":"end_turn"}}}}'
read l
"#,
            new = new_log.display(),
            ans = ans_log.display()
        );
        let _ = std::fs::write(&script, body);
        let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
        let link = McpLink { addr: "127.0.0.1:4242".into(), token: Some("t0k".into()) };
        let Ok(mut host) = AcpHost::start(&script, &dir.to_string_lossy(), link, &[], egui::Context::default()) else {
            unreachable_in_test("start failed");
            return;
        };
        host.prompt("draw a circle");
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline && !got.iter().any(|e| matches!(e, AgentEvent::TurnEnd { .. })) {
            for ev in host.poll() {
                if let AgentEvent::Permission { request, .. } = &ev {
                    host.answer(*request, Some("a"));
                }
                got.push(ev);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(got.first(), Some(&AgentEvent::Ready { name: "fake 9".into() }));
        assert!(got.contains(&AgentEvent::Text("ok".into())), "{got:?}");
        assert!(got.iter().any(|e| matches!(e, AgentEvent::Permission { title, options, .. } if title == "command_line" && options.len() == 1)));
        assert_eq!(got.last(), Some(&AgentEvent::TurnEnd { stop: "end_turn".into() }));
        let new: Value = serde_json::from_str(&std::fs::read_to_string(&new_log).unwrap_or_default()).unwrap_or_default();
        let server = &new["params"]["mcpServers"][0];
        assert_eq!(new["method"], "session/new");
        assert_eq!(server["name"], "redrob-shape");
        assert_eq!(server["args"], json!(["mcp", "--connect", "127.0.0.1:4242"]));
        assert_eq!(server["env"], json!([{"name": shape_agent::CONTROL_TOKEN_ENV, "value": "t0k"}]));
        let ans: Value = serde_json::from_str(&std::fs::read_to_string(&ans_log).unwrap_or_default()).unwrap_or_default();
        assert_eq!(ans, json!({"jsonrpc": "2.0", "id": "p1", "result": {"outcome": {"outcome": "selected", "optionId": "a"}}}));
        drop(host);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    fn unreachable_in_test(why: &str) {
        assert!(why.is_empty(), "{why}");
    }
}
