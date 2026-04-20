# crux-planner Design

## Overview

`crux-planner` is a goal-to-pipeline generation system that translates natural language goals or
structured intent specifications into executable YAML pipelines. It sits between human intent and
`crux-script` execution, automating the composition of steps from the `crux-agentic` handler
registry.

### Goals

1. **Natural Language to YAML**: Accept unstructured goals ("Summarize and extract entities from
   this document") and generate valid `crux-script` YAML pipelines.
2. **Intent Modeling**: Support structured input (goal object with constraints, preferences, budget
   allocation) for repeatability and testing.
3. **Static vs Dynamic Output**: Design both modes:
   - **Static**: Planner generates a fixed YAML file, user executes via `crux-run`.
   - **Dynamic**: Planner generates pipelines at runtime, executed immediately without disk
     artifacts.
4. **Handler Integration**: Understand the `crux-agentic` handler registry (shell, fs, git, json,
   llm modules) and compose them correctly.
5. **Composition Rules**: Model how handlers chain (output of one as input to next), budget
   allocation, and error recovery.

---

## Architecture

### Two Implementation Paths

#### Path A: YAML Spec + BAML Function (Recommended)

Define a YAML schema for planner input and use BAML to generate executable YAML:

```yaml
# planner-input.yaml
goal: "Summarize a text file and extract key entities"
input_path: "document.txt"
constraints:
  - budget:
      calls: 3
      tokens: 4000
  - prefer_error_recovery: true
preferences:
  - handler_order: [fs::read, llm::extract, json::write]
  - summarize_before_extract: true
```

BAML function (e.g., `GeneratePipeline`):

```baml
function GeneratePipeline(goal: string, input: PlannerIntent) -> PipelineYaml {
  client: "openai"
  prompt: #"
    Given the user goal and constraints, generate a crux-script YAML pipeline.

    Goal: {{ goal }}
    Constraints: {{ input.constraints }}
    Handler Registry: [fs::read, fs::write, llm::invoke, llm::extract,
                       json::parse, json::write, shell::capture, ...]

    Output: Valid BAML-generated YAML following crux-script schema.
  "#
}
```

**Pros:**
- Reuses existing BAML infrastructure (crux-agentic already integrated).
- Familiar to users (YAML in, YAML out).
- Easy testing: golden YAML snapshots.

**Cons:**
- Requires LLM call for every plan generation (latency, cost).
- Less control over composition rules (LLM may hallucinate handlers).

#### Path B: New Crate (`crux-planner`)

Implement a Rust crate with:
- Goal/Intent AST
- Handler registry query interface
- Composition rules engine
- Code generation to YAML

```rust
use crux_planner::{Goal, Planner, PlannerConfig};

let goal = Goal::new("Summarize document and extract entities");
let planner = Planner::new(PlannerConfig::default());
let pipeline = planner.plan(&goal)?;

// Write to YAML or execute directly
crux_script::execute(&pipeline, input).await?
```

**Pros:**
- Deterministic (no LLM, no latency).
- Composable (rules expressed as Rust code).
- Replayable (same input always produces same pipeline).

**Cons:**
- New crate to maintain.
- Harder to extend (requires code changes vs. prompt tuning).
- Less flexible for novel compositions.

---

## Interaction with crux-script

### ArmDef and StepNode Schema

Crux-script defines pipeline steps via `ArmDef` (step definition) and `StepNode` (runtime step
node). Planner must respect:

1. **Handler Invocation**: Each handler has a name (`module::handler`), required/optional args.
2. **Step Ordering**: Steps execute sequentially or fan-out via `join_all`.
3. **Data Flow**: Output of step N becomes input to step N+1, unless explicitly routed.
4. **Pipe Stages**: Multi-stage transforms within a single step (e.g., `pipe([extract, validate,
   format])`).
5. **Speculate Arms**: Alternative paths picked by `pick_best_by` or `first_ok`.

### Integration Points

- **Handler Registry Query**: Planner reads handler names, signatures, and descriptions from the
  registry (or from a manifest).
- **Budget Allocation**: Planner must respect user budget and distribute across steps
  (e.g., 3 steps with 1000 tokens each, or 1 step with 2000).
- **Static Args Injection**: User may provide static args at plan time; planner embeds them in
  the generated YAML.
- **Replay and Recovery**: Generated pipelines should be replay-safe; planner avoids non-determinism
  (no random handler selection) and includes recovery hooks if user specifies.

---

## Input Schema

### Natural Language Goal (Simple)

```yaml
goal: "Process a log file: extract errors, deduplicate, count by type"
```

### Structured Intent (Complex)

```yaml
intent:
  goal: "Summarize a research paper and extract key concepts"

  # Input specification
  input_source:
    type: file
    path: "research_paper.pdf"  # Planner may infer fs::read, pdf::extract

  # Output specification
  output_destination:
    type: file
    path: "summary.json"
    format: json

  # Constraints
  constraints:
    budget:
      calls: 2
      tokens: 5000
    timeout_seconds: 30
    retry_on_failure: true

  # Preferences
  preferences:
    use_streaming: false
    prefer_structured_output: true
    batch_size: 10
```

### Composition Hints (Optional)

```yaml
hints:
  handler_sequence: [fs::read, llm::extract, json::write]
  prefer_speculate_on: [extract]  # Offer multiple extraction styles
  error_recovery: escalate_to_human
```

---

## Output Schema

Generated YAML follows crux-script structure:

```yaml
pipeline: my_pipeline_name
budget: { calls: 2, tokens: 5000 }

steps:
  - step: read
    handler: fs::read
    path: input.txt

  - step: summarize
    handler: llm::extract
    function: Summarize
    input:
      text: "$step:read"  # Reference previous step output
      max_sentences: 3

  - step: output
    handler: json::write
    content:
      summary: "$step:summarize.summary"
      key_points: "$step:summarize.key_points"
    path: output.json
```

---

## Static vs Dynamic Modes

### Static Mode

**Input:** NL goal or structured intent YAML
**Output:** `.crux` file on disk
**Execution:** User runs `crux-run <generated>.crux <input>.json`

```bash
crux-planner plan --goal "summarize and extract" --output my_pipeline.crux
crux-run my_pipeline.crux input.json
```

**Use case:** One-off scripts, reusable templates, version control.

### Dynamic Mode

**Input:** Intent object (struct or YAML)
**Output:** In-memory `Pipeline` value
**Execution:** Planner calls `crux_script::execute()` directly, or returns pipeline for caller to
execute

```rust
let intent = Intent::new("summarize and extract");
let (pipeline, output) = planner.plan_and_execute(&intent, input).await?;
```

**Use case:** REPL, interactive agents, runtime composition.

---

## Composition Rules Engine

### Core Rules

1. **Type Compatibility**: Output type of step N must match input type of step N+1.
   - E.g., `fs::read` returns `String` → valid input to `llm::extract` (expects `{function, input}`)
   - Invalid: `json::parse` output `Value` → `fs::write` input path (expects string).

2. **Handler Dependency Graph**: Model which handlers depend on which (e.g., `llm::extract`
   requires BAML feature, OPENAI_API_KEY env var).

3. **Budget Propagation**: Scoped budgets per step and delegation.
   - Total pipeline budget = sum of step budgets (or auto-allocate if not specified).

4. **Data Flow Semantics**: Explicit routing vs implicit chaining.
   - Implicit: each step's output becomes the next step's input.
   - Explicit: user specifies input bindings (via `$step:name` references).

5. **Fallback Chains**: If a handler fails, offer recovery options (skip, retry, substitute,
   escalate).

### Example Composition

```yaml
# Goal: Analyze a git commit, extract changes, summarize per file

steps:
  - step: checkout
    handler: git::checkout
    ref: HEAD

  - step: list_changes
    handler: git::diff
    base_ref: origin/main

  - step: summarize_changes    # speculate: try 2 summarization approaches
    handler: llm::extract
    speculate:
      - name: by_file
        input:
          function: SummarizeByFile
          input: "$step:list_changes"
      - name: overall
        input:
          function: SummarizeDiff
          input: "$step:list_changes"
    pick_best_by: confidence

  - step: output
    handler: json::write
    content: "$step:summarize_changes"
    path: summary.json
```

---

## Implementation Roadmap

### Phase 1: Design & Spec (Now)
- [ ] Finalize input/output schemas (YAML + Rust enums)
- [ ] Model handler registry interface
- [ ] Define composition rules
- [ ] Snapshot golden YAML examples (10–15 test cases)

### Phase 2: Path A (BAML-based, Low Effort)
- [ ] Write BAML function schema for pipeline generation
- [ ] Wire to `crux-agentic` planner module
- [ ] Implement `planner::generate(goal) -> String` (returns YAML)
- [ ] CLI: `crux-planner plan --goal "..." --output plan.crux`
- [ ] Integration tests with OPENAI_API_KEY

### Phase 3: Path B (Rust Crate, High Effort, Future)
- [ ] New crate `crux-planner` with AST + codegen
- [ ] Handler registry manifest (JSON or Rust derive)
- [ ] Composition rules engine (type checker + optimizer)
- [ ] `Planner::plan(&goal) -> Pipeline` (deterministic)

### Phase 4: Refinement
- [ ] Performance tuning (caching, parallelization)
- [ ] User feedback loops (plan explanation, debugging)
- [ ] Dynamic mode integration with `crux_script::execute`

---

## Example Use Cases

### 1. Data Processing Pipeline
**Goal:** "Read CSV, parse rows, extract email addresses, write to JSON"

Generated pipeline:
```yaml
steps:
  - step: read
    handler: fs::read
    path: input.csv
  - step: parse
    handler: json::parse
    input: "$step:read"
  - step: extract_emails
    handler: llm::extract
    function: ExtractEmails
    input: "$step:parse"
  - step: write
    handler: json::write
    content: "$step:extract_emails"
    path: emails.json
```

### 2. Code Review Assistant
**Goal:** "Review a git commit, summarize changes, flag issues, generate suggestions"

Generated pipeline:
```yaml
steps:
  - step: diff
    handler: git::diff
    base_ref: origin/main
  - step: analyze
    handler: llm::extract
    speculate:
      - name: issues
        function: FindCodeIssues
      - name: suggestions
        function: GenerateSuggestions
    pick_best_by: confidence
  - step: output
    handler: json::write
    content: "$step:analyze"
    path: review.json
```

### 3. Interactive Chat
**Goal:** "Have a conversation with a crux agent about this document"

Dynamic mode:
```rust
let intent = Intent::new("Chat: answer questions about document.txt");
let planner = Planner::new();
let pipeline = planner.plan(&intent).await?;
// Execute interactively, collect user input per step
crux_script::execute_interactive(&pipeline, user_input).await?
```

---

## Open Questions

1. **Handler Discovery**: Should planner read handler metadata from the registry at runtime, or
   from a static manifest?
2. **Ambiguity Resolution**: When NL goal maps to multiple handler sequences, how does planner
   choose? (Cost? Speed? Determinism?)
3. **User Feedback Loop**: Should planner explain its plan? ("I will read file → extract entities
   → write JSON")
4. **Versioning**: If handler signatures change, how do old pipelines behave?
5. **Composition Optimization**: Should planner suggest optimizations? ("You can combine steps 2
   and 3 into one speculate arm")

---

## References

- `crux-script`: YAML schema in `crates/crux-script/src/types.rs`
- `crux-agentic`: Handler registry in `crates/crux-agentic/src/lib.rs`
- BAML integration: `crates/crux-agentic/baml_src/`
- ArmDef and StepNode: `crates/crux-core/src/types/step.rs`
