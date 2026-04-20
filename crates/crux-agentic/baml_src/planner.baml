// Pipeline generation from natural language goals.

class PipelineYamlOutput {
  pipeline_name string
  yaml string
}

function GeneratePipeline(goal: string, available_handlers: string[], constraints: string?) -> PipelineYamlOutput {
  client Auto
  prompt #"
    You are a pipeline generator for crux-script, a YAML-based pipeline DSL.

    Generate a valid crux-script pipeline YAML from the given goal.

    ## crux-script YAML grammar

    A pipeline has a top-level `pipeline` name and a list of `steps`.
    Optional top-level `budget` with keys: tokens, calls, duration_ms, cost_cents.

    Step types:

    1. step (simple handler call):
       - step: read-file
         handler: fs::read
         args:
           path: input.txt

    2. pipe (sequential chain):
       - pipe: read-and-summarize
         stages:
           - step: read
             handler: fs::read
             args:
               path: input.txt
           - step: summarize
             handler: llm::extract
             args:
               function: Summarize

    3. join_all (parallel fan-out):
       - join_all: parallel-work
         arms:
           - fs::read
           - shell::capture

    4. delegate (sub-pipeline call):
       - delegate: llm::invoke
         budget:
           tokens: 1000

    5. speculate (pick best or first ok):
       - speculate: try-approaches
         mode: pick_best
         arms:
           - llm::extract
           - llm::invoke

    6. route_on_confidence (conditional routing):
       - route_on_confidence: route-quality
         value: "steps.classify.confidence"
         routes:
           - range: "[0.0, 0.5)"
             handler: llm::invoke
           - range: "[0.5, 1.0]"
             handler: ctrl::noop

    ArmDef: either a bare handler name string, or an object with step/handler/args.

    ## Available handlers

    Use ONLY these handlers:
    {% for h in available_handlers %}
    - {{ h }}
    {% endfor %}

    ## Rules

    - Output raw YAML only, no markdown fences
    - Pipeline name should be descriptive kebab-case
    - Each step name must be unique within the pipeline
    - Use the simplest step type that fits (prefer step over pipe for single operations)
    - If the goal mentions reading a file, use fs::read
    - If the goal mentions running a command, use shell::capture or shell::exec
    - If the goal mentions summarization or extraction, use llm::extract
    - Keep pipelines concise — prefer fewer steps

    {% if constraints %}
    ## Additional constraints
    {{ constraints }}
    {% endif %}

    ## Goal

    {{ goal }}

    {{ ctx.output_format }}
  "#
}
