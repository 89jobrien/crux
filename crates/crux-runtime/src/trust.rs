use std::collections::HashMap;

const DEFAULT_TRUST_SCORE: f64 = 0.5;

/// Per-agent trust score with temporal decay.
#[derive(Debug, Clone)]
pub struct TrustScore {
    pub score: f64,
    pub successes: u32,
    pub failures: u32,
    last_updated: f64,
}

impl Default for TrustScore {
    fn default() -> Self {
        Self {
            score: DEFAULT_TRUST_SCORE,
            successes: 0,
            failures: 0,
            last_updated: unix_now(),
        }
    }
}

impl TrustScore {
    /// Record a successful operation, increasing trust.
    pub fn record_success(&mut self, reward: f64) {
        self.successes += 1;
        self.score = (self.score + reward * (1.0 - self.score)).min(1.0);
        self.last_updated = unix_now();
    }

    /// Record a failed operation, decreasing trust.
    pub fn record_failure(&mut self, penalty: f64) {
        self.failures += 1;
        self.score = (self.score - penalty * self.score).max(0.0);
        self.last_updated = unix_now();
    }

    /// Current score with temporal decay applied.
    pub fn current(&self, decay_rate: f64) -> f64 {
        let elapsed = unix_now() - self.last_updated;
        self.score * (-decay_rate * elapsed).exp()
    }

    /// Current score at a specific timestamp (for deterministic testing).
    pub fn current_at(&self, now: f64, decay_rate: f64) -> f64 {
        let elapsed = now - self.last_updated;
        self.score * (-decay_rate * elapsed).exp()
    }

    /// Fraction of operations that succeeded.
    pub fn reliability(&self) -> f64 {
        let total = (self.successes + self.failures) as f64;
        if total == 0.0 {
            0.0
        } else {
            self.successes as f64 / total
        }
    }
}

/// Registry of trust scores keyed by agent ID.
pub struct TrustRegistry {
    scores: HashMap<String, TrustScore>,
}

impl TrustRegistry {
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
        }
    }

    /// Get or create a mutable trust score for an agent.
    pub fn get_mut(&mut self, agent_id: &str) -> &mut TrustScore {
        self.scores.entry(agent_id.to_string()).or_default()
    }

    /// Get an immutable trust score for an agent, if it exists.
    pub fn get(&self, agent_id: &str) -> Option<&TrustScore> {
        self.scores.get(agent_id)
    }

    /// Select the most trusted agent from a list of candidates.
    pub fn most_trusted<'a>(&self, agents: &'a [String], decay_rate: f64) -> Option<&'a str> {
        agents
            .iter()
            .max_by(|a, b| {
                let ta = self
                    .scores
                    .get(a.as_str())
                    .map_or(0.5, |s| s.current(decay_rate));
                let tb = self
                    .scores
                    .get(b.as_str())
                    .map_or(0.5, |s| s.current(decay_rate));
                ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(String::as_str)
    }

    /// Check if an agent meets a minimum trust threshold.
    pub fn meets_threshold(&self, agent_id: &str, threshold: f64, decay_rate: f64) -> bool {
        self.scores
            .get(agent_id)
            .is_some_and(|s| s.current(decay_rate) >= threshold)
    }
}

impl Default for TrustRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_score_is_half() {
        let ts = TrustScore::default();
        assert!((ts.score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn success_increases_score() {
        let mut ts = TrustScore::default();
        let before = ts.score;
        ts.record_success(0.1);
        assert!(ts.score > before);
        assert_eq!(ts.successes, 1);
    }

    #[test]
    fn failure_decreases_score() {
        let mut ts = TrustScore::default();
        let before = ts.score;
        ts.record_failure(0.2);
        assert!(ts.score < before);
        assert_eq!(ts.failures, 1);
    }

    #[test]
    fn score_clamped_to_unit_range() {
        let mut ts = TrustScore {
            score: 0.99,
            last_updated: unix_now(),
            ..Default::default()
        };
        ts.record_success(1.0);
        assert!(ts.score <= 1.0);

        let mut ts2 = TrustScore {
            score: 0.01,
            last_updated: unix_now(),
            ..Default::default()
        };
        ts2.record_failure(1.0);
        assert!(ts2.score >= 0.0);
    }

    #[test]
    fn decay_reduces_score_over_time() {
        let ts = TrustScore {
            score: 0.8,
            successes: 5,
            failures: 0,
            last_updated: unix_now() - 100.0,
        };
        // With decay_rate=0.01, after 100s: 0.8 * exp(-1.0) ~ 0.294
        let decayed = ts.current(0.01);
        assert!(decayed < 0.8);
        assert!(decayed > 0.0);
    }

    #[test]
    fn current_at_deterministic() {
        let ts = TrustScore {
            score: 0.8,
            successes: 0,
            failures: 0,
            last_updated: 1000.0,
        };
        let at_1100 = ts.current_at(1100.0, 0.01);
        // 0.8 * exp(-0.01 * 100) = 0.8 * exp(-1.0)
        let expected = 0.8 * (-1.0_f64).exp();
        assert!((at_1100 - expected).abs() < 1e-10);
    }

    #[test]
    fn reliability_computed_correctly() {
        let ts = TrustScore {
            score: 0.5,
            successes: 7,
            failures: 3,
            last_updated: unix_now(),
        };
        assert!((ts.reliability() - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn reliability_zero_when_no_operations() {
        let ts = TrustScore::default();
        assert!((ts.reliability() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn registry_get_mut_creates_default() {
        let mut reg = TrustRegistry::new();
        let ts = reg.get_mut("agent-1");
        assert!((ts.score - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn registry_most_trusted_picks_highest() {
        let mut reg = TrustRegistry::new();
        reg.get_mut("a").record_success(0.3);
        reg.get_mut("b").record_success(0.1);
        let agents = vec!["a".into(), "b".into()];
        assert_eq!(reg.most_trusted(&agents, 0.0), Some("a"));
    }

    #[test]
    fn registry_most_trusted_unknown_agents_get_default() {
        let reg = TrustRegistry::new();
        let agents = vec!["x".into(), "y".into()];
        // Both unknown → both get 0.5 → either is valid
        assert!(reg.most_trusted(&agents, 0.0).is_some());
    }

    #[test]
    fn registry_meets_threshold() {
        let mut reg = TrustRegistry::new();
        reg.get_mut("a").score = 0.8;
        assert!(reg.meets_threshold("a", 0.7, 0.0));
        assert!(!reg.meets_threshold("a", 0.9, 0.0));
        assert!(!reg.meets_threshold("unknown", 0.1, 0.0));
    }
}
