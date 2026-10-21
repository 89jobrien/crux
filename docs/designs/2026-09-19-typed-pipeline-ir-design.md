# Design: Typed Pipeline IR and Contract-Bearing Step Runners

## Table of contents

- [Goal](#goal)
- [Approved Approach](#approved-approach)
- [Constraints](#constraints)
- [Context Map](#context-map)
- [Crate Ownership](#crate-ownership)
- [Architecture](#architecture)
- [Public API](#public-api)
- [Compilation Semantics](#compilation-semantics)
- [Execution Semantics](#execution-semantics)
- [CLI Integration](#cli-integration)
- [Plugin Integration](#plugin-integration)
- [Data Flow](#data-flow)
- [Hexagonal Boundaries](#hexagonal-boundaries)
- [Migration Strategy](#migration-strategy)
- [Out of Scope](#out-of-scope)
- [Risk](#risk)

## Goal

Compile untrusted `.crux` pipeline definitions into a typed, scope-checked intermediate
representation before execution so invalid references, incompatible values, incomplete contracts,
and unresolved executors fail before side effects begin.

## Approved Approach

Use a **Typed Pipeline IR** owned by `crux-script`, with `HandlerRegistry` as the canonical
registry and a redesigned, contract-bearing `StepRunner` as its object-safe execution port.

## Constraints

- Parsed `PipelineDef` and `CruxfileDef` values remain the serializable YAML-facing DTOs.
- `StepRunner` remains a public extension point, but its current synchronous signature is replaced.
- `StepRunnerRegistry` and the five null-returning built-in runner structs are removed.
- The schema language is a Crux-native recursive type system, not standard JSON Schema.
- Every shipped handler in `crux-stdlib`, `crux-agentic`, and `crux-baml` receives a contract.
- Normal compilation permits explicit dynamic boundaries with warnings.
- Strict compilation rejects missing contracts and references that cross dynamic boundaries.
- `crux check --strict` and `crux run --strict` use the same compiler and strictness rules.
- A pipeline input schema is optional in normal mode and required for every strict pipeline.
- The executor evaluates compiled expressions; it does not silently preserve unresolved templates.
- Runtime input and output values are checked against the contracts used during compilation.
- No implementation code is part of this design document.

## Context Map

### Files to Modify

| File | Responsibility | Change |
| --- | --- | --- |
| `crates/crux-script/src/metadata.rs` | Handler metadata and argument schemas | Add recursive value schemas, handler contracts, and confidence capability |
| `crates/crux-script/src/step_runner.rs` | Current duplicate synchronous runner API | Redesign `StepRunner`; remove `StepRunnerRegistry` and null runners |
| `crates/crux-script/src/registry.rs` | Handler and agent registration | Store contract-bearing runners and typed agent metadata |
| `crates/crux-script/src/compiler.rs` | New compile boundary | Compile parsed DTOs into typed IR and aggregate diagnostics |
| `crates/crux-script/src/ir.rs` | New runtime-only IR | Own typed pipelines, typed expressions, scopes, and resolved executors |
| `crates/crux-script/src/runner.rs` | Pipeline execution | Execute typed IR and enforce runtime contracts |
| `crates/crux-script/src/validator.rs` | Existing static checks | Become compatibility wrappers over compiler diagnostics |
| `crates/crux-script/src/schema.rs` | YAML DTOs | Add optional pipeline and target input schemas |
| `crates/crux-script/src/expr.rs` | Runtime expression parser/evaluator | Split parsing from evaluation and execute typed expressions |
| `crates/crux-script/src/resolve.rs` | Cruxfile target DAG | Feed complete target order into Cruxfile compilation |
| `crates/crux-script/src/output.rs` | Pipeline introspection | Traverse typed IR where executor resolution is required |
| `crates/crux-script/src/lib.rs` | Public facade | Export schemas, compiler, typed IR, and redesigned runner API |
| `crates/crux-cli/src/bin/crux/main.rs` | CLI arguments | Add first-class `check` command and shared strict semantics |
| `crates/crux-cli/src/bin/crux/check.rs` | Validation command | Compile files with the same registry and options as execution |
| `crates/crux-cli/src/bin/crux/run.rs` | Execution composition root | Compile all pipelines or targets before executing any target |
| `crates/crux-cli/src/bin/crux/registry.rs` | Registry composition | Remove successful fallback stubs and process exits |
| `crates/crux-plugin/src/protocol.rs` | Plugin wire declarations | Add optional serialized handler metadata |
| `crates/crux-plugin/src/host.rs` | Plugin declaration storage | Retain complete declarations rather than names alone |
| `crates/crux-plugin/src/bridge.rs` | Plugin adapter | Register plugin-backed `StepRunner` implementations |
| `CONFORMANCE.md` | Normative pipeline contract | Replace stale expression and validation requirements |
| `crates/crux-script/ARCHITECTURE.md` | Script architecture reference | Document compile and typed-execution layers |
| `crates/crux-script/README.md` | Public usage | Show compile, contracts, and compiled execution |

All registration modules must add complete contracts:

- `crates/crux-stdlib/src/ctrl.rs`
- `crates/crux-stdlib/src/fs.rs`
- `crates/crux-stdlib/src/git.rs`
- `crates/crux-stdlib/src/json.rs`
- `crates/crux-stdlib/src/shell.rs`
- `crates/crux-stdlib/src/text.rs`
- `crates/crux-agentic/src/analysis.rs`
- `crates/crux-agentic/src/ci.rs`
- `crates/crux-agentic/src/container.rs`
- `crates/crux-agentic/src/harness.rs`
- `crates/crux-agentic/src/llm.rs`
- `crates/crux-agentic/src/review.rs`
- `crates/crux-agentic/src/rx.rs`
- `crates/crux-agentic/src/sqlite.rs`
- `crates/crux-agentic/src/task.rs`
- `crates/crux-agentic/src/triage/classify.rs`
- `crates/crux-agentic/src/triage/env.rs`
- `crates/crux-agentic/src/triage/sync.rs`
- `crates/crux-agentic/src/triage/todo.rs`
- `crates/crux-agentic/src/triage/worktree.rs`
- `crates/crux-baml/src/extract.rs`
- `crates/crux-baml/src/planner.rs`

Generated BAML files under `crates/crux-baml/src/baml_client/` are not modified.

### Dependency Edges

```text
crux-types
    ^
crux-runtime
    ^
crux-script
    +-- crux-stdlib
    +-- crux-plugin
    +-- crux-baml
    +-- crux-agentic
    +-- crux-cli
    +-- crux facade (script feature)
```

- `crux-script` owns schemas, compilation, typed IR, and runner ports without depending on any
  concrete handler crate.
- `crux-stdlib`, `crux-agentic`, and `crux-baml` implement adapters and declare contracts using
  `crux-script` types.
- `crux-plugin` already depends on `crux-script`; adding serialized metadata creates no cycle.
- `crux-cli` remains the composition root that loads built-ins and plugins before compilation.
- The typed IR clones `Arc<dyn StepRunner>` values from the registry and is runtime-only.

### Existing Coverage

| Test area | Current coverage | Required addition |
| --- | --- | --- |
| `crates/crux-script/tests/validation.rs` | Handler args, duplicates, routes, budgets | Recursive types, scope, references, strictness, and control-flow types |
| `crates/crux-script/src/registry.rs` tests | Closure registration and usage accounting | Contract/runner atomicity and dynamic legacy adapters |
| `crates/crux-script` integration tests | Raw DTO execution for all combinators | Compile-then-execute parity and inability to bypass compilation |
| `crates/crux-agentic/tests/register_all.rs` | Built-in registration presence | Every built-in has a complete contract |
| `crates/crux-plugin/tests/protocol.rs` | Declaration round trips | Old declaration compatibility and contract round trips |
| `crates/crux-plugin/tests/bridge.rs` | Plugin invocation | Contract-bearing plugin runner registration |
| CLI integration tests | Current `--strict` handler checks | Shared check/run diagnostics and exit behavior |
| `examples/**/*.crux` | Parse and selected execution coverage | Strict compilation sweep |

Coverage gaps confirmed by the current tree:

- `Runner::run_target` executes target steps without the existing validation pass.
- No test compiles `PipelineDef` or `CruxfileDef` into a typed artifact.
- No recursive object, array, or union assignability tests exist.
- No test validates handler outputs against declared contracts.
- No test guarantees that every registered built-in has complete schemas.
- No plugin protocol test carries handler input/output contracts.
- No replay test preserves handler-reported optional confidence; typed replay must reject pipelines
  whose control flow depends on such confidence until replay metadata can preserve it.

### Reference Patterns

| File | Pattern to Follow |
| --- | --- |
| `crates/crux-script/src/schema.rs` | Recursive YAML DTOs and serde conventions |
| `crates/crux-script/src/validator.rs` | Aggregated Miette diagnostics |
| `crates/crux-script/src/registry.rs` | Boxed object-safe async callables and atomic registration helpers |
| `crates/crux-script/src/resolve.rs` | Whole-graph target validation before target execution |
| `crates/crux-runtime/src/context.rs` | Object-safe port with concrete runtime adapter |
| `crates/crux-plugin/src/protocol.rs` | Serializable plugin declaration boundary |

### Context Risks

- `StepRunner`, `StepRunnerRegistry`, `ArgType`, `ArgSchema`, `HandlerMetadata`, and
  `HandlerRegistry` are public APIs re-exported from `crux-script`.
- Changing `get_handler()` call syntax affects handler tests and external embedding code.
- The plugin `HandlerDecl` JSON shape is an external wire contract; new metadata must be optional.
- Typed IR containing trait objects cannot implement `Serialize` and must not replace the YAML DTOs.
- The current CLI injects successful stubs for unknown handlers in non-strict mode; typed execution
  must not preserve that behavior.
- Cruxfiles must compile every selected target and dependency before the first target causes a side
  effect.
- The complete migration touches six crates. Delivery is divided into bounded phases so each phase
  changes at most three crate boundaries.
- Existing unrelated modifications in `.ctx/` and `.health-baseline.json` are outside this design.

## Crate Ownership

- **Domain and compiler owner**: `crux-script` owns `ValueSchema`, contracts, `StepRunner`, typed
  expressions, typed pipeline IR, compiler diagnostics, and execution.
- **Standard adapters**: `crux-stdlib` declares contracts for deterministic local handlers.
- **Agentic adapters**: `crux-agentic` and optional `crux-baml` declare contracts for LLM,
  container, database, analysis, CI, review, and triage handlers.
- **Plugin adapter**: `crux-plugin` transports contracts and adapts subprocess invocation into
  `StepRunner`.
- **Composition and presentation**: `crux-cli` assembles the registry, selects compile options,
  and renders diagnostics.

No new crate or external dependency is introduced.

## Architecture

```text
YAML text
  -> PipelineDef / CruxfileDef                   (serializable DTO)
  -> compiler + HandlerRegistry                  (resolution and type analysis)
  -> Compilation<TypedPipeline/TypedCruxfile>    (runtime-only IR + warnings)
  -> Runner                                      (input validation and orchestration)
  -> StepRunner                                  (handler execution port)
  -> HandlerExecution                            (outcome + usage)
  -> output-contract validation
  -> Crux<Value>
```

The compiler owns all static reasoning. The executor does not look up handler names, parse template
expressions, or infer scopes. It consumes resolved typed nodes and performs only runtime value
checks that static analysis cannot prove.

The design is implemented in three architecture-safe phases on one branch:

1. `crux-script`: schemas, contracts, `StepRunner`, compiler, typed IR, and runner conversion.
2. `crux-cli` and `crux-plugin`: shared compile entry points, strict CLI behavior, and plugin
   metadata transport.
3. `crux-stdlib`, `crux-agentic`, and `crux-baml`: complete built-in contract migration and strict
   example conformance.

These are development checkpoints, not independently releasable states. No phase is merged or
released until all built-in contracts and strict gates are complete.

## Public API

### Recursive Value Schemas

In `crux-script::metadata`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", content = "definition", rename_all = "snake_case")]
pub enum ValueSchema {
    Dynamic,
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array { items: Box<ValueSchema> },
    Object(ObjectSchema),
    Union { variants: Vec<ValueSchema> },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectSchema {
    properties: BTreeMap<String, SchemaProperty>,
    additional: Option<Box<ValueSchema>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaProperty {
    schema: ValueSchema,
    required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array,
    Object,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaViolationKind {
    #[error("got {actual}")]
    TypeMismatch { actual: ValueKind },
    #[error("required property is missing")]
    MissingRequiredProperty,
    #[error("additional property is not allowed")]
    AdditionalPropertyNotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("schema mismatch at {path}: expected {expected}, {kind}")]
pub struct SchemaViolation {
    pub path: String,
    pub expected: ValueSchema,
    pub kind: SchemaViolationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaBuildError {
    #[error("union schema must contain at least one variant")]
    EmptyUnion,
}
```

Construction and validation methods:

```rust
impl ValueSchema {
    pub fn array(items: ValueSchema) -> Self;
    pub fn object(schema: ObjectSchema) -> Self;
    pub fn union(
        variants: impl IntoIterator<Item = ValueSchema>,
    ) -> Result<Self, SchemaBuildError>;
    pub fn validate_definition(&self) -> Result<(), SchemaBuildError>;
    pub fn is_assignable_from(&self, source: &ValueSchema) -> bool;
    pub fn validate(&self, value: &Value) -> Result<(), SchemaViolation>;
}

impl ObjectSchema {
    pub fn new() -> Self;
    pub fn required(self, name: impl Into<String>, schema: ValueSchema) -> Self;
    pub fn optional(self, name: impl Into<String>, schema: ValueSchema) -> Self;
    pub fn additional(self, schema: ValueSchema) -> Self;
    pub fn property(&self, name: &str) -> Option<&SchemaProperty>;
    pub fn additional_schema(&self) -> Option<&ValueSchema>;
}

impl SchemaProperty {
    pub fn schema(&self) -> &ValueSchema;
    pub fn is_required(&self) -> bool;
}

impl fmt::Display for ValueSchema;
impl fmt::Display for ValueKind;
```

`None` in `ObjectSchema::additional` means a closed object. `Some(ValueSchema::Dynamic)` means
unknown extra fields are accepted, but strict expressions may not traverse them.

Assignability is deterministic:

- Exact scalar kinds are assignable to themselves; `Integer` is also assignable to `Number`.
- A `Dynamic` target accepts every source. A `Dynamic` source is assignable only to a `Dynamic`
  target; permissive compilation may continue across it with a warning, while strict compilation
  rejects any attempted traversal or narrowing.
- Arrays are covariant in their item schema.
- Every source union variant must be assignable to at least one target union variant.
- Union construction flattens nested unions, removes duplicate variants, collapses one-member
  unions, and rejects empty unions.
- A source object must contain every target-required property with an assignable schema. A source
  optional property cannot satisfy a target-required property.
- For every property declared by both source and target, the source property's schema must be
  assignable to the target property's schema, regardless of whether either declaration is optional.
- If the target object is closed, the source must be closed and may not declare extra properties.
  If the target allows additional properties, each source-only property and the source additional
  schema must be assignable to the target additional schema.
- Runtime value validation follows the same scalar, array, object, required-property, additional-
  property, and union rules as static assignability.

`ValueSchema::union` is fallible. Serde remains permissive for backward-compatible decoding, so
registry insertion and pipeline compilation call `validate_definition` and reject empty unions
before assignability or execution.

`ArgType` remains as a primitive convenience and converts into the recursive schema. `ArgSchema`
retains its builder shape, while `ArgSpec` stores a `ValueSchema` instead of a flat `ArgType`:

```rust
impl From<ArgType> for ValueSchema;

pub struct ArgSpec {
    pub name: String,
    pub schema: ValueSchema,
    pub required: bool,
    pub description: Option<String>,
}

impl ArgSpec {
    pub fn required(name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self;
    pub fn optional(name: impl Into<String>, schema: impl Into<ValueSchema>) -> Self;
    pub fn describe(self, description: impl Into<String>) -> Self;
}

impl ArgSchema {
    pub fn required(
        self,
        name: impl Into<String>,
        schema: impl Into<ValueSchema>,
    ) -> Self;
    pub fn optional(
        self,
        name: impl Into<String>,
        schema: impl Into<ValueSchema>,
    ) -> Self;
}
```

### Handler Contracts

`HandlerMetadata` remains the serializable contract and policy record. Optional schema fields
distinguish a missing legacy contract from an explicitly dynamic contract.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceCapability {
    Never,
    Optional,
    Always,
}

pub struct HandlerMetadata {
    pub name: String,
    pub description: String,
    pub args: ArgSchema,
    pub input_schema: Option<ValueSchema>,
    pub output_schema: Option<ValueSchema>,
    pub confidence: Option<ConfidenceCapability>,
    pub risk: RiskLevel,
    pub side_effects: Vec<SideEffect>,
    pub capabilities: Vec<Capability>,
    pub deterministic: bool,
}

impl HandlerMetadata {
    pub fn input_schema(self, schema: ValueSchema) -> Self;
    pub fn output_schema(self, schema: ValueSchema) -> Self;
    pub fn confidence(self, capability: ConfidenceCapability) -> Self;
    pub fn has_complete_contract(&self) -> bool;
}
```

### Contract-Bearing Step Runner

`StepRunner` replaces the closure type as the canonical execution port. Its boxed future keeps the
trait object-safe without adding an async-trait dependency. The invocation boundary separates the
upstream pipeline value from declarative `args`; `HandlerMetadata::input_schema` describes only
`StepInvocation::input`, while `ArgSchema` describes only `StepInvocation::args`.

```rust
pub type StepFuture<'a> =
    Pin<Box<dyn Future<Output = HandlerExecution> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq)]
pub struct StepInvocation {
    input: Value,
    args: Value,
}

impl StepInvocation {
    pub fn new(input: Value, args: Value) -> Self;
    pub fn input(&self) -> &Value;
    pub fn args(&self) -> &Value;
    pub fn into_parts(self) -> (Value, Value);
}

pub trait StepRunner: Send + Sync {
    fn metadata(&self) -> &HandlerMetadata;
    fn run(&self, invocation: StepInvocation) -> StepFuture<'_>;
}
```

`StepRunnerRegistry`, `RunnerCapability`, `StepContext`, `StepOutput`, `ShellRunner`,
`FsWriteRunner`, `GitCommitRunner`, `JsonUpdateRunner`, and `LlmCallRunner` are removed. Closure
registration methods remain as adapters that construct an internal `ClosureStepRunner`.
That adapter converts `StepInvocation` to the historical JSON envelope: object inputs gain an
`args` property, while scalar inputs become `{ "input": value, "args": args }`. Native
`StepRunner` implementations consume the separated values directly.

### Agent Contracts

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: String,
    pub input_schema: Option<ValueSchema>,
    pub output_schema: Option<ValueSchema>,
}

impl AgentMetadata {
    pub fn new(name: impl Into<String>) -> Self;
    pub fn input_schema(self, schema: ValueSchema) -> Self;
    pub fn output_schema(self, schema: ValueSchema) -> Self;
    pub fn has_complete_contract(&self) -> bool;
}
```

Existing `agent` and `agent_fn` methods remain dynamic compatibility adapters. Contract-bearing
registration uses:

```rust
pub fn agent_with_metadata<A>(
    &mut self,
    metadata: AgentMetadata,
) -> Result<(), RegistryError>
where
    A: Agent<Input = Value, Output = Value>;

pub fn agent_fn_with_metadata<F, Fut>(
    &mut self,
    metadata: AgentMetadata,
    handler: F,
) -> Result<(), RegistryError>
where
    F: Fn(Value) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, CruxErr>> + Send + 'static;
```

### Canonical Registry

```rust
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("step runner '{name}' is already registered")]
    DuplicateRunner { name: String },
    #[error("agent '{name}' is already registered")]
    DuplicateAgent { name: String },
    #[error("registered name '{registered}' does not match metadata name '{metadata}'")]
    MetadataNameMismatch {
        registered: String,
        metadata: String,
    },
    #[error("invalid schema for '{name}': {source}")]
    InvalidSchema {
        name: String,
        source: SchemaBuildError,
    },
}

pub struct HandlerRegistry {
    runners: HashMap<String, Arc<dyn StepRunner>>,
    agents: HashMap<String, RegisteredAgent>,
}

impl HandlerRegistry {
    pub fn register<R>(&mut self, runner: R) -> Result<(), RegistryError>
    where
        R: StepRunner + 'static;

    pub fn register_arc(
        &mut self,
        runner: Arc<dyn StepRunner>,
    ) -> Result<(), RegistryError>;
    pub fn runner(&self, name: &str) -> Option<Arc<dyn StepRunner>>;
    pub fn runners(&self) -> impl Iterator<Item = &dyn StepRunner>;
    pub fn agent_metadata(&self, name: &str) -> Option<&AgentMetadata>;
    pub(crate) fn agent_binding(&self, name: &str) -> Option<RegisteredAgent>;
}
```

`RegisteredAgent` is `pub(crate)` and `Clone`; `ClosureStepRunner` remains private. The compiler
uses `agent_binding` to place a resolved agent executor in typed IR without exposing that callable
through the public API. Existing handler registration
methods keep their generic closure inputs, wrap closures in `ClosureStepRunner`, and change their
return type to `Result<(), RegistryError>`. Methods without metadata create incomplete contracts
that are valid only in permissive compilation. Agent registration methods also return
`Result<(), RegistryError>`.

`register_metadata` and `get_metadata` are removed. Metadata is inseparable from its runner and is
read through `HandlerRegistry::runner(name).metadata()`. Registration never silently replaces an
existing runner or agent; callers must construct a new registry to select a different adapter.

### Compiler Options and Diagnostics

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CompileMode {
    #[default]
    Permissive,
    Strict,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompileOptions {
    mode: CompileMode,
}

impl CompileOptions {
    pub const fn permissive() -> Self;
    pub const fn strict() -> Self;
    pub const fn mode(self) -> CompileMode;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationCode {
    DuplicateName,
    UnknownHandler,
    UnknownAgent,
    MissingContract,
    MissingInputSchema,
    InvalidArguments,
    InvalidExpression,
    UnknownReference,
    ForwardReference,
    InvalidScope,
    TypeMismatch,
    DynamicBoundary,
    InvalidControlFlow,
    InvalidRoute,
    InvalidBudget,
    TargetResolution,
}
```

`ValidationDiagnostic` gains `pub code: ValidationCode`. Existing severity, logical location, and
message fields remain.

### Compilation Results and Typed Artifacts

```rust
#[derive(Debug, Clone)]
pub struct Compilation<T> {
    artifact: Option<T>,
    diagnostics: Vec<ValidationDiagnostic>,
}

impl<T> Compilation<T> {
    pub fn artifact(&self) -> Option<&T>;
    pub fn diagnostics(&self) -> &[ValidationDiagnostic];
    pub fn error_count(&self) -> usize;
    pub fn warning_count(&self) -> usize;
    pub fn is_ok(&self) -> bool;
    pub fn is_executable(&self) -> bool;
    pub fn into_artifact(self) -> Option<T>;
    pub fn into_parts(self) -> (Option<T>, Vec<ValidationDiagnostic>);
}

#[derive(Clone)]
pub struct TypedPipeline {
    pub(crate) name: String,
    pub(crate) input_schema: Option<ValueSchema>,
    pub(crate) steps: Vec<TypedStep>,
    pub(crate) budget: Option<BudgetDef>,
    pub(crate) display: Option<PipelineDisplayDef>,
}

#[derive(Clone)]
pub struct TypedCruxfile {
    pub(crate) project: String,
    pub(crate) default_target: String,
    pub(crate) targets: IndexMap<String, TypedTarget>,
}

impl TypedPipeline {
    pub fn name(&self) -> &str;
    pub fn input_schema(&self) -> Option<&ValueSchema>;
}

impl TypedCruxfile {
    pub fn project(&self) -> &str;
    pub fn default_target(&self) -> &str;
    pub fn contains_target(&self, name: &str) -> bool;
}

pub fn compile_pipeline(
    definition: &PipelineDef,
    registry: &HandlerRegistry,
    options: CompileOptions,
) -> Compilation<TypedPipeline>;

pub fn compile_cruxfile(
    definition: &CruxfileDef,
    registry: &HandlerRegistry,
    options: CompileOptions,
) -> Compilation<TypedCruxfile>;
```

`TypedStep`, `TypedExpression`, `TypedTarget`, scope frames, and executor bindings are
`pub(crate)` IR types. The `TypedPipeline` and `TypedCruxfile` fields shown above are also
`pub(crate)`, allowing sibling compiler and runner modules to construct and inspect them without
exposing their representation downstream. The public artifacts implement manual `Debug` that
omits trait-object internals; they deliberately do not implement `Serialize` or `Deserialize`.

Compilation always returns one diagnostic collection. Any error produces `artifact: None`.
Warnings normally accompany `artifact: Some`, except unresolved external executors in permissive
mode: those produce warnings and `artifact: None`. This lets `check` report aspirational/plugin
references while ensuring no executor can run unresolved IR. Strict mode reports the same
condition as an error. `ValidationReport` remains a compatibility view over the compilation's
diagnostics rather than a second error channel.

### Pipeline DTO Additions

```rust
pub struct PipelineDef {
    pub pipeline: String,
    pub input_schema: Option<ValueSchema>,
    pub budget: Option<BudgetDef>,
    pub vars: Option<IndexMap<String, Value>>,
    pub display: Option<PipelineDisplayDef>,
    pub steps: Vec<StepDef>,
}

pub struct TargetDef {
    pub depends: Vec<String>,
    pub budget: Option<BudgetDef>,
    pub steps: Vec<StepDef>,
}
```

`PipelineDef::input_schema` uses `#[serde(default)]`, preserving existing YAML parsing. Cruxfile
targets retain their current null-input semantics; target inputs and inter-target value flow are
not introduced by this design.

### Runner API

```rust
impl Runner {
    pub async fn run(&self, pipeline: &PipelineDef, input: Value) -> Crux<Value>;

    pub async fn run_compiled(
        &self,
        pipeline: &TypedPipeline,
        input: Value,
    ) -> Crux<Value>;

    pub async fn run_with_replay(
        &self,
        pipeline: &PipelineDef,
        input: Value,
        previous: &Crux<Value>,
        mode: ReplayMode,
    ) -> Crux<Value>;

    pub async fn run_compiled_with_replay(
        &self,
        pipeline: &TypedPipeline,
        input: Value,
        previous: &Crux<Value>,
        mode: ReplayMode,
    ) -> Crux<Value>;

    pub async fn run_target(
        &self,
        cruxfile: &TypedCruxfile,
        target: &str,
    ) -> Crux<Value>;
}
```

`run` and `run_with_replay` are permissive compile-and-run compatibility wrappers.
`run_unchecked` is removed. `run_compiled`, `run_compiled_with_replay`, and `run_target` execute
only artifacts whose executor bindings resolved during compilation.

## Compilation Semantics

### Variables and Lexical Scope

1. Variables compile in declaration order.
2. A variable may reference pipeline input or an earlier variable.
3. Self-references, forward references, unknown variables, and cycles are errors.
4. Top-level steps compile sequentially and may reference only prior visible steps.
5. Nested loop bodies inherit outer variables and prior outer steps.
6. A loop body additionally receives typed `iter.index` and its declared item binding.
7. Nested step names remain local to the loop and do not leak after the loop.
8. Pipe, join, route, and speculation arm labels remain internal; only the enclosing combinator
   name enters the outer scope.

Typed expressions reference compiler-assigned binding IDs rather than runtime strings. Execution
uses a stack of runtime scope frames keyed by those IDs. Entering an iteration pushes a frame;
leaving it removes every nested value before the enclosing loop result is bound. Repeated loop
iterations therefore cannot overwrite or leak bindings into outer scopes.

### Expression Typing

- Exact `{{ path }}` templates preserve the referenced schema.
- Interpolated strings containing one or more templates produce `ValueSchema::String`.
- `input.<field>` resolves through the declared input schema.
- Bare `input` resolves to the complete declared input schema. Strict compilation requires that
  schema even when no field path is used, because the value flows into the first step.
- `vars.<name>` resolves through the inferred variable schema.
- `steps.<name>.output` resolves to the step output schema.
- `steps.<name>.output.<path>` requires every path segment to exist.
- `steps.<name>.confidence` requires `ConfidenceCapability::Always` in strict mode. Optional
  confidence emits a warning in permissive mode.
- `iter.index` is an integer; `iter.<binding>` is the `for_each` array item schema.
- Loop conditions, `until`, and `break_if` must be boolean.
- Route confidence expressions must be numeric.
- `for_each.items` must be an array.

### Combinator Output Types

- A simple step uses its handler output schema.
- A pipe uses the final stage output and checks every stage output against the next stage input.
- A join produces an array whose item schema is the union of arm outputs.
- A route produces the union of branch outputs and requires complete, non-overlapping confidence
  coverage.
- Speculation produces the union of arm outputs.
- `poll` executes at least once and produces its body output.
- `while`, `repeat`, and `for_each` may execute zero times, so their output is the union of incoming
  input and body output.
- `on_error` adds the fallback handler output to the primary result union.
- `allow_failure` adds the documented `{ status: string, error: string }` failure object to the
  result union.

Confidence capability propagates independently from value schemas:

- A simple step inherits its runner capability; `on_error` or `allow_failure` weakens `Always` to
  `Optional` because recovery paths carry no confidence.
- A pipe inherits its final stage capability.
- A non-empty join is `Always` only when every arm is `Always`, `Never` only when every arm is
  `Never`, and `Optional` otherwise.
- A confidence route is `Always` because runtime falls back to the routing score when the selected
  handler reports none.
- Delegation, speculation, and loop nodes are `Never` under current runtime semantics.
- `SpeculateMode::PickBest` additionally requires every arm output to contain a required numeric
  `score` property. Permissive mode warns at dynamic boundaries; strict mode rejects them.

### Static Errors

Compilation rejects malformed templates, duplicate bindings, future-step references, illegal
scope access, incompatible handler input, invalid argument schemas, empty combinators, route gaps
or overlaps, impossible unions, and references unavailable on every control-flow path.

The current DSL has no early-return node, so ordinary sequential steps remain reachable. The
compiler detects only reachability that is statically expressible through existing route and loop
constructs.

## Execution Semantics

1. `Runner::run` compiles permissively and converts compiler errors or a missing executable
   artifact into a failed `Crux<Value>` before invoking a handler.
2. `run_compiled` validates the actual pipeline input against `input_schema` when present.
3. Each typed node already contains its resolved `Arc<dyn StepRunner>` or agent binding.
4. Before invocation, the runner validates `StepInvocation::input` against `input_schema` and
   `StepInvocation::args` against `ArgSchema` independently.
5. The runner calls `StepRunner::run` and preserves existing `HandlerExecution` usage accounting.
6. Runtime enforces confidence contracts: `Always` requires `Some`, `Never` requires `None`, and
   `Optional` accepts either.
7. A successful output is validated against the declared output schema before entering expression
   state or flowing to another step.
8. Contract violations become `CruxErr::StepFailed` values containing the executor name, JSON path,
   expected schema, and violation kind.
9. Runtime evaluates pre-parsed typed expressions through binding-ID scope frames and never
   reparses YAML template strings.
10. Cruxfile compilation validates every target and precomputes dependency order. `run_target`
    executes the selected target closure only after the entire Cruxfile produced an executable
    artifact; every target retains the current `Value::Null` input.
11. Replay remains available for pipelines whose typed expressions do not read handler-reported
    confidence. `run_compiled_with_replay` rejects confidence-dependent pipelines before seeding
    replay because current traces cache values but cannot restore optional handler confidence.

## CLI Integration

Add a first-class command while retaining `crux run --check` as a compatibility alias:

```text
crux check [--strict] [--plugins <path>] <paths>...
crux run [--strict] [--plugins <path>] <pipeline-or-cruxfile>
```

- `check` and `run` build the same registry, including the same plugin path.
- `--strict` expands from "reject unregistered names" to the complete strict compilation policy.
- Normal `check` may exit successfully with warnings and no executable artifact for unavailable
  external handlers.
- Normal and strict `run` both require an executable artifact.
- Cruxfile checks compile every target, not only the selected default. Execution uses the already
  compiled dependency closure for the selected target.
- Diagnostics retain logical YAML paths such as `steps[2].args.cmd`; exact line and column spans are
  not promised.
- The CLI no longer injects successful handler or agent stubs.
- Registry construction returns errors to the caller instead of calling `std::process::exit`.

## Plugin Integration

`HandlerDecl` gains optional metadata so old plugin declarations remain decodable:

```rust
pub struct HandlerDecl {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HandlerMetadata>,
}
```

- The bridge verifies that `metadata.name` equals `HandlerDecl.name`.
- Duplicate plugin names or collisions with built-ins return `RegistryError`; plugins never replace
  an existing executor implicitly.
- Missing metadata creates a plugin runner with an incomplete contract.
- Permissive compilation treats that runner as dynamic and warns.
- Strict compilation rejects it.
- Output validation applies to plugin runners exactly as it does to built-in runners.
- Request correlation, streaming, cancellation, metered plugin usage, and protocol negotiation remain
  separate protocol work.

## Data Flow

1. Source: `serde-saphyr` parses YAML into serializable pipeline or Cruxfile DTOs.
2. Transform: the compiler resolves registry names, parses expressions, builds lexical scopes,
   propagates recursive schemas, and aggregates diagnostics.
3. Intermediate sink: successful compilation returns runtime-only typed IR containing resolved
   executor handles.
4. Runtime source: the runner validates actual input and invokes the executor already stored in the
   typed node.
5. Runtime transform: the executor returns `HandlerExecution`; the runner validates its output and
   records usage.
6. Final sink: validated values update typed expression state and produce the final `Crux<Value>`.

## Hexagonal Boundaries

- **Port**: `StepRunner` in `crux-script::step_runner` defines contract-bearing async step
  execution.
- **Closure adapter**: private `ClosureStepRunner` in `crux-script::registry` preserves ergonomic
  Rust closure registration.
- **Plugin adapter**: `crux-plugin::bridge` implements `StepRunner` through `PluginHost`.
- **Compiler domain**: `crux-script::compiler` depends only on `HandlerRegistry` contracts and
  parsed DTOs, never concrete infrastructure crates.
- **Presentation adapter**: `crux-cli` translates compilation diagnostics into human or machine
  output and exit codes.

## Migration Strategy

1. Introduce schemas, optional metadata fields, typed IR, and compiler while retaining permissive
   closure registration methods.
2. Redesign `StepRunner`, convert `HandlerRegistry`, and migrate script tests from callable handler
   closures to `runner.run(StepInvocation::new(input, args))`.
3. Route existing `validate_pipeline` and `validate_cruxfile` through compiler diagnostics.
4. Convert `Runner` internals to typed IR and remove `run_unchecked`.
5. Convert CLI check/run and remove successful fallback stubs.
6. Extend optional plugin declarations and bridge plugin runners.
7. Annotate every built-in handler and agent contract.
8. Add input schemas to every maintained pipeline compiled in strict mode.
9. Strict-compile all maintained examples in CI.
10. Update `CONFORMANCE.md`, `crux-script` and plugin architecture/reference documentation, and
    release notes.

## Out of Scope

- Standard JSON Schema interoperability.
- Deriving schemas automatically from Rust types or macros.
- Persisting or serializing typed IR.
- Persistent compiled-IR caching.
- Exact YAML line and column spans or replacing `serde-saphyr`.
- Parallel `for_each` execution.
- Runtime delegation redesign or child-trace budget propagation.
- Extending trace and replay formats to persist optional handler-reported confidence; replay of
  confidence-dependent typed pipelines is rejected until that separate wire change lands.
- Plugin request IDs, protocol negotiation, streaming, cancellation, or usage metering.
- Preserving the old synchronous `StepRunner` method signatures.
- Compatibility aliases for unused stub names such as `fs-write` and `llm-call`.

## Risk

- [x] **Breaking public API**: `StepRunner` changes shape; `StepRunnerRegistry` and the null runner
  types are removed; handler lookup changes from callable closures to runner objects.
- [x] **Behavioral change**: normal execution no longer substitutes successful stubs for unknown
  handlers or agents.
- [x] **Serialization change**: pipeline and plugin DTOs gain optional fields with serde defaults;
  existing documents remain decodable.
- [x] **Wire compatibility risk**: new plugin metadata is optional, but plugins using it must share
  the same tagged `ValueSchema` representation.
- [x] **Contract accuracy risk**: incorrect built-in schemas can reject valid values or admit invalid
  ones; registration completeness and runtime contract tests are release gates.
- [x] **Scope risk**: adapter migration spans six crates, mitigated by three bounded delivery phases.
- [ ] **Circular dependency**: none introduced; `crux-script` remains below all concrete adapters.
- [ ] **New external dependency**: none.
- [ ] **Feature flag required**: none for core compilation; existing BAML optionality remains.
- [ ] **Typed IR wire migration**: none; typed artifacts are runtime-only.
