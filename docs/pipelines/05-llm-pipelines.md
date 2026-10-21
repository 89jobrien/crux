# LLM pipelines

Every handler that makes a model call is routed through BAML. There is no
feature flag and no separate build: `crux-baml` is a required dependency, so
provider selection, retries, and output parsing all live in one place.

## Completion with llm::invoke

```yaml
pipeline: ask
budget: { calls: 1, tokens: 2000 }

steps:
  - step: answer
    handler: llm::invoke
    args:
      prompt: "What is the capital of France?"
```

## Structured output with a caller-supplied schema

`llm::invoke` is free text by default. Pass a `schema` and BAML injects those
fields into the completion's output type, so the model is constrained to that
exact shape and the result is validated rather than prose you have to parse
yourself:

```yaml
- step: review
  handler: llm::invoke
  args:
    prompt: "Review this diff for correctness and security problems.\n\n{{ steps.diff.output.stdout }}"
    system: "You are a senior engineer reviewing a diff."
    schema:
      type: object
      properties:
        verdict:
          type: string
          enum: [approve, comment, block]
          description: "Overall call on whether the diff is safe to merge."
        blockers:
          type: array
          items: { type: string }
          description: "Problems that must be fixed before merge. Empty if none."
        score:
          type: number
          description: "0.0 (unsafe) to 1.0 (safe)."
```

Injected fields are merged alongside `content`, so templates read
`{{ steps.review.output.verdict }}`.

A bare `{field: {schema}}` map is accepted as shorthand for a full JSON Schema.
Values must be schemas, not samples — a bare `"high"` is ambiguous between a
type and an enum, and guessing wrong silently mis-shapes the output.

| JSON Schema       | BAML                     |
| ----------------- | ------------------------ |
| `string`          | `string`                 |
| `number`          | `float`                  |
| `integer`         | `int`                    |
| `boolean`         | `bool`                   |
| `array`           | `T[]`                    |
| `enum` of strings | union of literal strings |
| `anyOf` / `oneOf` | union                    |
| `$ref`            | local `$defs` only       |

Anything outside that subset is rejected with a message naming the offending
field, rather than sending a mis-shaped schema to the model.

## Confidence: every LLM call scores itself

All LLM handlers emit a step confidence, so any of them can feed
`route_on_confidence` without a separate scoring step:

```yaml
- step: assess
  handler: llm::invoke
  args:
    prompt: "In one sentence, what does this workspace do?"

- route_on_confidence: report
  value: "{{ steps.assess.confidence }}"
  routes:
    - range: "[0.0, 0.5)"
      label: unsure
      handler: ctrl::log
    - range: "[0.5, 1.0]"
      label: confident
      handler: ctrl::log
```

The score combines two inputs:

| Input | Source |
| --- | --- |
| Self-report | Every BAML completion type declares `confidence: float`; the prompt asks the model how well its answer is supported |
| Retry penalty | BAML's `FunctionLog::calls()` returns "all calls made (including retries)" |

```text
confidence = self_report * 0.8 ^ (attempts - 1)
```

A first-try success passes the self-report through untouched; needing a second
attempt to get parseable output discounts it. `confidence` and `attempts` both
appear in the payload, so a trace shows *why* a step scored what it did.

The self-report half is only loosely calibrated — a confident-sounding answer is
not necessarily a correct one. The retry term is the objective half. For
decisions needing real calibration, use `llm::confidence`, which scores support
against explicit evidence and criteria rather than asking the model to grade
itself. See `examples/invoke_confidence.crux`.

## Backends

The default BAML client tries a local Ollama first, so nothing below needs a key
when `ollama serve` is reachable:

```bash
ollama serve   # then nothing else
```

It falls back to `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` when no local model is
available:

```bash
export ANTHROPIC_API_KEY=sk-ant-...
# or
export OPENAI_API_KEY=sk-...
```

Pin a specific BAML client with the `client` arg. `ollama` is also usable as a
hosted-style provider for the `LlmProvider` port, which stays public for calling
a model directly from Rust.

## Structured extraction with BAML

Already registered — no feature flag or special build.

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

## Analysis with llm::analyze and llm::confidence

`llm::analyze` turns raw evidence into a summary plus severity-tagged findings,
and reports a confidence score. `llm::confidence` scores how strongly evidence
supports a specific claim, and reports that score as the step confidence — which
makes it the natural way to feed `route_on_confidence` a calibrated number
instead of an ad-hoc `score` field.

```yaml
- step: inspect
  handler: llm::analyze
  args:
    subject: "cargo deny advisories report"
    evidence: "{{ steps.gather.output.stdout }}"
    focus: security advisories

- step: score
  handler: llm::confidence
  args:
    claim: "The staged diff is safe to commit as-is."
    evidence: "{{ steps.inspect.output.summary }}"

- route_on_confidence: verdict
  value: "{{ steps.score.confidence }}"
  routes:
    - range: "[0.0, 0.4)"
      label: blockers
      handler: ctrl::log
    - range: "[0.4, 1.0]"
      label: ok
      handler: ctrl::log
```

Both bind to a local-first BAML client: they try a local Ollama
(`http://localhost:11434/v1`, model `llama3.2`) and need no API key when
`ollama serve` is reachable, falling back to hosted providers otherwise. Pin a
specific client with the `client` arg. Full runnable pipeline:
`examples/analyze_review.crux`.

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
