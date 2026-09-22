use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::CanonicalModelId;

/// Provider pricing in integer cents per one million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCost {
    pub input_cents_per_million_tokens: u64,
    pub output_cents_per_million_tokens: u64,
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
}

impl ModelCapabilities {
    /// Return whether this model satisfies all requested capabilities.
    pub fn satisfies(&self, requirements: &ModelRequirements) -> bool {
        self.context_window >= requirements.min_context_window
            && (!requirements.requires_tools || self.supports_tools)
            && (!requirements.requires_json_mode || self.supports_json_mode)
            && (!requirements.requires_vision || self.supports_vision)
            && (requirements.allow_deprecated || !self.deprecated)
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
    pub fn select(&self, requirements: &ModelRequirements) -> Vec<CanonicalModelId> {
        self.entries
            .iter()
            .filter(|(_, capabilities)| capabilities.satisfies(requirements))
            .map(|(id, _)| id.clone())
            .collect()
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

    #[test]
    fn deprecated_models_require_explicit_opt_in() {
        let capabilities = ModelCapabilities {
            context_window: 8_192,
            supports_tools: false,
            supports_json_mode: false,
            supports_vision: false,
            cost: None,
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
                aliases: vec!["gpt-4o".into()],
                deprecated: false,
            },
        );

        let json = serde_json::to_string(&registry).unwrap();
        let decoded: ModelRegistry = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.resolve("gpt-4o"), Some(&id));
    }
}
