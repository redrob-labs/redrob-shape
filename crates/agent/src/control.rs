//! Loopback JSON-lines control channel between the window and the MCP server it starts.
//!
//! The app listens on an ephemeral `127.0.0.1` port with a random token; the MCP server
//! (`redrob-shape mcp --connect ADDR`, which the agent spawns) sends one request per line
//! (`{"id", "token", "method", "params"}`) and reads one reply per line (`{"id", "ok", "result"}`
//! or `{"id", "ok": false, "error"}`). Requests are handed to the UI thread through a channel and
//! answered when it has run them, so every tool call happens on the scene the user is looking at.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// Environment variable the app sets for the MCP server it spawns.
pub const CONTROL_TOKEN_ENV: &str = "REDROB_SHAPE_CONTROL_TOKEN";

/// One tool call waiting for the UI thread.
pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    reply: Sender<Value>,
}

impl ControlRequest {
    pub fn new(method: String, params: Value) -> (Self, Receiver<Value>) {
        let (tx, rx) = channel();
        (Self { method, params, reply: tx }, rx)
    }
    pub fn ok(self, result: Value) {
        let _ = self.reply.send(json!({"ok": true, "result": result}));
    }
    pub fn err(self, message: impl Into<String>) {
        let _ = self.reply.send(json!({"ok": false, "error": message.into()}));
    }
}

/// A running control server.
pub struct ControlServer {
    pub port: u16,
    pub token: String,
    pub requests: Receiver<ControlRequest>,
}

impl ControlServer {
    /// Bind `127.0.0.1:0` and start accepting. `wake` is called whenever a request arrives so the
    /// UI can repaint and pick it up.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let (tx, requests) = channel::<ControlRequest>();
        let wake = std::sync::Arc::new(wake);
        let token_for_threads = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let token = token_for_threads.clone();
                let wake = wake.clone();
                std::thread::spawn(move || serve(stream, tx, token, wake));
            }
        });
        Ok(Self { port, token, requests })
    }

    pub fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// Whether a request may run: only with exactly the server's token.
pub fn authorized(msg: &Value, token: &str) -> bool {
    msg.get("token").and_then(Value::as_str).is_some_and(|got| constant_time_eq(got.as_bytes(), token.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_token() -> String {
    // No crypto crate in the tree: mix the clock and addresses of fresh allocations through a
    // 64-bit hash. The token only needs to be unguessable by another local process.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::time::SystemTime::now().hash(&mut h);
    std::process::id().hash(&mut h);
    let probe = Box::new(0u8);
    (&*probe as *const u8 as usize).hash(&mut h);
    let a = h.finish();
    std::time::Instant::now().hash(&mut h);
    let b = h.finish();
    format!("{a:016x}{b:016x}")
}

fn serve(stream: TcpStream, tx: Sender<ControlRequest>, token: String, wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
    let Ok(read) = stream.try_clone() else { return };
    let mut out = stream;
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if !authorized(&msg, &token) => {
                json!({"id": msg.get("id").cloned().unwrap_or(Value::Null), "ok": false, "error": "unauthorized"})
            }
            Ok(msg) => {
                let id = msg.get("id").cloned().unwrap_or(Value::Null);
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let (req, rrx) = ControlRequest::new(method, params);
                if tx.send(req).is_err() {
                    break;
                }
                wake();
                let mut r = rrx.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| json!({"ok": false, "error": "timeout"}));
                if let Some(o) = r.as_object_mut() {
                    o.insert("id".into(), id);
                }
                r
            }
            Err(e) => json!({"ok": false, "error": format!("bad JSON: {e}")}),
        };
        if writeln!(out, "{reply}").is_err() {
            break;
        }
    }
}

/// Client side: the MCP server's connection to the window.
pub struct Remote {
    addr: String,
    token: String,
    conn: Option<(BufReader<TcpStream>, TcpStream)>,
    next_id: u64,
}

impl Remote {
    pub fn connect(addr: &str, token: &str) -> std::io::Result<Self> {
        let mut r = Self { addr: addr.to_string(), token: token.to_string(), conn: None, next_id: 1 };
        r.reconnect()?;
        Ok(r)
    }

    fn reconnect(&mut self) -> std::io::Result<()> {
        self.conn = None;
        let mut last = std::io::Error::new(std::io::ErrorKind::NotFound, format!("cannot resolve {}", self.addr));
        for sa in self.addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sa, Duration::from_millis(800)) {
                Ok(s) => {
                    s.set_nodelay(true).ok();
                    s.set_read_timeout(Some(Duration::from_secs(130))).ok();
                    let read = s.try_clone()?;
                    self.conn = Some((BufReader::new(read), s));
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    fn roundtrip(&mut self, line: &str) -> std::io::Result<String> {
        if self.conn.is_none() {
            self.reconnect()?;
        }
        let Some((reader, writer)) = self.conn.as_mut() else { return Err(std::io::Error::other("not connected")) };
        writer.write_all(line.as_bytes())?;
        writer.write_all(b"\n")?;
        writer.flush()?;
        let mut reply = String::new();
        if reader.read_line(&mut reply)? == 0 {
            self.conn = None;
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "connection closed"));
        }
        Ok(reply)
    }

    /// Run a tool in the window.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"id": id, "token": self.token, "method": method, "params": params}).to_string();
        let reply = match self.roundtrip(&line) {
            Ok(r) => r,
            Err(_) => self.roundtrip(&line).map_err(|e| format!("Redrob Shape at {}: {e}", self.addr))?,
        };
        let v: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))?;
        if v.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(v.get("result").cloned().unwrap_or(Value::Null))
        } else {
            Err(v.get("error").and_then(Value::as_str).unwrap_or("error").to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_gate() {
        assert!(authorized(&json!({"token": "abc"}), "abc"));
        assert!(!authorized(&json!({"token": "abd"}), "abc"));
        assert!(!authorized(&json!({}), "abc"));
        assert_ne!(random_token(), random_token());
    }

    #[test]
    fn round_trip_through_the_loopback() {
        let server = ControlServer::start(|| {}).unwrap();
        let (addr, token) = (server.addr(), server.token.clone());
        let requests = server.requests;
        // UI thread stand-in: answer every request by echoing its method.
        std::thread::spawn(move || {
            for req in requests {
                if req.method == "fail" {
                    req.err("nope");
                } else {
                    let body = json!({"method": req.method, "params": req.params});
                    req.ok(body);
                }
            }
        });
        let mut remote = Remote::connect(&addr, &token).unwrap();
        let r = remote.call("scene_summary", json!({"x": 1})).unwrap();
        assert_eq!(r["method"], "scene_summary");
        assert_eq!(r["params"]["x"], 1);
        assert_eq!(remote.call("fail", json!({})).unwrap_err(), "nope");
        let mut bad = Remote::connect(&addr, "wrong").unwrap();
        assert_eq!(bad.call("scene_summary", json!({})).unwrap_err(), "unauthorized");
    }
}
