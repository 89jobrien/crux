use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::CanonicalModelId;

/// Provider pricing in integer cents per one million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input_cents_per_million_tokens: u64,
    pub output_cents_per_million_tokens: u64,
}

/// Broad cost classification used for budget-aware model routing.
///
/// Tier ordering is `Low < Medium < High`. A requirement of `max_cost_tier: Medium`
/// accepts `Low` and `Medium` models but rejects `High` ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostTier {
    /// Cheapest models (e.g. GPT-4o-mini, Claude Haiku).
    Low,
    /// Mid-tier models (e.g. GPT-4o, Claude Sonnet).
    Medium,
    /// Premium models (e.g. o1, Claude Opus).
    High,
}

/// Capabilities and lifecycle metadata used for model selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub context_window: u64,
    pub supports_tools: bool,
    pub supports_json_mode: bool,
    pub supports_vision: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<ModelCost>,
    /// Broad cost tier label for budget-aware routing. When absent, no tier
    /// constraint is applied and the model satisfies any `max_cost_tier` requirement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_tier: Option<CostTier>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub deprecated: bool,
}

/// Capability constraints supplied by planners and policies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRequirements {
    pub min_context_window: u64,
    pub requires_tools: bool,
    pub requires_json_mode: bool,
    pub requires_vision: bool,
    pub allow_deprecated: bool,
    /// Upper bound on cost tier. `None` means any tier is acceptable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_tier: Option<CostTier>,
}

impl ModelCapabilities {
    /// Return whether this model satisfies all requested capabilities.
    pub fn satisfies(&self, requirements: &ModelRequirements) -> bool {
        self.context_window >= requirements.min_context_window
            && (!requirements.requires_tools || self.supports_tools)
            && (!requirements.requires_json_mode || self.supports_json_mode)
            && (!requirements.requires_vision || self.supports_vision)
            && (requirements.allow_deprecated || !self.deprecated)
            && requirements
                .max_cost_tier
                .map_or(true, |max| self.cost_tier.map_or(true, |tier| tier <= max))
    }
}

/// In-memory capability registry with deterministic registration-order selection.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ModelRegistry {
    entries: Vec<(CanonicalModelId, ModelCapabilities)>,
    #[serde(skip)]
    aliases: HashMap<String, usize>,
}

impl<'de> Deserialize<'de> for ModelRegistry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RegistryData {
            entries: Vec<(CanonicalModelId, ModelCapabilities)>,
        }

        let data = RegistryData::deserialize(deserializer)?;
        let mut registry = Self {
            entries: data.entries,
            aliases: HashMap::new(),
        };
        registry.rebuild_aliases();
        Ok(registry)
    }
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or replace one canonical model and its aliases.
    pub fn register(&mut self, id: CanonicalModelId, capabilities: ModelCapabilities) {
        if let Some(index) = self
            .entries
            .iter()
            .position(|(registered, _)| registered == &id)
        {
            self.entries[index] = (id, capabilities);
        } else {
            self.entries.push((id, capabilities));
        }
        self.rebuild_aliases();
    }

    /// Resolve a canonical key or provider alias to its canonical model ID.
    pub fn resolve(&self, name: &str) -> Option<&CanonicalModelId> {
        self.aliases
            .get(name)
            .and_then(|index| self.entries.get(*index))
            .map(|(id, _)| id)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(id, _)| id.as_key() == name)
                    .map(|(id, _)| id)
            })
    }

    /// Look up metadata for a canonical model ID.
    pub fn capabilities(&self, id: &CanonicalModelId) -> Option<&ModelCapabilities> {
        self.entries
            .iter()
            .find(|(registered, _)| registered == id)
            .map(|(_, capabilities)| capabilities)
    }

    /// Select every registered model satisfying the requirements.
    ///
    /// Models are returned in registration order (deterministic). Use
    /// [`Self::select_first`] when only the best single match is needed.
    pub fn select(&self, requirements: &ModelRequirements) -> Vec<CanonicalModelId> {
        self.entries
            .iter()
            .filter(|(_, capabilities)| capabilities.satisfies(requirements))
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Return the first registered model satisfying the requirements.
    ///
    /// Tie-breaking rule: **registration order**. The first model registered that
    /// satisfies all constraints is selected. Register preferred models first to
    /// influence priority.
    pub fn select_first(&self, requirements: &ModelRequirements) -> Option<&CanonicalModelId> {
        self.entries
            .iter()
            .find(|(_, capabilities)| capabilities.satisfies(requirements))
            .map(|(id, _)| id)
    }

    fn rebuild_aliases(&mut self) {
        self.aliases.clear();
        for (index, (id, capabilities)) in self.entries.iter().enumerate() {
            self.aliases.insert(id.as_key(), index);
            for alias in &capabilities.aliases {
                self.aliases.insert(alias.clone(), index);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_capabilities(context_window: u64, cost_tier: Option<CostTier>) -> ModelCapabilities {
        ModelCapabilities {
            context_window,
            supports_tools: true,
            supports_json_mode: true,
            supports_vision: false,
            cost: None,
            cost_tier,
            aliases: vec![],
            deprecated: false,
        }
    }

    #[test]
    fn deprecated_models_require_explicit_opt_in() {
        let capabilities = ModelCapabilities {
            context_window: 8_192,
            supports_tools: false,
            supports_json_mode: false,
            supports_vision: false,
            cost: None,
            cost_tier: None,
            aliases: vec![],
            deprecated: true,
        };

        assert!(!capabilities.satisfies(&ModelRequirements::default()));
        assert!(capabilities.satisfies(&ModelRequirements {
            allow_deprecated: true,
            ..ModelRequirements::default()
        }));
    }

    #[test]
    fn serde_round_trip_rebuilds_alias_index() {
        let mut registry = ModelRegistry::new();
        let id: CanonicalModelId = "openai:gpt:4o:".parse().unwrap();
        registry.register(
            id.clone(),
            ModelCapabilities {
                context_window: 128_000,
                supports_tools: true,
                supports_json_mode: true,
                supports_vision: true,
                cost: None,
                cost_tier: Some(CostTier::Medium),
                aliases: vec!["gpt-4o".into()],
                deprecated: false,
            },
        );

        let json = serde_json::to_string(&registry).unwrap();
        let decoded: ModelRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.resolve("gpt-4o"), Some(&id));
    }

    #[test]
    fn cost_tier_constraint_filters_expensive_models() {
        let caps_low = make_capabilities(32_000, Some(CostTier::Low));
        let caps_medium = make_capabilities(32_000, Some(CostTier::Medium));
        let caps_high = make_capabilities(32_000, Some(CostTier::High));
        let caps_untiered = make_capabilities(32_000, None);

        let req = ModelRequirements {
            max_cost_tier: Some(CostTier::Medium),
            ..ModelRequirements::default()
        };

        assert!(caps_low.satisfies(&req), "Low satisfies max=Medium");
        assert!(caps_medium.satisfies(&req), "Medium satisfies max=Medium");
        assert!(
            !caps_high.satisfies(&req),
            "High does not satisfy max=Medium"
        );
        assert!(caps_untiered.satisfies(&req), "untiered satisfies any max");
    }

    #[test]
    fn select_first_returns_earliest_matching_registration() {
        let mut registry = ModelRegistry::new();
        let mini: CanonicalModelId = "openai:gpt:4o-mini:".parse().unwrap();
        let full: CanonicalModelId = "openai:gpt:4o:".parse().unwrap();

        registry.register(
            mini.clone(),
            make_capabilities(128_000, Some(CostTier::Low)),
        );
        registry.register(
            full.clone(),
            make_capabilities(128_000, Some(CostTier::Medium)),
        );

        // No constraint — first registered wins.
        assert_eq!(
            registry.select_first(&ModelRequirements::default()),
            Some(&mini)
        );

        // Cost constraint excludes first; second wins.
        let req = ModelRequirements {
            max_cost_tier: Some(CostTier::Medium),
            min_context_window: 128_000,
            requires_tools: true,
            ..ModelRequirements::default()
        };
        // mini also satisfies this (Low <= Medium), so it should still win.
        assert_eq!(registry.select_first(&req), Some(&mini));

        // Require High tier exactly — none match since max_cost_tier=High allows Low+Medium+High.
        // Use requires_vision to filter both out instead.
        let vision_req = ModelRequirements {
            requires_vision: true,
            ..ModelRequirements::default()
        };
        assert_eq!(registry.select_first(&vision_req), None);
    }

    #[test]
    fn select_first_returns_none_when_no_model_satisfies() {
        let registry = ModelRegistry::new();
        assert_eq!(
            registry.select_first(&ModelRequirements {
                min_context_window: 1_000_000,
                ..ModelRequirements::default()
            }),
            None
        );
    }

    #[test]
    fn cost_tier_ordering_is_low_medium_high() {
        assert!(CostTier::Low < CostTier::Medium);
        assert!(CostTier::Medium < CostTier::High);
    }
}
