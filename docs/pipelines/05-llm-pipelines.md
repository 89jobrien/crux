# LLM pipelines

## Raw completion with llm::invoke

Available without any feature flags. Calls an LLM and returns the
raw response.

```yaml
pipeline: ask
budget: { calls: 1, tokens: 2000 }

steps:
  - step: answer
    handler: llm::invoke
    args:
      prompt: "What is the capital of France?"
      provider: anthropic
      model: claude-sonnet-4-6
```

Set your API key:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
# or
export OPENAI_API_KEY=sk-...
```

Supported providers: `anthropic`, `openai`, `ollama`.

## Structured extraction with BAML

Build `crux run` with the `baml` feature to unlock structured
extraction handlers:

```bash
cargo build -p crux-agentic --features baml --bin crux run --release
```

### llm::extract

Calls a BAML function and returns structured JSON.

```yaml
pipeline: extract_summary
budget: { calls: 2 }

steps:
  - step: summarize
    handler: llm::extract
    # Input: { "function": "Summarize", "input": { "text": "...", "max_sentences": 3 } }

  - step: log_output
    handler: ctrl::log
```

Run with input:

```bash
crux run examples/extract_summary.crux examples/input_summary.json
```

Output:

```json
{
  "summary": "Crux is an agentic DSL for Rust...",
  "key_points": ["Every execution unit is a Crux<T> value", "..."],
  "word_count": 89
}
```

Three BAML functions are wired: `ExtractEntities`, `Summarize`,
`Classify`.

### llm::decompose

Break a spec into a task list:

```yaml
- step: decompose
  handler: llm::decompose
  args:
    spec: "Build a REST API with auth and rate limiting"
```

### llm::plan

Generate a pipeline from a natural-language goal:

```yaml
- step: plan
  handler: llm::plan
  args:
    goal: "Review this PR for security issues"
```

## Combining LLM steps with other handlers

A typical pattern: gather context with shell/git handlers, then
pass it to an LLM step.

```yaml
pipeline: review_with_context
budget: { calls: 4, tokens: 4000 }

steps:
  - join_all: context
    arms:
      - step: diff
        handler: git::diff
        args:
          revision: "HEAD~1"
      - step: files
        handler: git::staged_files

  - step: analyze
    handler: llm::invoke
    args:
      prompt: "Review this diff for issues: {{input}}"
      provider: anthropic
      model: claude-sonnet-4-6

  - step: log_review
    handler: ctrl::log
    args:
      pretty: true
```

Next: [Real-world examples](./06-real-world-examples.md).
