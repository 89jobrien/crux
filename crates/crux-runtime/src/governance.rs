use serde::{Deserialize, Serialize};

/// Result of a policy check against a tool or content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyAction {
    Allow,
    Deny,
    Review,
}

/// Composable, serializable governance policy for agent tool access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernancePolicy {
    pub name: String,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub blocked_tools: Vec<String>,
    #[serde(default)]
    pub blocked_patterns: Vec<String>,
    #[serde(default = "default_max_calls")]
    pub max_calls_per_request: usize,
    #[serde(default)]
    pub require_human_approval: Vec<String>,
}

fn default_max_calls() -> usize {
    100
}

impl Default for GovernancePolicy {
    fn default() -> Self {
        Self {
            name: String::new(),
            allowed_tools: vec![],
            blocked_tools: vec![],
            blocked_patterns: vec![],
            max_calls_per_request: default_max_calls(),
            require_human_approval: vec![],
        }
    }
}

impl GovernancePolicy {
    /// Check whether a tool is permitted by this policy.
    pub fn check_tool(&self, tool_name: &str) -> PolicyAction {
        if self.blocked_tools.iter().any(|t| t == tool_name) {
            return PolicyAction::Deny;
        }
        if self.require_human_approval.iter().any(|t| t == tool_name) {
            return PolicyAction::Review;
        }
        if !self.allowed_tools.is_empty() && !self.allowed_tools.iter().any(|t| t == tool_name) {
            return PolicyAction::Deny;
        }
        PolicyAction::Allow
    }

    /// Check content against blocked patterns. Returns the first matched pattern.
    pub fn check_content(&self, content: &str) -> Option<String> {
        let content_lower = content.to_lowercase();
        for pattern in &self.blocked_patterns {
            if content_lower.contains(&pattern.to_lowercase()) {
                return Some(pattern.clone());
            }
        }
        None
    }
}

/// Compose multiple policies with most-restrictive-wins semantics.
///
/// Blocked lists union, allowed lists intersect, rate limits take minimum.
pub fn compose_policies(policies: &[GovernancePolicy]) -> GovernancePolicy {
    let mut combined = GovernancePolicy {
        name: "composed".into(),
        max_calls_per_request: usize::MAX,
        ..Default::default()
    };

    for policy in policies {
        combined.blocked_tools.extend(policy.blocked_tools.clone());
        combined
            .blocked_patterns
            .extend(policy.blocked_patterns.clone());
        combined
            .require_human_approval
            .extend(policy.require_human_approval.clone());
        combined.max_calls_per_request = combined
            .max_calls_per_request
            .min(policy.max_calls_per_request);

        if !policy.allowed_tools.is_empty() {
            combined.allowed_tools = if combined.allowed_tools.is_empty() {
                policy.allowed_tools.clone()
            } else {
                combined
                    .allowed_tools
                    .iter()
                    .filter(|t| policy.allowed_tools.contains(t))
                    .cloned()
                    .collect()
            };
        }
    }

    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_tool_passes() {
        let policy = GovernancePolicy {
            allowed_tools: vec!["search".into(), "read".into()],
            ..Default::default()
        };
        assert_eq!(policy.check_tool("search"), PolicyAction::Allow);
    }

    #[test]
    fn unlisted_tool_denied_when_allowlist_set() {
        let policy = GovernancePolicy {
            allowed_tools: vec!["search".into()],
            ..Default::default()
        };
        assert_eq!(policy.check_tool("shell_exec"), PolicyAction::Deny);
    }

    #[test]
    fn blocked_tool_denied() {
        let policy = GovernancePolicy {
            blocked_tools: vec!["shell_exec".into()],
            ..Default::default()
        };
        assert_eq!(policy.check_tool("shell_exec"), PolicyAction::Deny);
    }

    #[test]
    fn blocked_overrides_allowed() {
        let policy = GovernancePolicy {
            allowed_tools: vec!["shell_exec".into()],
            blocked_tools: vec!["shell_exec".into()],
            ..Default::default()
        };
        assert_eq!(policy.check_tool("shell_exec"), PolicyAction::Deny);
    }

    #[test]
    fn review_tool_flagged() {
        let policy = GovernancePolicy {
            require_human_approval: vec!["send_email".into()],
            ..Default::default()
        };
        assert_eq!(policy.check_tool("send_email"), PolicyAction::Review);
    }

    #[test]
    fn empty_policy_allows_everything() {
        let policy = GovernancePolicy::default();
        assert_eq!(policy.check_tool("anything"), PolicyAction::Allow);
    }

    #[test]
    fn content_filter_catches_pattern() {
        let policy = GovernancePolicy {
            blocked_patterns: vec!["api_key".into(), "password".into()],
            ..Default::default()
        };
        assert_eq!(
            policy.check_content("set API_KEY=sk-123"),
            Some("api_key".into())
        );
    }

    #[test]
    fn content_filter_passes_clean_input() {
        let policy = GovernancePolicy {
            blocked_patterns: vec!["password".into()],
            ..Default::default()
        };
        assert!(
            policy
                .check_content("search for quarterly report")
                .is_none()
        );
    }

    #[test]
    fn compose_unions_blocked_lists() {
        let a = GovernancePolicy {
            blocked_tools: vec!["shell".into()],
            ..Default::default()
        };
        let b = GovernancePolicy {
            blocked_tools: vec!["delete".into()],
            ..Default::default()
        };
        let c = compose_policies(&[a, b]);
        assert_eq!(c.check_tool("shell"), PolicyAction::Deny);
        assert_eq!(c.check_tool("delete"), PolicyAction::Deny);
    }

    #[test]
    fn compose_intersects_allowed_lists() {
        let a = GovernancePolicy {
            allowed_tools: vec!["search".into(), "read".into(), "write".into()],
            ..Default::default()
        };
        let b = GovernancePolicy {
            allowed_tools: vec!["search".into(), "read".into()],
            ..Default::default()
        };
        let c = compose_policies(&[a, b]);
        assert_eq!(c.check_tool("search"), PolicyAction::Allow);
        assert_eq!(c.check_tool("write"), PolicyAction::Deny);
    }

    #[test]
    fn compose_takes_minimum_rate_limit() {
        let a = GovernancePolicy {
            max_calls_per_request: 50,
            ..Default::default()
        };
        let b = GovernancePolicy {
            max_calls_per_request: 10,
            ..Default::default()
        };
        let c = compose_policies(&[a, b]);
        assert_eq!(c.max_calls_per_request, 10);
    }

    #[test]
    fn serde_round_trip() {
        let policy = GovernancePolicy {
            name: "test".into(),
            allowed_tools: vec!["search".into()],
            blocked_tools: vec!["shell".into()],
            blocked_patterns: vec!["password".into()],
            max_calls_per_request: 25,
            require_human_approval: vec!["email".into()],
        };
        let json = serde_json::to_string(&policy).unwrap();
        let back: GovernancePolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.max_calls_per_request, 25);
        assert_eq!(back.allowed_tools, vec!["search"]);
    }
}
