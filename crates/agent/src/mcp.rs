//! A Model Context Protocol server over stdio (newline-delimited JSON-RPC 2.0) that forwards
//! `tools/call` to a window through the control channel.

use crate::control::Remote;
use crate::tools;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED: &[&str] = &[PROTOCOL_VERSION, "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Redrob Shape is a 3D modelling app. The scene is Y up, metres. Start with scene_summary to \
see what exists, then add_primitive / transform_object / set_material / mesh_op / add_modifier / add_light to build. \
Objects are referred to by name or id. Every change you make appears in the user's window at once and is undoable.";

/// Where tool calls go: the window (through [`Remote`]) or, in tests, anything else.
pub trait Backend {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    fn describe(&self) -> String;
}

impl Backend for Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        Remote::call(self, method, params)
    }
    fn describe(&self) -> String {
        "connected to the Redrob Shape window".into()
    }
}

pub struct Server {
    backend: Box<dyn Backend>,
    initialized: bool,
}

fn response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}
fn error(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.into()}})
}

impl Server {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self { backend, initialized: false }
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Serve until `input` closes.
    pub fn serve(&mut self, input: impl BufRead, mut output: impl Write) -> std::io::Result<()> {
        for line in input.lines() {
            let line = line?;
            if let Some(reply) = self.handle_line(&line) {
                output.write_all(reply.as_bytes())?;
                output.write_all(b"\n")?;
                output.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim();
        if line.is_empty() {
            return None;
        }
        let reply = match serde_json::from_str::<Value>(line) {
            Ok(Value::Array(batch)) => {
                let replies: Vec<Value> = batch.into_iter().filter_map(|m| self.handle(m)).collect();
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            Ok(msg) => self.handle(msg),
            Err(e) => Some(error(Value::Null, PARSE_ERROR, format!("parse error: {e}"))),
        };
        reply.map(|r| r.to_string())
    }

    pub fn handle(&mut self, msg: Value) -> Option<Value> {
        let Value::Object(o) = &msg else { return Some(error(Value::Null, INVALID_REQUEST, "message must be an object")) };
        let id = o.get("id").cloned();
        let Some(method) = o.get("method").and_then(Value::as_str) else {
            if o.contains_key("result") || o.contains_key("error") {
                return None;
            }
            return Some(error(id.unwrap_or(Value::Null), INVALID_REQUEST, "missing `method`"));
        };
        let params = o.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = id else {
            if method == "notifications/initialized" {
                self.initialized = true;
            }
            return None;
        };
        Some(match self.request(method, &params) {
            Ok(r) => response(id, r),
            Err((code, m)) => error(id, code, m),
        })
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, (i64, String)> {
        match method {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                let version = if SUPPORTED.contains(&asked) { asked } else { PROTOCOL_VERSION };
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}, "resources": {"listChanged": false}},
                    "serverInfo": {"name": "redrob-shape", "title": "Redrob Shape", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!("{INSTRUCTIONS} Backend: {}.", self.backend.describe()),
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tools::tool_definitions()),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).ok_or((INVALID_PARAMS, "missing tool `name`".to_string()))?;
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Ok(match self.backend.call(name, args) {
                    Ok(v) => json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}]}),
                    Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
                })
            }
            "resources/list" => Ok(json!({"resources": [{"uri": "redrob-shape://scene", "name": "Scene", "mimeType": "application/json"}]})),
            "resources/read" => {
                let uri = params.get("uri").and_then(Value::as_str).unwrap_or("");
                if uri != "redrob-shape://scene" {
                    return Err((-32002, format!("unknown resource `{uri}`")));
                }
                let v = self.backend.call("scene_summary", json!({})).map_err(|e| (-32603, e))?;
                Ok(json!({"contents": [{"uri": uri, "mimeType": "application/json", "text": v.to_string()}]}))
            }
            _ => Err((METHOD_NOT_FOUND, format!("method `{method}` not found"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_scene::Scene;

    /// A backend that runs tools on an in-memory scene.
    struct Local(Scene);
    impl Backend for Local {
        fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
            tools::execute(&mut self.0, method, &params)
        }
        fn describe(&self) -> String {
            "test".into()
        }
    }

    #[test]
    fn lifecycle_and_tool_call() {
        let mut s = Server::new(Box::new(Local(Scene::new("t"))));
        let init = s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#).unwrap();
        let v: Value = serde_json::from_str(&init).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "redrob-shape");
        assert!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none());
        assert!(s.is_initialized());
        let list: Value = serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#).unwrap()).unwrap();
        assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "add_primitive"));
        let call = s
            .handle_line(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"add_primitive","arguments":{"kind":"sphere"}}}"#)
            .unwrap();
        let v: Value = serde_json::from_str(&call).unwrap();
        assert!(v["result"]["content"][0]["text"].as_str().unwrap().contains("Sphere"));
        let bad: Value =
            serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"nope"}}"#).unwrap()).unwrap();
        assert_eq!(bad["result"]["isError"], true);
        let nf: Value = serde_json::from_str(&s.handle_line(r#"{"jsonrpc":"2.0","id":5,"method":"zzz"}"#).unwrap()).unwrap();
        assert_eq!(nf["error"]["code"], METHOD_NOT_FOUND);
        let res: Value = serde_json::from_str(
            &s.handle_line(r#"{"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"redrob-shape://scene"}}"#).unwrap(),
        )
        .unwrap();
        assert!(res["result"]["contents"][0]["text"].as_str().unwrap().contains("Sphere"));
        assert!(s.handle_line("not json").unwrap().contains("parse error"));
    }
}
