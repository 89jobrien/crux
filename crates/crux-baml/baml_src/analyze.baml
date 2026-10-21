// Analysis and confidence-scoring functions backing the `llm::analyze` and
// `llm::confidence` pipeline handlers.
//
// Both functions bind to the `Local` client (Ollama first, then hosted
// providers) so a local model is enough to run credential-free.

enum Severity {
  BLOCKING
  WARNING
  INFO
}

enum ConfidenceLevel {
  HIGH
  MEDIUM
  LOW
}

class Finding {
  title string
  detail string
  severity Severity
  evidence string?
}

class AnalyzeOutput {
  summary string
  findings Finding[]
  recommendation string
  confidence float
}

function Analyze(subject: string, evidence: string, focus: string?) -> AnalyzeOutput {
  client Local
  prompt #"
    You are a senior engineer reviewing the evidence below. Report only what the
    evidence supports — do not speculate about code, tests, or state you cannot see.

    Subject under review: {{ subject }}
    {% if focus %}
    Focus area: {{ focus }}
    {% endif %}

    Evidence:
    {{ evidence }}

    Produce:
    - summary: 1-3 sentences stating what the evidence shows
    - findings: zero or more discrete observations, most important first
      - title: short label, max 60 chars
      - detail: what was observed and why it matters
      - severity: BLOCKING (must be fixed), WARNING (should be addressed), or INFO (worth knowing)
      - evidence: the exact excerpt from the evidence that supports this finding (optional)
    - recommendation: the single most useful next action, or an empty string when
      there is nothing to do
    - confidence: 0.0 to 1.0 — how well the evidence supports the summary and findings.
      Use a low score when the evidence is truncated, ambiguous, or incomplete.

    {{ ctx.output_format }}
  "#
}

class ConfidenceOutput {
  score float
  level ConfidenceLevel
  reasoning string
  factors string[]
}

function ScoreConfidence(claim: string, evidence: string, criteria: string[]) -> ConfidenceOutput {
  client Local
  prompt #"
    You are scoring how strongly the evidence supports the claim. Be calibrated and
    skeptical: a claim backed only by thin or indirect evidence should score low.

    Claim: {{ claim }}

    Evidence:
    {{ evidence }}

    Score the claim against each criterion, then weigh them together.
    {% for c in criteria %}
    - {{ c }}
    {% endfor %}
    {% if criteria | length == 0 %}
    Use the default criteria: directness of support, completeness of the evidence,
    internal consistency, and absence of contradicting information.
    {% endif %}

    Produce:
    - score: 0.0 to 1.0 overall support for the claim
    - level: HIGH (>= 0.8), MEDIUM (>= 0.5), or LOW (< 0.5)
    - reasoning: 1-3 sentences justifying the score
    - factors: the individual signals that moved the score up or down

    {{ ctx.output_format }}
  "#
}

test TestAnalyzeCIOutput {
  functions [Analyze]
  args {
    subject "cargo deny advisories report"
    evidence #"
      warning[RUSTSEC-2024-0001]: vulnerability in transitive dependency `openssl 0.9.1`
        severity: high
        fix: upgrade `openssl` to 0.9.8
      warning[RUSTSEC-2024-0042]: unmaintained crate `paste 0.1.0` in dev-dependencies
    "#
    focus "security advisories"
  }
}

test TestScoreConfidenceSupportedClaim {
  functions [ScoreConfidence]
  args {
    claim "The working tree has no unstaged changes."
    evidence "git status --porcelain produced no output."
    criteria [
      #"Is the evidence direct and unambiguous?"#,
      #"Is the evidence complete?"#,
      #"Does anything contradict the claim?"#
    ]
  }
}

test TestScoreConfidenceUnsupportedClaim {
  functions [ScoreConfidence]
  args {
    claim "All tests pass on the current branch."
    evidence "Only `cargo build` was run. The test suite was never invoked."
    criteria [
      #"Is the evidence direct and unambiguous?"#,
      #"Is the evidence complete?"#,
      #"Does anything contradict the claim?"#
    ]
  }
}

// -- Generic completion -------------------------------------------------------
//
// Backs `llm::invoke` / `llm::stream` / `llm::invoke_with_fallback`.
//
// `@@dynamic` lets a caller inject its own fields at runtime through a BAML
// TypeBuilder, so one function covers both free-text completion (no injected
// fields) and schema-validated structured output.
//
// `confidence` is declared here rather than left to the caller so that *every*
// completion carries one, which is what makes any LLM step routable by
// `route_on_confidence`. The handler adjusts this self-report by how many
// attempts BAML needed to reach a parseable answer — see `complete.rs`.

class Completion {
  content string

  // The model's own assessment of how well the answer is supported by the
  // prompt. Saturates at 1.0; the handler applies a retry penalty on top.
  confidence float

  @@dynamic
}

function Complete(prompt: string, system: string?, max_tokens: int?) -> Completion {
  client Local
  prompt #"
    {{ ctx.output_format }}

    Set `confidence` to how well this answer is actually supported: high when the
    prompt contained everything needed, low when you had to guess or when the
    request was ambiguous. Do not default it to 1.0.

    {% if system %}
    {{ _.role("system") }}
    {{ system }}
    {% endif %}
    {{ _.role("user") }}
    {{ prompt }}
  "#
}

test TestCompleteFreeText {
  functions [Complete]
  args {
    prompt "What is the capital of France? Answer in one word."
  }
}
