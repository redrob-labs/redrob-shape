// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a plan may contain, checked against the one tool contract.
//!
//! `tool_catalog.json` lives with the add-on, which loads it at run time; this file compiles the same
//! bytes in. The planning prompt is built from it too, so what the model is told, what this agent
//! accepts and what Blender accepts cannot drift apart. `tool_conformance.json` holds cases both
//! validators must judge alike, and the test at the bottom runs them here.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::OnceLock;
use thiserror::Error;

/// The path is the same in this repository's `overlay/` and in the patched Blender tree, where
/// `intern/` and `scripts/` are siblings.
pub const CATALOG_JSON: &str =
    include_str!("../../../scripts/addons_core/redrob_shape/tool_catalog.json");

const MAX_REPORTED_NAME_CHARS: usize = 48;

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub limits: Limits,
    pub face_selector_degrees: f64,
    pub tools: BTreeMap<String, Tool>,
}

#[derive(Debug, Deserialize)]
pub struct Limits {
    pub max_actions: usize,
    pub max_name_bytes: usize,
    pub max_summary_bytes: usize,
}

#[derive(Debug, Deserialize)]
pub struct Tool {
    pub description: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Field>,
    #[serde(default)]
    pub variant_field: Option<String>,
    #[serde(default)]
    pub variants: BTreeMap<String, Variant>,
    #[serde(default)]
    pub at_least_one_of: Vec<String>,
    #[serde(default)]
    pub mesh_only: bool,
}

#[derive(Debug, Deserialize)]
pub struct Variant {
    pub description: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Field>,
}

#[derive(Debug, Deserialize)]
pub struct Field {
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub required: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Object,
    Name,
    MaterialName,
    ModifierName,
    Enum,
    Number,
    Integer,
    Boolean,
    Vector3,
    Vector4,
}

pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(CATALOG_JSON).expect("tool_catalog.json is valid"))
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub summary: String,
    pub actions: Vec<Action>,
}

/// One action, exactly as the model sent it once it has passed the catalog. Absent optional fields
/// stay absent: the add-on fills defaults in, so its preview shows every effective value.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Action {
    #[serde(rename = "type")]
    pub kind: String,
    pub arguments: Map<String, Value>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("summary is empty or too large")]
    InvalidSummary,
    #[error("plan contains too many actions")]
    TooManyActions,
    #[error("unsupported action: {0}")]
    UnknownAction(String),
    #[error("unsupported argument: {0}")]
    UnknownArgument(String),
    #[error("missing argument: {0}")]
    MissingArgument(String),
    #[error("argument has the wrong type: {0}")]
    WrongType(String),
    #[error("argument is not one of the allowed values: {0}")]
    InvalidChoice(String),
    #[error("name is empty or too large")]
    InvalidName,
    #[error("numeric argument is not finite or is outside its allowed range")]
    InvalidNumber,
    #[error("transform_object must specify at least one transform")]
    EmptyTransform,
    #[error("modifier arguments do not match the selected modifier")]
    InvalidModifierArguments,
}

/// Untrusted text in an error message: bounded and single-line.
fn reported(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_REPORTED_NAME_CHARS)
        .collect()
}

impl Plan {
    pub fn validate(&self) -> Result<(), ValidationError> {
        let limits = &catalog().limits;
        if self.summary.trim().is_empty() || self.summary.len() > limits.max_summary_bytes {
            return Err(ValidationError::InvalidSummary);
        }
        if self.actions.len() > limits.max_actions {
            return Err(ValidationError::TooManyActions);
        }
        for action in &self.actions {
            action.validate()?;
        }
        Ok(())
    }
}

impl Action {
    /// Every field this action may carry: the tool's own, plus the chosen variant's.
    fn fields(
        &self,
    ) -> Result<(&'static Tool, BTreeMap<&'static str, &'static Field>), ValidationError> {
        let tool = catalog()
            .tools
            .get(&self.kind)
            .ok_or_else(|| ValidationError::UnknownAction(reported(&self.kind)))?;
        let mut fields: BTreeMap<&str, &Field> = tool
            .fields
            .iter()
            .map(|(name, field)| (name.as_str(), field))
            .collect();
        if let Some(variant_field) = &tool.variant_field {
            let chosen = self
                .arguments
                .get(variant_field)
                .and_then(Value::as_str)
                .and_then(|value| tool.variants.get(value))
                .ok_or(ValidationError::InvalidModifierArguments)?;
            fields.extend(
                chosen
                    .fields
                    .iter()
                    .map(|(name, field)| (name.as_str(), field)),
            );
        }
        Ok((tool, fields))
    }

    fn validate(&self) -> Result<(), ValidationError> {
        let (tool, fields) = self.fields()?;
        for name in self.arguments.keys() {
            let is_variant = tool.variant_field.as_deref() == Some(name.as_str());
            if !is_variant && !fields.contains_key(name.as_str()) {
                return Err(if tool.variant_field.is_some() {
                    ValidationError::InvalidModifierArguments
                } else {
                    ValidationError::UnknownArgument(reported(name))
                });
            }
        }
        for (name, field) in &fields {
            match self.arguments.get(*name) {
                Some(value) => check(name, field, value)?,
                None if field.required => {
                    return Err(if tool.variant_field.is_some() {
                        ValidationError::InvalidModifierArguments
                    } else {
                        ValidationError::MissingArgument((*name).to_owned())
                    })
                }
                None => {}
            }
        }
        if !tool.at_least_one_of.is_empty()
            && !tool
                .at_least_one_of
                .iter()
                .any(|name| self.arguments.contains_key(name))
        {
            return Err(ValidationError::EmptyTransform);
        }
        Ok(())
    }
}

fn check(name: &str, field: &Field, value: &Value) -> Result<(), ValidationError> {
    let wrong = || ValidationError::WrongType(name.to_owned());
    match field.kind {
        Kind::Object | Kind::Name | Kind::MaterialName | Kind::ModifierName => {
            validate_name(value.as_str().ok_or_else(wrong)?)
        }
        Kind::Enum => {
            let text = value.as_str().ok_or_else(wrong)?;
            if field.values.iter().any(|allowed| allowed == text) {
                Ok(())
            } else {
                Err(ValidationError::InvalidChoice(name.to_owned()))
            }
        }
        Kind::Boolean => value.as_bool().map(|_| ()).ok_or_else(wrong),
        Kind::Number => number(value, field).ok_or_else(wrong)?,
        Kind::Integer => {
            // 2.0 is not an integer, here or in the add-on.
            if value.is_f64() {
                return Err(wrong());
            }
            let integer = value.as_i64().ok_or_else(wrong)?;
            in_range(integer as f64, field)
        }
        Kind::Vector3 | Kind::Vector4 => {
            let size = if field.kind == Kind::Vector3 { 3 } else { 4 };
            let items = value
                .as_array()
                .filter(|items| items.len() == size)
                .ok_or_else(wrong)?;
            for item in items {
                number(item, field).ok_or_else(wrong)??;
            }
            Ok(())
        }
    }
}

/// `None` when the value is not a number at all; otherwise whether it is in range.
fn number(value: &Value, field: &Field) -> Option<Result<(), ValidationError>> {
    value.as_f64().map(|number| in_range(number, field))
}

fn in_range(value: f64, field: &Field) -> Result<(), ValidationError> {
    let above_min = field.min.is_none_or(|minimum| value >= minimum);
    let below_max = field.max.is_none_or(|maximum| value <= maximum);
    if value.is_finite() && above_min && below_max {
        Ok(())
    } else {
        Err(ValidationError::InvalidNumber)
    }
}

fn validate_name(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty()
        || value.len() > catalog().limits.max_name_bytes
        || value.chars().any(char::is_control)
    {
        Err(ValidationError::InvalidName)
    } else {
        Ok(())
    }
}

pub fn parse_plan(content: &str) -> Result<Plan, PlanParseError> {
    let json = extract_json(content)?;
    let mut plan: Plan = serde_json::from_str(json).map_err(|_| PlanParseError::InvalidJson)?;
    // Models often send `null` for an optional argument they do not use. The per-tool serde
    // structs this replaced read that as absent, so the catalog check does too; a required
    // argument sent as null is then simply missing.
    for action in &mut plan.actions {
        action.arguments.retain(|_, value| !value.is_null());
    }
    plan.validate().map_err(PlanParseError::Validation)?;
    Ok(plan)
}

fn extract_json(content: &str) -> Result<&str, PlanParseError> {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return Ok(trimmed);
    }

    let after_open = &trimmed[3..];
    let newline = after_open.find('\n').ok_or(PlanParseError::InvalidFence)?;
    let language = after_open[..newline].trim();
    if !language.is_empty() && !language.eq_ignore_ascii_case("json") {
        return Err(PlanParseError::InvalidFence);
    }
    let body_and_close = after_open[newline + 1..].trim_end();
    let body = body_and_close
        .strip_suffix("```")
        .ok_or(PlanParseError::InvalidFence)?;
    Ok(body.trim())
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum PlanParseError {
    #[error("model response is not valid JSON for a plan")]
    InvalidJson,
    #[error("model response contains an invalid code fence")]
    InvalidFence,
    #[error("model response failed local validation: {0}")]
    Validation(ValidationError),
}

/// The planning instructions, built once from the catalog.
pub fn system_prompt() -> &'static str {
    static PROMPT: OnceLock<String> = OnceLock::new();
    PROMPT.get_or_init(build_system_prompt)
}

fn describe_field(name: &str, field: &Field) -> String {
    let mut text = match field.kind {
        Kind::Object => "name of an object in the scene or created earlier in this plan".to_owned(),
        Kind::Name => "new object name".to_owned(),
        Kind::MaterialName => "new material name".to_owned(),
        Kind::ModifierName => "modifier name".to_owned(),
        Kind::Enum => format!("one of {}", field.values.join(", ")),
        Kind::Boolean => "true or false".to_owned(),
        Kind::Number | Kind::Integer | Kind::Vector3 | Kind::Vector4 => {
            let shape = match field.kind {
                Kind::Number => "number",
                Kind::Integer => "integer",
                Kind::Vector3 => "[x, y, z] numbers",
                _ => "[r, g, b, a] numbers",
            };
            match (field.min, field.max) {
                (Some(minimum), Some(maximum)) => format!("{shape} from {minimum} to {maximum}"),
                _ => shape.to_owned(),
            }
        }
    };
    if let Some(description) = &field.description {
        let _ = write!(text, ", {description}");
    }
    if field.required {
        text.push_str(", required");
    } else if let Some(default) = &field.default {
        let _ = write!(text, ", default {default}");
    }
    format!("\"{name}\": {text}")
}

fn build_system_prompt() -> String {
    let catalog = catalog();
    let mut prompt = String::new();
    let _ = writeln!(
        prompt,
        "You are the planning service of Redrob Shape, a 3D creation suite built on Blender. \
         Return exactly one strict JSON object and no commentary:"
    );
    let _ = writeln!(
        prompt,
        "{{\"summary\":\"non-empty string\",\"actions\":[ACTION,...]}}"
    );
    let _ = writeln!(
        prompt,
        "Use at most {} actions. Each ACTION is {{\"type\":\"<tool>\",\"arguments\":{{...}}}} with one of these tools:",
        catalog.limits.max_actions
    );
    for (name, tool) in &catalog.tools {
        let fields: Vec<String> = tool
            .fields
            .iter()
            .map(|(field_name, field)| describe_field(field_name, field))
            .collect();
        match &tool.variant_field {
            None => {
                let _ = writeln!(
                    prompt,
                    "- {name}: {} Arguments: {}",
                    tool.description,
                    fields.join("; ")
                );
            }
            Some(variant_field) => {
                let _ = writeln!(prompt, "- {name}: {}", tool.description);
                for (variant, details) in &tool.variants {
                    let mut all = fields.clone();
                    all.insert(1, format!("\"{variant_field}\": \"{variant}\""));
                    all.extend(
                        details
                            .fields
                            .iter()
                            .map(|(field_name, field)| describe_field(field_name, field)),
                    );
                    let _ = writeln!(
                        prompt,
                        "  - {variant_field} \"{variant}\": {} Arguments: {}",
                        details.description,
                        all.join("; ")
                    );
                }
            }
        }
    }
    let _ = writeln!(
        prompt,
        "Rules: optional arguments may be left out, but never add arguments a tool does not list. \
         Names are at most {} UTF-8 bytes. Every number is finite. When the instruction does not name \
         an object, shape the selected objects, or the active object if none is selected. Refer to \
         objects by their exact names from the scene context or from earlier actions in this plan. \
         Face directions are world axes; faces count as pointing along an axis within {} degrees. \
         Never emit Python, shell commands, file or network operations, or any tool not listed here.",
        catalog.limits.max_name_bytes, catalog.face_selector_degrees
    );
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_plan() -> &'static str {
        r#"{"summary":"Add a cube","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","name":"Cube","location":[0.0,1.0,2.0],"scale":[1.0,1.0,1.0]}}]}"#
    }

    #[test]
    fn the_catalog_parses_and_every_tool_is_described() {
        let catalog = catalog();
        assert!(catalog.tools.len() >= 13);
        for (name, tool) in &catalog.tools {
            assert!(!tool.description.trim().is_empty(), "{name}");
            if tool.variant_field.is_some() {
                assert!(!tool.variants.is_empty(), "{name}");
            }
        }
    }

    #[test]
    fn the_prompt_names_every_tool_and_variant_from_the_catalog() {
        let prompt = system_prompt();
        for (name, tool) in &catalog().tools {
            assert!(prompt.contains(&format!("- {name}:")), "{name}");
            for variant in tool.variants.keys() {
                assert!(
                    prompt.contains(&format!("\"{variant}\":")),
                    "{name} {variant}"
                );
            }
        }
    }

    #[test]
    fn parses_plain_and_fenced_json() {
        let plain = parse_plan(valid_plan()).expect("plain plan should parse");
        let fenced = parse_plan(&format!("```json\n{}\n```", valid_plan()))
            .expect("fenced plan should parse");
        assert_eq!(plain, fenced);
    }

    #[test]
    fn rejects_unknown_action_and_arguments() {
        // These were InvalidJson while each tool was a serde struct; the catalog now names them.
        let unknown_action =
            r#"{"summary":"bad","actions":[{"type":"run_python","arguments":{}}]}"#;
        assert_eq!(
            parse_plan(unknown_action),
            Err(PlanParseError::Validation(ValidationError::UnknownAction(
                "run_python".into()
            )))
        );

        let unknown_argument = r#"{"summary":"bad","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","script":"oops"}}]}"#;
        assert_eq!(
            parse_plan(unknown_argument),
            Err(PlanParseError::Validation(
                ValidationError::UnknownArgument("script".into())
            ))
        );
        let extra_envelope = r#"{"summary":"bad","actions":[{"type":"delete_object","arguments":{"object":"Cube"},"run":true}]}"#;
        assert_eq!(parse_plan(extra_envelope), Err(PlanParseError::InvalidJson));
    }

    #[test]
    fn rejects_out_of_range_and_non_finite_values() {
        let huge = r#"{"summary":"bad","actions":[{"type":"transform_object","arguments":{"object":"Cube","location":[1000001.0,0.0,0.0]}}]}"#;
        assert!(matches!(
            parse_plan(huge),
            Err(PlanParseError::Validation(ValidationError::InvalidNumber))
        ));

        let tiny_scale = r#"{"summary":"bad","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","scale":[1e-12,1.0,1.0]}}]}"#;
        assert!(matches!(
            parse_plan(tiny_scale),
            Err(PlanParseError::Validation(ValidationError::InvalidNumber))
        ));

        let too_far = r#"{"summary":"bad","actions":[{"type":"extrude_faces","arguments":{"object":"Cube","faces":"+z","distance":1000.5}}]}"#;
        assert!(matches!(
            parse_plan(too_far),
            Err(PlanParseError::Validation(ValidationError::InvalidNumber))
        ));
    }

    #[test]
    fn validates_modifier_specific_arguments() {
        let invalid = r#"{"summary":"bad","actions":[{"type":"add_modifier","arguments":{"object":"Cube","modifier":"mirror","width":1.0,"axis":"x"}}]}"#;
        assert!(matches!(
            parse_plan(invalid),
            Err(PlanParseError::Validation(
                ValidationError::InvalidModifierArguments
            ))
        ));
        let twist_without_angle = r#"{"summary":"bad","actions":[{"type":"add_modifier","arguments":{"object":"Cube","modifier":"twist"}}]}"#;
        assert!(matches!(
            parse_plan(twist_without_angle),
            Err(PlanParseError::Validation(
                ValidationError::InvalidModifierArguments
            ))
        ));
    }

    #[test]
    fn integers_are_integers() {
        let fractional = r#"{"summary":"bad","actions":[{"type":"subdivide_mesh","arguments":{"object":"Cube","cuts":2.0}}]}"#;
        assert_eq!(
            parse_plan(fractional),
            Err(PlanParseError::Validation(ValidationError::WrongType(
                "cuts".into()
            )))
        );
    }

    #[test]
    fn omits_absent_optional_arguments_from_validated_output() {
        let plan = parse_plan(
            r#"{"summary":"Smooth it","actions":[{"type":"add_modifier","arguments":{"object":"Cube","modifier":"subdivision","levels":2}}]}"#,
        )
        .expect("plan should parse");
        let value = serde_json::to_value(plan).expect("plan should serialize");
        let arguments = &value["actions"][0]["arguments"];
        assert_eq!(arguments["levels"], 2);
        assert!(arguments.get("width").is_none());
        assert!(arguments.get("segments").is_none());
        assert!(arguments.get("axis").is_none());
        assert!(arguments.get("render_levels").is_none());
    }

    #[test]
    fn rejects_unclosed_or_non_json_fences() {
        assert_eq!(
            parse_plan("```rust\n{}\n```"),
            Err(PlanParseError::InvalidFence)
        );
        assert_eq!(parse_plan("```json\n{}"), Err(PlanParseError::InvalidFence));
    }

    /// The cases the add-on's tests also run. Both validators must give every one the same verdict.
    #[test]
    fn agrees_with_the_shared_conformance_cases() {
        #[derive(Deserialize)]
        struct Case {
            name: String,
            plan: Value,
            valid: bool,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../../scripts/addons_core/redrob_shape/tests/tool_conformance.json"
        ))
        .expect("tool_conformance.json is valid");
        assert!(cases.len() > 100, "only {} cases", cases.len());
        for case in cases {
            let verdict = parse_plan(&case.plan.to_string());
            assert_eq!(verdict.is_ok(), case.valid, "{}: {verdict:?}", case.name);
        }
    }
}
