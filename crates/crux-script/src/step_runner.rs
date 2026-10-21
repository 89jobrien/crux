//! Contract-bearing asynchronous runners and the legacy capability registry.

/// Step runner ports and the legacy capability registry.
///
/// This is separate from [`HandlerRegistry`] (async, closure-based) and serves
/// as an auditable catalog of built-in step kinds with their required capabilities.
use std::{future::Future, pin::Pin, sync::LazyLock};

use miette::{IntoDiagnostic, Result, miette};
use serde_json::Value;

use crux_types::error::CruxErr;

use crate::{HandlerExecution, HandlerMetadata, HandlerOutput};

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
    /// Returns the stable handler kind registered for this runner.
    fn kind(&self) -> &'static str;
    /// Lists the execution capabilities required by this runner.
    fn required_capabilities(&self) -> Vec<RunnerCapability>;
    /// Executes the legacy runner with its alias and configuration.
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
    /// Creates an empty legacy runner registry.
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
        // --no-verify: this step runs unattended inside a pipeline, so it must
        // not execute the target repository's hooks. Hooks are arbitrary
        // commands supplied by that repo; firing them here lets an unrelated
        // repository act on the caller's environment.
        run_git(repo, &["commit", "--no-verify", "-m", message])?;
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

fn config_str_owned(config: &Value, key: &str) -> std::result::Result<String, CruxErr> {
    config
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CruxErr::step_failed(key, format!("missing string config field '{key}'")))
}

// ── Production StepRunner implementations ──────────────────────────────────
// These give the built-in runners a first-class presence in the production
// HandlerRegistry surface (see HandlerRegistry::register_builtin_runners).

impl StepRunner for ShellRunner {
    fn metadata(&self) -> &HandlerMetadata {
        static META: LazyLock<HandlerMetadata> = LazyLock::new(|| HandlerMetadata::new("shell"));
        &META
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let (_, args) = invocation.into_parts();
        Box::pin(async move {
            let cmd = match config_str_owned(&args, "cmd") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            match tokio::task::spawn_blocking(move || {
                std::process::Command::new("sh").args(["-c", &cmd]).output()
            })
            .await
            {
                Ok(Ok(output)) => {
                    HandlerExecution::free(Ok(HandlerOutput::new(serde_json::json!({
                        "exit_code": output.status.code(),
                        "stdout": String::from_utf8_lossy(&output.stdout),
                        "stderr": String::from_utf8_lossy(&output.stderr),
                    }))))
                }
                Ok(Err(e)) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("shell", e.to_string())))
                }
                Err(e) => HandlerExecution::free(Err(CruxErr::step_failed("shell", e.to_string()))),
            }
        })
    }
}

impl StepRunner for FsWriteRunner {
    fn metadata(&self) -> &HandlerMetadata {
        static META: LazyLock<HandlerMetadata> = LazyLock::new(|| HandlerMetadata::new("fs-write"));
        &META
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let (_, args) = invocation.into_parts();
        Box::pin(async move {
            let path = match config_str_owned(&args, "path") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let content = match config_str_owned(&args, "content") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let bytes = content.len();
            let path_for_result = path.clone();
            match tokio::task::spawn_blocking(move || std::fs::write(&path, &content)).await {
                Ok(Ok(())) => HandlerExecution::free(Ok(HandlerOutput::new(
                    serde_json::json!({"path": path_for_result, "bytes_written": bytes}),
                ))),
                Ok(Err(e)) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("fs-write", e.to_string())))
                }
                Err(e) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("fs-write", e.to_string())))
                }
            }
        })
    }
}

impl StepRunner for GitCommitRunner {
    fn metadata(&self) -> &HandlerMetadata {
        static META: LazyLock<HandlerMetadata> =
            LazyLock::new(|| HandlerMetadata::new("git-commit"));
        &META
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let (_, args) = invocation.into_parts();
        Box::pin(async move {
            let repo = match config_str_owned(&args, "repo") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let message = match config_str_owned(&args, "message") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let result = tokio::task::spawn_blocking(move || -> miette::Result<Value> {
                run_git(&repo, &["add", "-A"])?;
                // --no-verify: this step runs unattended inside a pipeline, so it must
                // not execute the target repository's hooks. Hooks are arbitrary
                // commands supplied by that repo; firing them here lets an unrelated
                // repository act on the caller's environment.
                run_git(&repo, &["commit", "--no-verify", "-m", &message])?;
                let commit = run_git(&repo, &["rev-parse", "HEAD"])?;
                Ok(serde_json::json!({"committed": true, "commit": commit.trim()}))
            })
            .await;
            match result {
                Ok(Ok(v)) => HandlerExecution::free(Ok(HandlerOutput::new(v))),
                Ok(Err(e)) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("git-commit", e.to_string())))
                }
                Err(e) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("git-commit", e.to_string())))
                }
            }
        })
    }
}

impl StepRunner for JsonUpdateRunner {
    fn metadata(&self) -> &HandlerMetadata {
        static META: LazyLock<HandlerMetadata> =
            LazyLock::new(|| HandlerMetadata::new("json-update"));
        &META
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let (_, args) = invocation.into_parts();
        Box::pin(async move {
            let mut document = match args.get("document").cloned() {
                Some(v) => v,
                None => {
                    return HandlerExecution::free(Err(CruxErr::step_failed(
                        "json-update",
                        "json-update requires args.document",
                    )));
                }
            };
            let pointer = match config_str_owned(&args, "pointer") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let replacement = match args.get("value").cloned() {
                Some(v) => v,
                None => {
                    return HandlerExecution::free(Err(CruxErr::step_failed(
                        "json-update",
                        "json-update requires args.value",
                    )));
                }
            };
            match document.pointer_mut(&pointer) {
                Some(slot) => {
                    *slot = replacement;
                    HandlerExecution::free(Ok(HandlerOutput::new(document)))
                }
                None => HandlerExecution::free(Err(CruxErr::step_failed(
                    "json-update",
                    format!("JSON pointer does not exist: {pointer}"),
                ))),
            }
        })
    }
}

impl StepRunner for LlmCallRunner {
    fn metadata(&self) -> &HandlerMetadata {
        static META: LazyLock<HandlerMetadata> = LazyLock::new(|| HandlerMetadata::new("llm-call"));
        &META
    }

    fn run(&self, invocation: StepInvocation) -> StepFuture<'_> {
        let (_, args) = invocation.into_parts();
        Box::pin(async move {
            let cmd = match config_str_owned(&args, "cmd") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let prompt = match config_str_owned(&args, "prompt") {
                Ok(s) => s,
                Err(e) => return HandlerExecution::free(Err(e)),
            };
            let result = tokio::task::spawn_blocking(move || {
                std::process::Command::new("sh")
                    .args(["-c", &cmd])
                    .env("CRUX_PROMPT", &prompt)
                    .output()
            })
            .await;
            match result {
                Ok(Ok(output)) if output.status.success() => {
                    HandlerExecution::free(Ok(HandlerOutput::new(serde_json::json!({
                        "response": String::from_utf8_lossy(&output.stdout),
                        "provider": "command",
                    }))))
                }
                Ok(Ok(output)) => HandlerExecution::free(Err(CruxErr::step_failed(
                    "llm-call",
                    format!(
                        "LLM provider command failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    ),
                ))),
                Ok(Err(e)) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("llm-call", e.to_string())))
                }
                Err(e) => {
                    HandlerExecution::free(Err(CruxErr::step_failed("llm-call", e.to_string())))
                }
            }
        })
    }
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

#[cfg(test)]
mod builtin_runner_tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::HandlerRegistry;

    const BUILTIN_NAMES: &[&str] = &["shell", "fs-write", "git-commit", "json-update", "llm-call"];

    #[test]
    fn handler_registry_has_no_unknown_builtin() {
        let registry = HandlerRegistry::new();
        assert!(registry.runner("unknown").is_none());
    }

    #[test]
    fn with_builtin_runners_registers_all_five() {
        let registry = HandlerRegistry::new().with_builtin_runners();
        let names: Vec<&str> = registry
            .runners()
            .map(|r| r.metadata().name.as_str())
            .collect();
        for expected in BUILTIN_NAMES {
            assert!(names.contains(expected), "missing runner: {expected}");
        }
    }

    #[test]
    fn builtin_runner_names_match_metadata() {
        let registry = HandlerRegistry::new().with_builtin_runners();
        for name in BUILTIN_NAMES {
            let runner = registry.runner(name).expect("runner should be present");
            assert_eq!(runner.metadata().name, *name);
        }
    }

    #[tokio::test]
    async fn shell_runner_executes_command() {
        let registry = Arc::new(HandlerRegistry::new().with_builtin_runners());
        let runner = registry.runner("shell").unwrap();
        let execution = runner
            .run(StepInvocation::new(
                json!(null),
                json!({"cmd": "printf hello"}),
            ))
            .await;
        let output = execution.outcome.unwrap();
        assert_eq!(output.value["stdout"], "hello");
    }

    #[tokio::test]
    async fn fs_write_runner_writes_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("written.txt");
        let registry = Arc::new(HandlerRegistry::new().with_builtin_runners());
        let runner = registry.runner("fs-write").unwrap();
        let execution = runner
            .run(StepInvocation::new(
                json!(null),
                json!({"path": path, "content": "saved"}),
            ))
            .await;
        let output = execution.outcome.unwrap();
        assert_eq!(output.value["bytes_written"], 5);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved");
    }

    #[tokio::test]
    async fn json_update_runner_mutates_document() {
        let registry = Arc::new(HandlerRegistry::new().with_builtin_runners());
        let runner = registry.runner("json-update").unwrap();
        let execution = runner
            .run(StepInvocation::new(
                json!(null),
                json!({"document": {"status": "old"}, "pointer": "/status", "value": "new"}),
            ))
            .await;
        let output = execution.outcome.unwrap();
        assert_eq!(output.value["status"], "new");
    }

    #[tokio::test]
    async fn llm_call_runner_invokes_command() {
        let registry = Arc::new(HandlerRegistry::new().with_builtin_runners());
        let runner = registry.runner("llm-call").unwrap();
        let execution = runner
            .run(StepInvocation::new(
                json!(null),
                json!({"cmd": "printf response:$CRUX_PROMPT", "prompt": "hello"}),
            ))
            .await;
        let output = execution.outcome.unwrap();
        assert_eq!(output.value["response"], "response:hello");
    }

    #[tokio::test]
    async fn git_commit_runner_creates_commit() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        let repo_arg = repo.to_str().unwrap();

        // Hermetic setup: every git call names the target path explicitly and
        // runs with global/system config neutralized, so `git init` cannot
        // discover and reinitialize an enclosing repository (e.g. when the
        // suite is run from a git worktree).
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(repo)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };

        git(&["init", repo_arg]);
        git(&["config", "user.email", "crux@example.test"]);
        git(&["config", "user.name", "Crux Test"]);
        // Keep the commit from invoking any hook from an ambient environment.
        git(&["config", "core.hooksPath", "/dev/null"]);

        std::fs::write(repo.join("file.txt"), "content").unwrap();
        let registry = Arc::new(HandlerRegistry::new().with_builtin_runners());
        let runner = registry.runner("git-commit").unwrap();
        let execution = runner
            .run(StepInvocation::new(
                json!(null),
                json!({"repo": repo_arg, "message": "test commit"}),
            ))
            .await;
        let output = execution.outcome.unwrap();
        assert_eq!(output.value["committed"], true);
        assert!(output.value["commit"].as_str().unwrap().len() >= 7);
    }
}
