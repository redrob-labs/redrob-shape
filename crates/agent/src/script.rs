//! Shape Script: the console and add-on language. Each line is one tool call from
//! [`crate::tools`] -- `add_primitive kind=cube size=1 location=[0,1,0]` -- so scripts, the
//! console and the agent share one surface, the way Blender's Python console, scripts and
//! add-ons all speak `bpy`. Values are JSON-ish: numbers, `true`/`false`, `[1,2,3]`, bare words or
//! quoted strings. `#` starts a comment. `print <text>` echoes. `help` lists the tools.

use serde_json::{Map, Value, json};
use shape_scene::Scene;

use crate::tools;

/// Add-on metadata from `# key: value` header comments.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddonInfo {
    pub name: String,
    pub description: String,
    /// Where the add-on shows up: a label under the Scripts menu.
    pub menu: Option<String>,
}

pub fn addon_info(source: &str, fallback_name: &str) -> AddonInfo {
    let mut info = AddonInfo { name: fallback_name.to_string(), ..Default::default() };
    for line in source.lines().take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#')) {
        let Some(rest) = line.trim_start().strip_prefix('#') else { continue };
        if let Some((k, v)) = rest.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "name" => info.name = v.trim().to_string(),
                "description" => info.description = v.trim().to_string(),
                "menu" => info.menu = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }
    info
}

/// Run a script against the scene; returns one output line per statement (results or errors)
/// and whether anything changed the scene.
pub fn run(scene: &mut Scene, source: &str) -> (Vec<String>, bool) {
    let mut out = Vec::new();
    let mut mutated = false;
    for (n, raw) in source.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        match run_line(scene, line) {
            Ok((msg, m)) => {
                mutated |= m;
                if !msg.is_empty() {
                    out.push(msg);
                }
            }
            Err(e) => out.push(format!("line {}: {e}", n + 1)),
        }
    }
    (out, mutated)
}

fn run_line(scene: &mut Scene, line: &str) -> Result<(String, bool), String> {
    let (name, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    match name {
        "print" | "echo" => return Ok((rest.trim().trim_matches('"').to_string(), false)),
        "help" => {
            let names: Vec<String> = tools::tool_definitions()["tools"]
                .as_array()
                .map(|t| t.iter().filter_map(|d| d["name"].as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            return Ok((format!("tools: {}  (name key=value ...; values: numbers, words, \"text\", [x,y,z], true/false)", names.join(" ")), false));
        }
        _ => {}
    }
    let args = parse_args(rest)?;
    let result = tools::execute(scene, name, &Value::Object(args))?;
    let text = match &result {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    let text = if text.len() > 400 { format!("{}…", &text[..400]) } else { text };
    Ok((format!("{name}: {text}"), tools::mutates(name)))
}

/// `key=value key2=[1, 2, 3] key3="two words"` into a JSON object.
pub fn parse_args(s: &str) -> Result<Map<String, Value>, String> {
    let mut map = Map::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let key_start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect();
        if i >= chars.len() || chars[i] != '=' {
            return Err(format!("expected '=' after {key:?}"));
        }
        i += 1;
        let value_start = i;
        if i < chars.len() && chars[i] == '[' {
            let mut depth = 0;
            while i < chars.len() {
                match chars[i] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
        } else if i < chars.len() && chars[i] == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            i = (i + 1).min(chars.len());
        } else {
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
        }
        let raw: String = chars[value_start..i].iter().collect();
        map.insert(key, parse_value(raw.trim())?);
    }
    Ok(map)
}

fn parse_value(raw: &str) -> Result<Value, String> {
    if raw.is_empty() {
        return Err("empty value".into());
    }
    if let Some(inner) = raw.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        let items: Result<Vec<Value>, String> = inner.split(',').map(str::trim).filter(|t| !t.is_empty()).map(parse_value).collect();
        return Ok(Value::Array(items?));
    }
    if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
        return Ok(Value::String(inner.to_string()));
    }
    if raw == "true" || raw == "false" {
        return Ok(json!(raw == "true"));
    }
    if let Ok(n) = raw.parse::<f64>() {
        return Ok(json!(n));
    }
    Ok(Value::String(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_parse_numbers_words_arrays_and_strings() {
        let a = parse_args(r#"kind=cube size=1.5 location=[0, 1, 0] name="My Cube" smooth=true"#).unwrap();
        assert_eq!(a["kind"], "cube");
        assert_eq!(a["size"], 1.5);
        assert_eq!(a["location"], json!([0.0, 1.0, 0.0]));
        assert_eq!(a["name"], "My Cube");
        assert_eq!(a["smooth"], true);
        assert!(parse_args("kind").is_err());
    }

    #[test]
    fn script_adds_and_moves_objects_and_reports_errors() {
        let mut scene = Scene::new("s");
        let (out, mutated) = run(
            &mut scene,
            "# name: demo\nadd_primitive kind=cube name=Box size=1\ntransform_object object=Box location=[1,2,3]\nprint done\nnope x=1\n",
        );
        assert!(mutated);
        assert_eq!(scene.object_count(), 1);
        let (_, o) = scene.objects().next().unwrap();
        assert_eq!(o.name, "Box");
        assert!((o.transform.translation.y - 2.0).abs() < 1e-6);
        assert!(out.iter().any(|l| l == "done"));
        assert!(out.iter().any(|l| l.starts_with("line 5:")), "{out:?}");
        let info = addon_info("# name: Demo Addon\n# menu: Make Demo\n# description: adds a cube\nadd_primitive kind=cube", "demo");
        assert_eq!(info.name, "Demo Addon");
        assert_eq!(info.menu.as_deref(), Some("Make Demo"));
    }
}
