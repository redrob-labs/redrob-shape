// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_ACTIONS: usize = 128;
pub const MAX_SUMMARY_BYTES: usize = 8 * 1024;
const MAX_NAME_BYTES: usize = 63;
const MAX_ABS_COORDINATE: f64 = 1_000_000.0;
const MIN_SCALE: f64 = 1.0e-6;
const MAX_SCALE: f64 = 10_000.0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub summary: String,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", content = "arguments", rename_all = "snake_case")]
pub enum Action {
    CreatePrimitive(CreatePrimitive),
    TransformObject(TransformObject),
    AddModifier(AddModifier),
    SetMaterial(SetMaterial),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreatePrimitive {
    pub primitive: Primitive,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Primitive {
    Cube,
    Sphere,
    Cylinder,
    Cone,
    Plane,
    Torus,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TransformObject {
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f64; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f64; 3]>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AddModifier {
    pub object: String,
    pub modifier: Modifier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segments: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub levels: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_levels: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thickness: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<Axis>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Bevel,
    Subdivision,
    Solidify,
    Mirror,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SetMaterial {
    pub object: String,
    pub name: String,
    pub base_color: [f64; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f64>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("summary is empty or too large")]
    InvalidSummary,
    #[error("plan contains too many actions")]
    TooManyActions,
    #[error("name is empty or too large")]
    InvalidName,
    #[error("numeric argument is not finite or is outside its allowed range")]
    InvalidNumber,
    #[error("transform_object must specify at least one transform")]
    EmptyTransform,
    #[error("modifier arguments do not match the selected modifier")]
    InvalidModifierArguments,
}

impl Plan {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.summary.trim().is_empty() || self.summary.len() > MAX_SUMMARY_BYTES {
            return Err(ValidationError::InvalidSummary);
        }
        if self.actions.len() > MAX_ACTIONS {
            return Err(ValidationError::TooManyActions);
        }
        for action in &self.actions {
            action.validate()?;
        }
        Ok(())
    }
}

impl Action {
    fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::CreatePrimitive(args) => {
                if let Some(name) = &args.name {
                    validate_name(name)?;
                }
                if let Some(location) = args.location {
                    validate_vector(location, -MAX_ABS_COORDINATE, MAX_ABS_COORDINATE)?;
                }
                if let Some(scale) = args.scale {
                    validate_vector(scale, MIN_SCALE, MAX_SCALE)?;
                }
            }
            Self::TransformObject(args) => {
                validate_name(&args.object)?;
                if args.location.is_none() && args.rotation.is_none() && args.scale.is_none() {
                    return Err(ValidationError::EmptyTransform);
                }
                if let Some(location) = args.location {
                    validate_vector(location, -MAX_ABS_COORDINATE, MAX_ABS_COORDINATE)?;
                }
                if let Some(rotation) = args.rotation {
                    validate_vector(rotation, -MAX_ABS_COORDINATE, MAX_ABS_COORDINATE)?;
                }
                if let Some(scale) = args.scale {
                    validate_vector(scale, MIN_SCALE, MAX_SCALE)?;
                }
            }
            Self::AddModifier(args) => validate_modifier(args)?,
            Self::SetMaterial(args) => {
                validate_name(&args.object)?;
                validate_name(&args.name)?;
                validate_vector(args.base_color, 0.0, 1.0)?;
                if let Some(value) = args.metallic {
                    validate_number(value, 0.0, 1.0)?;
                }
                if let Some(value) = args.roughness {
                    validate_number(value, 0.0, 1.0)?;
                }
            }
        }
        Ok(())
    }
}

fn validate_modifier(args: &AddModifier) -> Result<(), ValidationError> {
    validate_name(&args.object)?;
    let valid = match args.modifier {
        Modifier::Bevel => {
            args.levels.is_none()
                && args.render_levels.is_none()
                && args.thickness.is_none()
                && args.axis.is_none()
                && args
                    .width
                    .is_some_and(|value| validate_number(value, 0.0, MAX_ABS_COORDINATE).is_ok())
                && args.segments.is_some_and(|value| (1..=64).contains(&value))
        }
        Modifier::Subdivision => {
            args.width.is_none()
                && args.segments.is_none()
                && args.thickness.is_none()
                && args.axis.is_none()
                && args.levels.is_some_and(|value| value <= 6)
                && args.render_levels.is_none_or(|value| value <= 6)
        }
        Modifier::Solidify => {
            args.width.is_none()
                && args.segments.is_none()
                && args.levels.is_none()
                && args.render_levels.is_none()
                && args.axis.is_none()
                && args.thickness.is_some_and(|value| {
                    validate_number(value, -MAX_ABS_COORDINATE, MAX_ABS_COORDINATE).is_ok()
                })
        }
        Modifier::Mirror => {
            args.width.is_none()
                && args.segments.is_none()
                && args.levels.is_none()
                && args.render_levels.is_none()
                && args.thickness.is_none()
                && args.axis.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifierArguments)
    }
}

fn validate_name(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty()
        || value.len() > MAX_NAME_BYTES
        || value.chars().any(char::is_control)
    {
        Err(ValidationError::InvalidName)
    } else {
        Ok(())
    }
}

fn validate_vector<const N: usize>(
    values: [f64; N],
    minimum: f64,
    maximum: f64,
) -> Result<(), ValidationError> {
    for value in values {
        validate_number(value, minimum, maximum)?;
    }
    Ok(())
}

fn validate_number(value: f64, minimum: f64, maximum: f64) -> Result<(), ValidationError> {
    if value.is_finite() && value >= minimum && value <= maximum {
        Ok(())
    } else {
        Err(ValidationError::InvalidNumber)
    }
}

pub fn parse_plan(content: &str) -> Result<Plan, PlanParseError> {
    let json = extract_json(content)?;
    let plan: Plan = serde_json::from_str(json).map_err(|_| PlanParseError::InvalidJson)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_plan() -> &'static str {
        r#"{"summary":"Add a cube","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","name":"Cube","location":[0.0,1.0,2.0],"scale":[1.0,1.0,1.0]}}]}"#
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
        let unknown_action =
            r#"{"summary":"bad","actions":[{"type":"run_python","arguments":{}}]}"#;
        assert_eq!(parse_plan(unknown_action), Err(PlanParseError::InvalidJson));

        let unknown_argument = r#"{"summary":"bad","actions":[{"type":"create_primitive","arguments":{"primitive":"cube","script":"oops"}}]}"#;
        assert_eq!(
            parse_plan(unknown_argument),
            Err(PlanParseError::InvalidJson)
        );
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

        let action = Action::SetMaterial(SetMaterial {
            object: "Cube".into(),
            name: "Material".into(),
            base_color: [f64::NAN, 0.0, 0.0, 1.0],
            metallic: None,
            roughness: None,
        });
        assert_eq!(action.validate(), Err(ValidationError::InvalidNumber));
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
    }

    #[test]
    fn rejects_unclosed_or_non_json_fences() {
        assert_eq!(
            parse_plan("```rust\n{}\n```"),
            Err(PlanParseError::InvalidFence)
        );
        assert_eq!(parse_plan("```json\n{}"), Err(PlanParseError::InvalidFence));
    }
}
