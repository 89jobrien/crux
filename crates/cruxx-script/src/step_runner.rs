/// Step runner registry — maps step kinds to capability-declared synchronous runners.
///
/// This is separate from [`HandlerRegistry`] (async, closure-based) and serves
/// as an auditable catalog of built-in step kinds with their required capabilities.
use anyhow::Result;

/// Capabilities a step runner may require from the execution environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunnerCapability {
    Shell,
    Filesystem,
    Git,
    JsonMutation,
    LlmCall,
    OutputPropagation,
}

/// Input context passed to a step runner at execution time.
pub struct StepContext {
    pub alias: String,
    pub config: serde_json::Value,
}

/// Output produced by a step runner.
pub struct StepOutput {
    pub value: serde_json::Value,
}

/// Trait implemented by every built-in step runner.
pub trait StepRunner: Send + Sync {
    fn kind(&self) -> &'static str;
    fn required_capabilities(&self) -> Vec<RunnerCapability>;
    fn run(&self, ctx: StepContext) -> Result<StepOutput>;
}

/// Registry of step runners, auditable by kind and capability.
///
/// Starts empty; call [`register_builtin_runners`](Self::register_builtin_runners)
/// to populate with the five built-in kinds.
pub struct StepRunnerRegistry {
    entries: Vec<Box<dyn StepRunner>>,
}

impl StepRunnerRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a custom step runner.
    pub fn register(&mut self, runner: Box<dyn StepRunner>) {
        self.entries.push(runner);
    }

    /// Look up a runner by kind string. Returns `None` if no match.
    pub fn get(&self, kind: &str) -> Option<&dyn StepRunner> {
        self.entries
            .iter()
            .find(|r| r.kind() == kind)
            .map(|r| r.as_ref())
    }

    /// Return all registered `(kind, capabilities)` pairs for auditing.
    pub fn list(&self) -> Vec<(&str, Vec<RunnerCapability>)> {
        self.entries
            .iter()
            .map(|r| (r.kind(), r.required_capabilities()))
            .collect()
    }

    /// Populate the registry with the five built-in step runners.
    pub fn register_builtin_runners(&mut self) {
        self.register(Box::new(ShellRunner));
        self.register(Box::new(FsWriteRunner));
        self.register(Box::new(GitCommitRunner));
        self.register(Box::new(JsonUpdateRunner));
        self.register(Box::new(LlmCallRunner));
    }
}

impl Default for StepRunnerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Built-in runner stubs ──────────────────────────────────────────────────

pub struct ShellRunner;
impl StepRunner for ShellRunner {
    fn kind(&self) -> &'static str {
        "shell"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Shell]
    }
    fn run(&self, _ctx: StepContext) -> Result<StepOutput> {
        Ok(StepOutput {
            value: serde_json::Value::Null,
        })
    }
}

pub struct FsWriteRunner;
impl StepRunner for FsWriteRunner {
    fn kind(&self) -> &'static str {
        "fs-write"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Filesystem]
    }
    fn run(&self, _ctx: StepContext) -> Result<StepOutput> {
        Ok(StepOutput {
            value: serde_json::Value::Null,
        })
    }
}

pub struct GitCommitRunner;
impl StepRunner for GitCommitRunner {
    fn kind(&self) -> &'static str {
        "git-commit"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Git, RunnerCapability::Filesystem]
    }
    fn run(&self, _ctx: StepContext) -> Result<StepOutput> {
        Ok(StepOutput {
            value: serde_json::Value::Null,
        })
    }
}

pub struct JsonUpdateRunner;
impl StepRunner for JsonUpdateRunner {
    fn kind(&self) -> &'static str {
        "json-update"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::JsonMutation, RunnerCapability::Filesystem]
    }
    fn run(&self, _ctx: StepContext) -> Result<StepOutput> {
        Ok(StepOutput {
            value: serde_json::Value::Null,
        })
    }
}

pub struct LlmCallRunner;
impl StepRunner for LlmCallRunner {
    fn kind(&self) -> &'static str {
        "llm-call"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::LlmCall, RunnerCapability::OutputPropagation]
    }
    fn run(&self, _ctx: StepContext) -> Result<StepOutput> {
        Ok(StepOutput {
            value: serde_json::Value::Null,
        })
    }
}

// ── Conformance helper ─────────────────────────────────────────────────────

/// Assert the basic contract for any `StepRunner` implementation.
///
/// Verifies:
/// - `kind()` is non-empty
/// - `run()` does not panic
#[cfg(test)]
pub fn assert_step_runner_contract(runner: &dyn StepRunner) {
    assert!(!runner.kind().is_empty(), "runner kind must be non-empty");
    let ctx = StepContext {
        alias: "test".to_string(),
        config: serde_json::Value::Null,
    };
    let _ = runner.run(ctx); // must not panic
}

#[cfg(test)]
mod step_runner_registry_tests {
    use super::*;

    #[test]
    fn registry_get_unknown_kind_returns_none() {
        let registry = StepRunnerRegistry::new();
        assert!(registry.get("unknown").is_none());
    }

    #[test]
    fn registry_list_includes_all_registered_kinds() {
        let mut registry = StepRunnerRegistry::new();
        registry.register_builtin_runners();
        let kinds: Vec<&str> = registry.list().iter().map(|(k, _)| *k).collect();
        assert!(kinds.contains(&"shell"));
        assert!(kinds.contains(&"fs-write"));
        assert!(kinds.contains(&"git-commit"));
        assert!(kinds.contains(&"json-update"));
        assert!(kinds.contains(&"llm-call"));
    }

    #[test]
    fn shell_runner_declares_shell_capability() {
        let mut registry = StepRunnerRegistry::new();
        registry.register_builtin_runners();
        let (_, caps) = registry
            .list()
            .into_iter()
            .find(|(k, _)| *k == "shell")
            .unwrap();
        assert!(caps.contains(&RunnerCapability::Shell));
    }

    #[test]
    fn llm_call_runner_declares_output_propagation() {
        let mut registry = StepRunnerRegistry::new();
        registry.register_builtin_runners();
        let (_, caps) = registry
            .list()
            .into_iter()
            .find(|(k, _)| *k == "llm-call")
            .unwrap();
        assert!(caps.contains(&RunnerCapability::LlmCall));
        assert!(caps.contains(&RunnerCapability::OutputPropagation));
    }

    #[test]
    fn all_builtin_runners_satisfy_contract() {
        let mut registry = StepRunnerRegistry::new();
        registry.register_builtin_runners();
        for (kind, _) in registry.list() {
            let runner = registry.get(kind).expect("runner must be findable by its own kind");
            assert_step_runner_contract(runner);
        }
    }
}
