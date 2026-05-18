use chrono::{DateTime, Utc};
use cruxx_core::types::harness::HarnessDiff;
use cruxx_types::id::CruxId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What kind of improvement is being proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImprovementKind {
    Resource,
    ToolPreference,
    DecompositionStrategy,
    DelegationPolicy,
    PromptTemplate,
    ConfidenceThreshold,
}

/// An incremental change to an agent's strategy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StrategyDiff {
    pub tool_preferences: Vec<(String, i32)>,
    pub confidence_thresholds: Vec<(String, f32)>,
    pub delegation_rules: Vec<DelegationRule>,
    pub prompt_patches: Vec<PromptPatch>,
    pub harness_diff: Option<HarnessDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationRule {
    pub pattern: String,
    pub min_steps: u32,
    pub action: DelegationAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegationAction {
    Delegate,
    Inline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptPatch {
    pub agent: String,
    pub section: String,
    pub content: String,
}

impl StrategyDiff {
    pub fn has_changes(&self) -> bool {
        !self.tool_preferences.is_empty()
            || !self.confidence_thresholds.is_empty()
            || !self.delegation_rules.is_empty()
            || !self.prompt_patches.is_empty()
            || self.harness_diff.as_ref().is_some_and(|d| d.has_changes())
    }
}

/// A proposed improvement with evidence and confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Improvement {
    pub id: CruxId,
    pub kind: ImprovementKind,
    pub target: String,
    pub diff: StrategyDiff,
    pub confidence: f32,
    pub evidence: Vec<String>,
    pub proposed_at: DateTime<Utc>,
}

/// An agent's accumulated strategy — the thing that improves over time.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Strategy {
    pub version: u64,
    pub tool_preferences: HashMap<String, i32>,
    pub confidence_thresholds: HashMap<String, f32>,
    pub delegation_rules: Vec<DelegationRule>,
    pub prompt_overrides: HashMap<String, String>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Strategy {
    pub fn apply(&mut self, diff: &StrategyDiff) {
        for (tool, weight) in &diff.tool_preferences {
            *self.tool_preferences.entry(tool.clone()).or_insert(0) += weight;
        }
        for (key, threshold) in &diff.confidence_thresholds {
            self.confidence_thresholds.insert(key.clone(), *threshold);
        }
        self.delegation_rules.extend(diff.delegation_rules.clone());
        for patch in &diff.prompt_patches {
            self.prompt_overrides
                .insert(patch.agent.clone(), patch.content.clone());
        }
        self.version += 1;
        self.updated_at = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_serializes_as_snake_case() {
        let kind = ImprovementKind::ToolPreference;
        assert_eq!(
            serde_json::to_string(&kind).unwrap(),
            r#""tool_preference""#
        );
    }

    #[test]
    fn strategy_diff_default_has_no_changes() {
        assert!(!StrategyDiff::default().has_changes());
    }

    #[test]
    fn strategy_diff_with_tool_pref_has_changes() {
        let d = StrategyDiff {
            tool_preferences: vec![("rg".into(), 10)],
            ..Default::default()
        };
        assert!(d.has_changes());
    }

    #[test]
    fn strategy_apply_accumulates() {
        let mut s = Strategy::default();
        let d = StrategyDiff {
            tool_preferences: vec![("rg".into(), 5)],
            ..Default::default()
        };
        s.apply(&d);
        assert_eq!(s.tool_preferences["rg"], 5);
        assert_eq!(s.version, 1);
        s.apply(&d);
        assert_eq!(s.tool_preferences["rg"], 10);
        assert_eq!(s.version, 2);
    }

    #[test]
    fn improvement_is_serializable() {
        let imp = Improvement {
            id: CruxId::new(),
            kind: ImprovementKind::ConfidenceThreshold,
            target: "agent-a".into(),
            diff: StrategyDiff::default(),
            confidence: 0.8,
            evidence: vec!["finding".into()],
            proposed_at: Utc::now(),
        };
        let json = serde_json::to_string(&imp).unwrap();
        let back: Improvement = serde_json::from_str(&json).unwrap();
        assert_eq!(back.target, "agent-a");
    }
}
