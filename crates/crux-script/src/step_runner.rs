/// Step runner ports and the legacy capability registry.
///
/// This is separate from [`HandlerRegistry`] (async, closure-based) and serves
/// as an auditable catalog of built-in step kinds with their required capabilities.
use std::{future::Future, pin::Pin};

use miette::{IntoDiagnostic, Result, miette};
use serde_json::Value;

use crate::{HandlerExecution, HandlerMetadata};

/// Future returned by an asynchronous [`StepRunner`].
pub type StepFuture<'a> = Pin<Box<dyn Future<Output = HandlerExecution> + Send + 'a>>;

/// One handler invocation with upstream input separated from declarative arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct StepInvocation {
    input: Value,
    args: Value,
}

impl StepInvocation {
    /// Create an invocation from upstream input and expanded step arguments.
    pub fn new(input: Value, args: Value) -> Self {
        Self { input, args }
    }

    /// Return the upstream pipeline value.
    pub fn input(&self) -> &Value {
        &self.input
    }

    /// Return the expanded declarative arguments.
    pub fn args(&self) -> &Value {
        &self.args
    }

    /// Consume the invocation into its upstream input and arguments.
    pub fn into_parts(self) -> (Value, Value) {
        (self.input, self.args)
    }
}

/// Contract-bearing asynchronous execution port for one pipeline handler.
pub trait StepRunner: Send + Sync {
    /// Return the runner's static contract and policy metadata.
    fn metadata(&self) -> &HandlerMetadata;

    /// Execute one invocation while preserving usage on success or failure.
    fn run(&self, invocation: StepInvocation) -> StepFuture<'_>;
}

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

/// Legacy synchronous runner retained until the canonical registry migration completes.
pub trait LegacyStepRunner: Send + Sync {
    fn kind(&self) -> &'static str;
    fn required_capabilities(&self) -> Vec<RunnerCapability>;
    fn run(&self, ctx: StepContext) -> Result<StepOutput>;
}

/// Registry of step runners, auditable by kind and capability.
///
/// Starts empty; call [`register_builtin_runners`](Self::register_builtin_runners)
/// to populate with the five built-in kinds.
pub struct StepRunnerRegistry {
    entries: Vec<Box<dyn LegacyStepRunner>>,
}

impl StepRunnerRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register a custom step runner.
    pub fn register(&mut self, runner: Box<dyn LegacyStepRunner>) {
        self.entries.push(runner);
    }

    /// Look up a runner by kind string. Returns `None` if no match.
    pub fn get(&self, kind: &str) -> Option<&dyn LegacyStepRunner> {
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

// ── Built-in runners ────────────────────────────────────────────────────────
// TODO(feature-idea-11): Consolidate these runners with the production HandlerRegistry surface.

pub struct ShellRunner;
impl LegacyStepRunner for ShellRunner {
    fn kind(&self) -> &'static str {
        "shell"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Shell]
    }
    fn run(&self, ctx: StepContext) -> Result<StepOutput> {
        let cmd = config_str(&ctx.config, "cmd")?;
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .output()
            .into_diagnostic()?;
        Ok(StepOutput {
            value: serde_json::json!({
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            }),
        })
    }
}

pub struct FsWriteRunner;
impl LegacyStepRunner for FsWriteRunner {
    fn kind(&self) -> &'static str {
        "fs-write"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Filesystem]
    }
    fn run(&self, ctx: StepContext) -> Result<StepOutput> {
        let path = config_str(&ctx.config, "path")?;
        let content = config_str(&ctx.config, "content")?;
        std::fs::write(path, content).into_diagnostic()?;
        Ok(StepOutput {
            value: serde_json::json!({"path": path, "bytes_written": content.len()}),
        })
    }
}

pub struct GitCommitRunner;
impl LegacyStepRunner for GitCommitRunner {
    fn kind(&self) -> &'static str {
        "git-commit"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::Git, RunnerCapability::Filesystem]
    }
    fn run(&self, ctx: StepContext) -> Result<StepOutput> {
        let repo = config_str(&ctx.config, "repo")?;
        let message = config_str(&ctx.config, "message")?;
        run_git(repo, &["add", "-A"])?;
        run_git(repo, &["commit", "-m", message])?;
        let commit = run_git(repo, &["rev-parse", "HEAD"])?;
        Ok(StepOutput {
            value: serde_json::json!({"committed": true, "commit": commit.trim()}),
        })
    }
}

pub struct JsonUpdateRunner;
impl LegacyStepRunner for JsonUpdateRunner {
    fn kind(&self) -> &'static str {
        "json-update"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![RunnerCapability::JsonMutation, RunnerCapability::Filesystem]
    }
    fn run(&self, ctx: StepContext) -> Result<StepOutput> {
        let mut document = ctx
            .config
            .get("document")
            .cloned()
            .ok_or_else(|| miette!("{} requires config.document", ctx.alias))?;
        let pointer = config_str(&ctx.config, "pointer")?;
        let replacement = ctx
            .config
            .get("value")
            .cloned()
            .ok_or_else(|| miette!("{} requires config.value", ctx.alias))?;
        let slot = document
            .pointer_mut(pointer)
            .ok_or_else(|| miette!("JSON pointer does not exist: {pointer}"))?;
        *slot = replacement;
        Ok(StepOutput { value: document })
    }
}

pub struct LlmCallRunner;
impl LegacyStepRunner for LlmCallRunner {
    fn kind(&self) -> &'static str {
        "llm-call"
    }
    fn required_capabilities(&self) -> Vec<RunnerCapability> {
        vec![
            RunnerCapability::LlmCall,
            RunnerCapability::OutputPropagation,
        ]
    }
    fn run(&self, ctx: StepContext) -> Result<StepOutput> {
        let cmd = config_str(&ctx.config, "cmd")?;
        let prompt = config_str(&ctx.config, "prompt")?;
        let output = std::process::Command::new("sh")
            .args(["-c", cmd])
            .env("CRUX_PROMPT", prompt)
            .output()
            .into_diagnostic()?;
        if !output.status.success() {
            return Err(miette!(
                "LLM provider command failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        Ok(StepOutput {
            value: serde_json::json!({
                "response": String::from_utf8_lossy(&output.stdout),
                "provider": "command"
            }),
        })
    }
}

fn config_str<'a>(config: &'a Value, key: &str) -> Result<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| miette!("missing string config field '{key}'"))
}

fn run_git(repo: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .into_diagnostic()?;
    if !output.status.success() {
        return Err(miette!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ── Conformance helper ─────────────────────────────────────────────────────

/// Assert the basic contract for any `StepRunner` implementation.
///
/// Verifies:
/// - `kind()` is non-empty
/// - `run()` does not panic
#[cfg(test)]
pub fn assert_step_runner_contract(runner: &dyn LegacyStepRunner) {
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
            let runner = registry
                .get(kind)
                .expect("runner must be findable by its own kind");
            assert_step_runner_contract(runner);
        }
    }

    #[test]
    fn builtin_runners_perform_declared_operations() {
        let temp = tempfile::tempdir().unwrap();
        let written = temp.path().join("written.txt");

        let shell = ShellRunner
            .run(StepContext {
                alias: "shell".into(),
                config: serde_json::json!({"cmd": "printf hello"}),
            })
            .unwrap();
        assert_eq!(shell.value["stdout"], "hello");

        let write = FsWriteRunner
            .run(StepContext {
                alias: "write".into(),
                config: serde_json::json!({"path": written, "content": "saved"}),
            })
            .unwrap();
        assert_eq!(write.value["bytes_written"], 5);
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "saved");

        let updated = JsonUpdateRunner
            .run(StepContext {
                alias: "json".into(),
                config: serde_json::json!({
                    "document": {"status": "old"},
                    "pointer": "/status",
                    "value": "new"
                }),
            })
            .unwrap();
        assert_eq!(updated.value["status"], "new");

        let llm = LlmCallRunner
            .run(StepContext {
                alias: "llm".into(),
                config: serde_json::json!({
                    "cmd": "printf response:$CRUX_PROMPT",
                    "prompt": "hello"
                }),
            })
            .unwrap();
        assert_eq!(llm.value["response"], "response:hello");
    }

    #[test]
    fn git_commit_runner_creates_commit() {
        let temp = tempfile::tempdir().unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "crux@example.test"],
            vec!["config", "user.name", "Crux Test"],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(temp.path())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(temp.path().join("file.txt"), "content").unwrap();

        let output = GitCommitRunner
            .run(StepContext {
                alias: "commit".into(),
                config: serde_json::json!({"repo": temp.path(), "message": "test commit"}),
            })
            .unwrap();
        assert_eq!(output.value["committed"], true);
        assert!(output.value["commit"].as_str().unwrap().len() >= 7);
    }
}
