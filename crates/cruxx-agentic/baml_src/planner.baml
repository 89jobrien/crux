// Pipeline generation from natural language goals.

class BudgetDef {
  tokens int?
  calls int?
  duration_ms int?
  cost_cents int?
}

class ArmDef {
  step string?
  handler string?
  args map<string, string>?
}

class StepNode {
  step string
  handler string?
  args map<string, string>?
}

class DelegateNode {
  delegate_handler string
  name string?
  budget BudgetDef?
}

class PipeNode {
  pipe string
  stages ArmDef[]
}

class JoinAllNode {
  join_all string
  arms ArmDef[]
}

class RouteBranch {
  range string
  label string
  handler string
  args map<string, string>?
}

class RouteNode {
  route_on_confidence string
  value string
  routes RouteBranch[]
}

class SpeculateNode {
  speculate string
  mode string?
  arms ArmDef[]
}

// Union via a wrapper — BAML uses a single class with optional fields for unions.
// Exactly one of the step-type fields will be set.
class StepDef {
  step StepNode?
  delegation DelegateNode?
  pipe PipeNode?
  join_all JoinAllNode?
  route RouteNode?
  speculate SpeculateNode?
}

class PipelineDef {
  pipeline string
  budget BudgetDef?
  steps StepDef[]
}

function GeneratePipeline(goal: string, available_handlers: string[], constraints: string?) -> PipelineDef {
  client OpenAIDefault
  prompt #"
    You are a pipeline generator for cruxx-script, a YAML-based pipeline DSL.

    Generate a valid cruxx-script pipeline from the given goal.

    ## cruxx-script pipeline grammar

    A pipeline has a top-level `pipeline` name and a list of `steps`.
    Optional top-level `budget` with keys: tokens, calls, duration_ms, cost_cents.

    Step types — set exactly one field per StepDef:

    1. step (simple handler call):
       step: { step: "read-file", handler: "fs::read", args: { path: "input.txt" } }

    2. pipe (sequential chain):
       pipe: { pipe: "read-and-summarize", stages: [
         { step: "read", handler: "fs::read", args: { path: "input.txt" } },
         { step: "summarize", handler: "llm::extract", args: { function: "Summarize" } }
       ]}

    3. join_all (parallel fan-out):
       join_all: { join_all: "parallel-work", arms: [
         { step: "a", handler: "fs::read" },
         { step: "b", handler: "shell::capture" }
       ]}

    4. delegation (sub-pipeline call):
       delegation: { delegate: "llm::invoke", budget: { tokens: 1000 } }

    5. speculate (pick best or first ok):
       speculate: { speculate: "try-approaches", mode: "pick_best", arms: [
         { step: "a", handler: "llm::extract" },
         { step: "b", handler: "llm::invoke" }
       ]}

    6. route (conditional routing on confidence):
       route: { route_on_confidence: "route-quality", value: "steps.classify.confidence",
         routes: [
           { range: "[0.0, 0.5)", label: "low", handler: "llm::invoke" },
           { range: "[0.5, 1.0]", label: "high", handler: "ctrl::noop" }
         ]}

    ArmDef: an object with step (label), optional handler, optional args.

    ## Available handlers

    Use ONLY these handlers:
    {% for h in available_handlers %}
    - {{ h }}
    {% endfor %}

    ## Rules

    - pipeline name should be descriptive kebab-case
    - Each step name must be unique within the pipeline
    - Use the simplest step type that fits (prefer step over pipe for single operations)
    - If the goal mentions reading a file, use fs::read
    - If the goal mentions running a command, use shell::capture or shell::exec
    - If the goal mentions summarization or extraction, use llm::extract
    - Keep pipelines concise — prefer fewer steps
    - Set exactly one step-type field per StepDef (step, pipe, join_all, delegation, speculate, route)

    {% if constraints %}
    ## Additional constraints
    {{ constraints }}
    {% endif %}

    ## Goal

    {{ goal }}

    {{ ctx.output_format }}
  "#
}

test TestGeneratePipelineFromMarkdownFile {
  functions [GeneratePipeline]
  args {
    goal #"
      Read a markdown file from disk, summarize its contents into bullet points,
      and classify the document into one of: design-doc, runbook, changelog, reference.
      The file path will be provided as input.
    "#
    available_handlers [
      #"fs::read"#,
      #"shell::capture"#,
      #"shell::exec"#,
      #"llm::extract"#,
      #"llm::invoke"#,
      #"ctrl::noop"#
    ]
    constraints #"
      - Must use fs::read to load the file before any LLM step
      - Classification confidence must route: below 0.6 → llm::invoke for a second pass, above → ctrl::noop
      - Keep the pipeline under 5 steps total
    "#
  }
}

test TestGeneratePipelineSimpleRead {
  functions [GeneratePipeline]
  args {
    goal #"
      Read a markdown file and extract all TODO items from it as a structured list.
    "#
    available_handlers [
      #"fs::read"#,
      #"llm::extract"#
    ]
    constraints null
  }
}
