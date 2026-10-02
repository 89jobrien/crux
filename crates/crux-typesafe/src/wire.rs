//! Wire types for the TypeSafe System One evaluation endpoint.
//!
//! Shapes follow the published API at `POST https://api.typesafe.ai/v1/systemone`.
//! Only the fields crux acts on are modelled; unknown fields are ignored so a
//! backend addition cannot break a pipeline.
//!
//! [`ScoreAnswer`], [`ChoiceAnswer`], and [`NoulAnswer`] all deserialize from the
//! same `answers` map by matching their `type` tag.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// The model alias crux requests.
pub const DEFAULT_MODEL: &str = "jev-latest";

// -- request --

/// A `score` question: rate the state against ordered, descriptive levels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreQuestion {
    /// Always `"score"`.
    #[serde(rename = "type")]
    pub question_type: &'static str,
    /// What is being rated.
    pub instructions: String,
    /// Ordered level descriptions, low end first.
    pub criteria: Vec<String>,
}

impl ScoreQuestion {
    /// Build a Score question.
    ///
    /// # Errors
    ///
    /// Returns [`QuestionError::TooFewLevels`] for fewer than two levels: the
    /// API rejects it, and `calibrate` could not normalize it either.
    pub fn new(
        instructions: impl Into<String>,
        criteria: Vec<String>,
    ) -> Result<Self, QuestionError> {
        if criteria.len() < 2 {
            return Err(QuestionError::TooFewLevels {
                levels: criteria.len(),
            });
        }
        Ok(Self {
            question_type: "score",
            instructions: instructions.into(),
            criteria,
        })
    }
}

/// A `choice` question: pick one option from a defined set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceQuestion {
    /// Always `"choice"`.
    #[serde(rename = "type")]
    pub question_type: &'static str,
    /// What the model should decide.
    pub instructions: String,
    /// Option name to rubric description.
    pub criteria: BTreeMap<String, String>,
}

impl ChoiceQuestion {
    /// Build a Choice question.
    pub fn new(instructions: impl Into<String>, criteria: BTreeMap<String, String>) -> Self {
        Self {
            question_type: "choice",
            instructions: instructions.into(),
            criteria,
        }
    }
}

/// A `noul` question: the probability that a yes/no condition holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulQuestion {
    /// Always `"noul"`.
    #[serde(rename = "type")]
    pub question_type: &'static str,
    /// The yes/no question.
    pub instructions: String,
    /// What a yes and a no mean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub criteria: Option<NoulCriteria>,
}

/// Optional descriptions of what a yes and a no mean.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulCriteria {
    /// What a yes, a value near 1, means.
    #[serde(rename = "true")]
    pub yes: String,
    /// What a no, a value near 0, means.
    #[serde(rename = "false")]
    pub no: String,
}

/// A request against the System One endpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneRequest {
    /// The content to evaluate.
    pub state: Value,
    /// Model alias.
    pub model: String,
    /// Typed questions, keyed by an id the caller chooses.
    ///
    /// The key is never sent to the model; the answer comes back under it.
    pub questions: BTreeMap<String, Value>,
}

/// A malformed question, rejected before any request is made.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum QuestionError {
    /// A Score question needs at least two levels.
    #[error("a Score question needs at least 2 levels, got {levels}")]
    TooFewLevels {
        /// Number of levels supplied.
        levels: usize,
    },
}

// -- response --

/// The full System One response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemOneResponse {
    /// Model that performed the evaluation, e.g. `jev-1.13.0`.
    pub model: String,
    /// One answer per requested question id.
    pub answers: BTreeMap<String, Answer>,
    /// Token usage.
    #[serde(default)]
    pub usage: Option<Usage>,
}

/// Token usage for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens consumed.
    #[serde(default)]
    pub input_tokens: u64,
    /// Output tokens consumed.
    #[serde(default)]
    pub output_tokens: u64,
}

/// One answer, discriminated by its `type` tag.
///
/// The tag is the single source of truth for the variant: the inner structs do
/// not repeat it, because serde consumes `type` for the tag before handing the
/// remainder to the variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    /// A `score` answer.
    Score(ScoreAnswer),
    /// A `choice` answer.
    Choice(ChoiceAnswer),
    /// A `noul` answer.
    Noul(NoulAnswer),
}

impl Answer {
    /// TypeSafe's concentration metric, when the variant carries one.
    ///
    /// `noul` answers report no separate `confidence` — their value *is* the
    /// probability.
    pub fn concentration(&self) -> Option<f64> {
        match self {
            Self::Score(answer) => Some(answer.confidence),
            Self::Choice(answer) => Some(answer.confidence),
            Self::Noul(_) => None,
        }
    }
}

/// A `score` answer.
///
/// Every level is judged on its own, so `probabilities` normally has a zero on
/// most levels and mass spread across one or two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreAnswer {
    /// Probability-weighted position across levels; can fall between two.
    pub score: f64,
    /// Level number to its description, echoed from `criteria`.
    pub legend: BTreeMap<String, Value>,
    /// Level number to probability. The values sum to 1.
    pub probabilities: BTreeMap<String, f64>,
    /// How concentrated `probabilities` is.
    ///
    /// Describes the answer's distribution, not its correctness. Never used as
    /// a routing confidence — see [`crate::calibration`].
    #[serde(default)]
    pub confidence: f64,
}

/// A `choice` answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceAnswer {
    /// The highest-probability option.
    pub choice: String,
    /// Every option to its probability. The values sum to 1.
    pub probabilities: BTreeMap<String, f64>,
    /// How concentrated `probabilities` is.
    #[serde(default)]
    pub confidence: f64,
}

/// A `noul` answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoulAnswer {
    /// Probability the answer is yes, `0.0` to `1.0`.
    pub noul: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_score_answer_deserializes_from_the_documented_shape() {
        let raw = json!({
            "type": "score",
            "score": 1.43,
            "legend": { "0": "Cosmetic", "1": "Workaround", "2": "Blocking" },
            "probabilities": { "0": 0.0, "1": 0.57, "2": 0.43 },
            "confidence": 0.35
        });
        let answer: Answer = serde_json::from_value(raw).unwrap();
        let Answer::Score(score) = answer else {
            panic!("expected a score answer, got {answer:?}");
        };
        assert!((score.score - 1.43).abs() < 1e-9);
        assert_eq!(score.probabilities["2"], 0.43);
        assert!((score.confidence - 0.35).abs() < 1e-9);
        assert_eq!(score.legend["1"], json!("Workaround"));
    }

    #[test]
    fn a_choice_answer_deserializes() {
        let raw = json!({
            "type": "choice",
            "choice": "billing",
            "probabilities": { "billing": 0.88, "technical": 0.12, "sales": 0.0 },
            "confidence": 0.81
        });
        let answer: Answer = serde_json::from_value(raw).unwrap();
        let Answer::Choice(choice) = answer else {
            panic!("expected a choice answer, got {answer:?}");
        };
        assert_eq!(choice.choice, "billing");
        assert_eq!(choice.probabilities["sales"], 0.0);
    }

    #[test]
    fn a_noul_answer_deserializes() {
        let raw = json!({ "type": "noul", "noul": 0.95 });
        let answer: Answer = serde_json::from_value(raw).unwrap();
        assert!(matches!(answer, Answer::Noul(_)));
        assert_eq!(answer.concentration(), None);
    }

    /// Unknown fields must not break a pipeline when the backend adds any.
    #[test]
    fn unknown_answer_fields_are_ignored() {
        let raw = json!({
            "type": "score",
            "score": 1.0,
            "legend": { "0": "a", "1": "b" },
            "probabilities": { "0": 0.5, "1": 0.5 },
            "confidence": 0.5,
            "some_future_field": { "nested": true }
        });
        let answer: Answer = serde_json::from_value(raw).unwrap();
        assert!(matches!(answer, Answer::Score(_)));
    }

    #[test]
    fn a_request_round_trips_through_the_documented_shape() {
        let question =
            ScoreQuestion::new("How severe?", vec!["low".into(), "high".into()]).unwrap();
        let request = SystemOneRequest {
            state: json!("a report"),
            model: DEFAULT_MODEL.to_owned(),
            questions: BTreeMap::from([(
                "severity".to_owned(),
                serde_json::to_value(&question).unwrap(),
            )]),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["model"], json!("jev-latest"));
        assert_eq!(value["questions"]["severity"]["type"], json!("score"));
        assert_eq!(value["questions"]["severity"]["criteria"][0], json!("low"));
    }

    #[test]
    fn a_single_level_score_question_is_rejected_before_sending() {
        let error = ScoreQuestion::new("How severe?", vec!["only".into()]).unwrap_err();
        assert!(matches!(error, QuestionError::TooFewLevels { levels: 1 }));
    }

    #[test]
    fn an_empty_choice_criteria_is_allowed_at_the_type_level() {
        // The API is the authority on whether an empty option set is meaningful;
        // the type does not second-guess it.
        let question = ChoiceQuestion::new("Which?", BTreeMap::new());
        assert!(question.criteria.is_empty());
    }
}
