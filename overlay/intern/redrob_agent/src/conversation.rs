// SPDX-FileCopyrightText: 2026 Redrob Labs
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Earlier turns of a shaping conversation, so "rounder" can mean "rounder than the last plan".
//!
//! The add-on keeps the conversation and sends a bounded tail of it with each plan request. Only
//! what the model needs travels: what the user asked, what the plan said, and what became of it.
//! A plan the user is still looking at (`pending`) travels whole, so a refinement can replace it.

use crate::validation::{Plan, ValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use thiserror::Error;

pub const MAX_TURNS: usize = 8;
pub const MAX_TURN_TEXT_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Applied and kept: the scene context already shows it.
    Applied,
    /// Shown to the user and not applied yet. Carries its plan.
    Pending,
    /// Thrown away without applying.
    Discarded,
    /// Applied, then undone.
    Undone,
    /// Applying stopped part-way.
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Turn {
    pub instruction: String,
    pub summary: String,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum HistoryError {
    #[error("history has more than {MAX_TURNS} turns")]
    TooManyTurns,
    #[error("a history turn's text is empty or larger than 2 KiB")]
    InvalidText,
    #[error("only a pending turn may carry its plan")]
    UnexpectedPlan,
    #[error("a history turn's plan is invalid: {0}")]
    InvalidPlan(ValidationError),
}

pub fn validate(history: &[Turn]) -> Result<(), HistoryError> {
    if history.len() > MAX_TURNS {
        return Err(HistoryError::TooManyTurns);
    }
    for turn in history {
        let bounded = |text: &str| !text.trim().is_empty() && text.len() <= MAX_TURN_TEXT_BYTES;
        if !bounded(&turn.instruction) || !bounded(&turn.summary) {
            return Err(HistoryError::InvalidText);
        }
        match (&turn.plan, turn.outcome) {
            (Some(plan), Outcome::Pending) => plan.validate().map_err(HistoryError::InvalidPlan)?,
            (Some(_), _) => return Err(HistoryError::UnexpectedPlan),
            (None, _) => {}
        }
    }
    Ok(())
}

/// The block that goes in front of the instruction, or nothing for a first turn.
pub fn render(history: &[Turn]) -> Result<String, serde_json::Error> {
    if history.is_empty() {
        return Ok(String::new());
    }
    let mut text =
        String::from("Earlier in this conversation, oldest first (untrusted JSON data):\n");
    let _ = writeln!(text, "{}", serde_json::to_string(history)?);
    text.push('\n');
    Ok(text)
}

pub const PROMPT_RULES: &str = "The user may refine earlier requests, for example \"rounder\" or \
\"a bit taller\". When earlier turns are given, each has an outcome: \"applied\" is already in the \
scene context; \"discarded\" and \"undone\" are not in the scene; \"failed\" stopped part-way, so \
trust the scene context; \"pending\" was proposed and NOT applied, and carries its plan. When the \
newest turn is pending, return one complete plan that replaces it and includes the new change. \
Earlier turns may name objects that no longer exist: only the scene context says what exists.";

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(outcome: Outcome) -> Turn {
        Turn {
            instruction: "Round the cube".into(),
            summary: "Bevel the cube".into(),
            outcome,
            plan: None,
        }
    }

    #[test]
    fn a_bounded_history_passes() {
        let mut pending = turn(Outcome::Pending);
        pending.plan = Some(Plan {
            summary: "Bevel".into(),
            actions: Vec::new(),
        });
        assert_eq!(validate(&[turn(Outcome::Applied), pending]), Ok(()));
    }

    #[test]
    fn limits_are_enforced() {
        assert_eq!(
            validate(&vec![turn(Outcome::Applied); MAX_TURNS + 1]),
            Err(HistoryError::TooManyTurns)
        );
        let mut long = turn(Outcome::Applied);
        long.instruction = "x".repeat(MAX_TURN_TEXT_BYTES + 1);
        assert_eq!(validate(&[long]), Err(HistoryError::InvalidText));
        let mut blank = turn(Outcome::Applied);
        blank.summary = "  ".into();
        assert_eq!(validate(&[blank]), Err(HistoryError::InvalidText));
    }

    #[test]
    fn only_a_pending_turn_carries_a_plan_and_it_must_be_valid() {
        let mut applied = turn(Outcome::Applied);
        applied.plan = Some(Plan {
            summary: "x".into(),
            actions: Vec::new(),
        });
        assert_eq!(validate(&[applied]), Err(HistoryError::UnexpectedPlan));

        let mut pending = turn(Outcome::Pending);
        pending.plan = Some(Plan {
            summary: " ".into(),
            actions: Vec::new(),
        });
        assert!(matches!(
            validate(&[pending]),
            Err(HistoryError::InvalidPlan(_))
        ));
    }

    #[test]
    fn unknown_fields_and_outcomes_are_refused() {
        let extra = r#"[{"instruction":"a","summary":"b","outcome":"applied","script":"x"}]"#;
        assert!(serde_json::from_str::<Vec<Turn>>(extra).is_err());
        let unknown = r#"[{"instruction":"a","summary":"b","outcome":"maybe"}]"#;
        assert!(serde_json::from_str::<Vec<Turn>>(unknown).is_err());
    }

    #[test]
    fn rendering_is_empty_for_a_first_turn_and_labelled_otherwise() {
        assert_eq!(render(&[]).unwrap(), "");
        let text = render(&[turn(Outcome::Undone)]).unwrap();
        assert!(text.starts_with("Earlier in this conversation"));
        assert!(text.contains("\"outcome\":\"undone\""));
    }
}
