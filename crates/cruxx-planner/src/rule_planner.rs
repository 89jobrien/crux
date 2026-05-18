//! Path B: user-composable, deterministic rule-based pipeline composer.
//!
//! [`RulePlanner`] matches a goal string against a priority-ordered list of
//! [`PlanRule`]s. The first rule whose *all* keywords appear (case-insensitive)
//! in the goal wins. When no rule matches, `default_steps` is returned.

/// A rule that maps a set of goal keywords to a handler step sequence.
///
/// All keywords must be present (case-insensitive substring match) for the
/// rule to fire.
#[derive(Debug, Clone)]
pub struct PlanRule {
    /// Keywords that must all appear in the goal (case-insensitive).
    pub keywords: Vec<String>,
    /// Handler sequence to emit when this rule matches.
    pub steps: Vec<String>,
}

impl PlanRule {
    /// Convenience constructor — accepts anything `Into<String>`.
    pub fn new(
        keywords: impl IntoIterator<Item = impl Into<String>>,
        steps: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            keywords: keywords.into_iter().map(Into::into).collect(),
            steps: steps.into_iter().map(Into::into).collect(),
        }
    }

    /// Returns `true` when every keyword in this rule appears in `goal_lower`.
    ///
    /// `goal_lower` must already be lowercased by the caller.
    fn matches(&self, goal_lower: &str) -> bool {
        self.keywords
            .iter()
            .all(|kw| goal_lower.contains(kw.to_lowercase().as_str()))
    }
}

/// A deterministic, rule-based planner (Path B).
///
/// Matches goal strings against a priority-ordered list of rules.  The first
/// matching rule wins.  If no rule matches, `default_steps` is returned.
///
/// # Example
///
/// ```
/// use cruxx_planner::rule_planner::{PlanRule, RulePlanner};
///
/// let rules = vec![
///     PlanRule::new(["fetch"], ["http::get", "json::write"]),
/// ];
/// let planner = RulePlanner::new(rules, vec!["shell::capture".into()]);
/// assert_eq!(planner.plan("fetch the remote data"), vec!["http::get", "json::write"]);
/// assert_eq!(planner.plan("unknown goal"), vec!["shell::capture"]);
/// ```
#[derive(Debug, Clone)]
pub struct RulePlanner {
    rules: Vec<PlanRule>,
    default_steps: Vec<String>,
}

impl RulePlanner {
    /// Create a new planner with the given rules and fallback step sequence.
    pub fn new(rules: Vec<PlanRule>, default_steps: Vec<String>) -> Self {
        Self {
            rules,
            default_steps,
        }
    }

    /// Match the goal against rules and return the first matching step sequence.
    ///
    /// Falls back to `default_steps` if no rule matches.
    pub fn plan(&self, goal: &str) -> Vec<String> {
        let lower = goal.to_lowercase();
        self.rules
            .iter()
            .find(|r| r.matches(&lower))
            .map(|r| r.steps.clone())
            .unwrap_or_else(|| self.default_steps.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steps(s: &[&str]) -> Vec<String> {
        s.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn rule_matches_by_keyword() {
        let rules = vec![PlanRule::new(["fetch"], ["http::get", "json::write"])];
        let planner = RulePlanner::new(rules, steps(&["shell::capture"]));
        assert_eq!(
            planner.plan("fetch the remote resource"),
            steps(&["http::get", "json::write"])
        );
    }

    #[test]
    fn rule_requires_all_keywords() {
        let rules = vec![PlanRule::new(["read", "json"], ["fs::read", "json::parse"])];
        let planner = RulePlanner::new(rules, steps(&["shell::capture"]));
        // only "read" present — should NOT match
        assert_eq!(planner.plan("read the file"), steps(&["shell::capture"]));
        // both keywords present — should match
        assert_eq!(
            planner.plan("read and parse json"),
            steps(&["fs::read", "json::parse"])
        );
    }

    #[test]
    fn first_matching_rule_wins() {
        let rules = vec![
            PlanRule::new(["git"], ["git::diff", "json::write"]),
            PlanRule::new(["write"], ["fs::read", "fs::write"]),
        ];
        let planner = RulePlanner::new(rules, steps(&["shell::capture"]));
        // "git write" matches the first rule (git), not the second
        assert_eq!(
            planner.plan("git write a summary"),
            steps(&["git::diff", "json::write"])
        );
    }

    #[test]
    fn default_steps_when_no_match() {
        let rules = vec![PlanRule::new(["git"], ["git::diff"])];
        let planner = RulePlanner::new(rules, steps(&["shell::capture", "json::write"]));
        assert_eq!(
            planner.plan("xyzzy frobnicate"),
            steps(&["shell::capture", "json::write"])
        );
    }

    #[test]
    fn matching_is_case_insensitive() {
        let rules = vec![PlanRule::new(["fetch"], ["http::get"])];
        let planner = RulePlanner::new(rules, steps(&["shell::capture"]));
        assert_eq!(planner.plan("Fetch the remote data"), steps(&["http::get"]));
        assert_eq!(planner.plan("FETCH ALL THE THINGS"), steps(&["http::get"]));
    }
}
