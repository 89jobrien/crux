// Structured extraction functions for crux-agentic llm::extract handler.

// ============================================================================
// Entity Extraction
// ============================================================================

class Entity {
  name string
  entity_type string
  description string?
}

class ExtractEntitiesOutput {
  entities Entity[]
}

function ExtractEntities(text: string) -> ExtractEntitiesOutput {
  client Auto
  prompt #"
    Extract all named entities from the following text.

    Text: {{ text }}

    For each entity, provide:
    - name: the entity as it appears in the text
    - entity_type: one of PERSON, ORGANIZATION, LOCATION, DATE, CONCEPT, OTHER
    - description: brief description if context is available (optional)

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Summarization
// ============================================================================

class SummarizeOutput {
  summary string
  key_points string[]
  word_count int
}

function Summarize(text: string, max_sentences: int) -> SummarizeOutput {
  client Auto
  prompt #"
    Summarize the following text concisely.

    Text: {{ text }}
    Target length: {{ max_sentences }} sentences or fewer.

    Provide:
    - summary: the condensed version
    - key_points: 2-5 most important points as bullet items
    - word_count: approximate word count of the original text

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Project Enrichment
// ============================================================================

class DescribeProjectOutput {
  description string
}

function DescribeProject(name: string, language: string?, readme: string?, commits: string[]) -> DescribeProjectOutput {
  client Auto
  prompt #"
    Write a single concise sentence (max 12 words) describing this software project.
    Be specific — mention what it does, not just what category it is.
    Do not start with "A" or "An". Do not use the project name.

    Project: {{ name }}
    Language: {{ language }}
    README excerpt: {{ readme }}
    Recent commits: {{ commits }}

    {{ ctx.output_format }}
  "#
}

class GenerateChangelogOutput {
  summary string
  highlights string[]
}

function GenerateChangelog(name: string, commits: string[]) -> GenerateChangelogOutput {
  client Auto
  prompt #"
    Given these recent commit messages for {{ name }}, write a brief human-readable
    changelog summary (1-2 sentences) and list 2-4 highlights.
    Focus on what changed for users, not implementation details.
    Skip chore/docs/ci commits in highlights.

    Commits:
    {% for c in commits %}
    - {{ c }}
    {% endfor %}

    {{ ctx.output_format }}
  "#
}

class ClassifyProjectOutput {
  category string
  confidence float
}

function ClassifyProject(name: string, description: string?, language: string?, topics: string[], commits: string[]) -> ClassifyProjectOutput {
  client Auto
  prompt #"
    Classify this software project into exactly one category.
    Categories: cli, plugin, infra, library, agent, tool, prototype

    Project: {{ name }}
    Description: {{ description }}
    Language: {{ language }}
    Topics: {{ topics }}
    Recent commits: {{ commits }}

    - category: the single best-fit category
    - confidence: 0.0 to 1.0

    {{ ctx.output_format }}
  "#
}

class AssessHealthOutput {
  status string
  confidence float
  reason string
}

function AssessHealth(name: string, pushed_at: string, commit_dates: string[], open_issues: int?) -> AssessHealthOutput {
  client Auto
  prompt #"
    Assess the health/activity status of this software project.
    Status options: active, stale, dormant, experimental

    Definitions:
    - active: regular commits in the last 30 days, clear direction
    - stale: no commits in 30-90 days, or sporadic activity
    - dormant: no activity in 90+ days
    - experimental: early-stage, irregular commits, prototype-like

    Project: {{ name }}
    Last pushed: {{ pushed_at }}
    Recent commit dates: {{ commit_dates }}
    Open issues: {{ open_issues }}

    {{ ctx.output_format }}
  "#
}

class SuggestRelatedOutput {
  related string[]
}

function SuggestRelated(name: string, description: string?, category: string?, all_projects: string[]) -> SuggestRelatedOutput {
  client Auto
  prompt #"
    Given this project and the full list of projects, suggest the 3 most related ones.
    Consider shared purpose, language, and ecosystem.
    Return project names exactly as they appear in the list.

    Project: {{ name }}
    Description: {{ description }}
    Category: {{ category }}

    All projects:
    {% for p in all_projects %}
    - {{ p }}
    {% endfor %}

    Return exactly 3 names in the `related` array.

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// CI / Hook Failure Triage
// ============================================================================

class ClassifyCIFailureOutput {
  kind string
  fix_type string
  suggested_fix string
  confidence float
  new_pattern string?
}

function ClassifyCIFailure(failure_output: string, known_patterns: string[]) -> ClassifyCIFailureOutput {
  client Auto
  prompt #"
    You are a CI and pre-commit hook repair agent.
    Classify this failure and suggest the minimal fix.

    Rules:
    - NEVER suggest modifying test expectations to make tests pass
    - Only fix implementation, hook config, or exclusion patterns
    - If the failure matches a known false-positive pattern, say so

    Failure output:
    {{ failure_output }}

    Known false-positive patterns:
    {% for p in known_patterns %}
    - {{ p }}
    {% endfor %}

    Return:
    - kind: "false-positive" or "real-failure"
    - fix_type: one of "hook-config", "obfsck-ignore", "clippy-allow", "implementation", "unknown"
    - suggested_fix: the exact change to make (file path + content or command to run)
    - confidence: 0.0 to 1.0 (how certain you are this fix is correct and safe)
    - new_pattern: if this is a false-positive, the pattern string to add to known patterns (optional)

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Spec / Plan Decomposition
// ============================================================================

class Task {
  id string
  name string
  title string
  description string
  priority string
  status string
  files string[]
}

class DecomposeSpecOutput {
  tasks Task[]
}

function DecomposeSpec(text: string) -> DecomposeSpecOutput {
  client Auto
  prompt #"
    You are a technical project manager. Read the following spec or implementation plan and
    decompose it into a structured list of discrete tasks.

    Rules:
    - Each task must be actionable and independently executable
    - id: short kebab-case identifier, e.g. "add-llm-handler"
    - name: snake_case version of id, e.g. "add_llm_handler"
    - title: human-readable title (max 60 chars)
    - description: 1-3 sentences describing exactly what to implement
    - priority: one of P0, P1, P2 (P0 = blocking/critical, P1 = core work, P2 = nice-to-have)
    - status: always "open"
    - files: list of file paths that will likely need to be created or modified

    Spec / Plan:
    {{ text }}

    {{ ctx.output_format }}
  "#
}

// ============================================================================
// Classification
// ============================================================================

class ClassifyOutput {
  label string
  confidence float
  reasoning string?
}

function Classify(text: string, labels: string[]) -> ClassifyOutput {
  client Auto
  prompt #"
    Classify the following text into exactly one of the provided labels.

    Text: {{ text }}
    Labels: {{ labels }}

    Provide:
    - label: the chosen label (must be one of the provided labels)
    - confidence: confidence score from 0.0 to 1.0
    - reasoning: brief explanation of why this label was chosen (optional)

    {{ ctx.output_format }}
  "#
}
