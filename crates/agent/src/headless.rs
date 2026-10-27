//! The MCP backend with no window: tool calls run on a scene in this process, and the scene is
//! written back to a file after every change when a save path is given.

use std::path::PathBuf;

use serde_json::Value;
use shape_scene::Scene;

use crate::mcp::Backend;
use crate::tools;

pub struct Headless {
    pub scene: Scene,
    /// Written after every tool call that changes the scene (the format follows the extension).
    pub save: Option<PathBuf>,
}

impl Headless {
    pub fn new(scene: Scene, save: Option<PathBuf>) -> Self {
        Self { scene, save }
    }
}

impl Backend for Headless {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let result = tools::execute(&mut self.scene, method, &params)?;
        if tools::mutates(method)
            && let Some(path) = &self.save
        {
            shape_format::export(&self.scene, path).map_err(|e| format!("{method} ran, but saving {} failed: {e}", path.display()))?;
        }
        Ok(result)
    }

    fn describe(&self) -> String {
        match &self.save {
            Some(p) => format!("headless scene, saved to {}", p.display()),
            None => "headless scene (not saved)".into(),
        }
    }
}
