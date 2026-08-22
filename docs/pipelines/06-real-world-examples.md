# Real-world examples

Two complete pipelines that ship with crux. Both run against fixture
files -- no API keys or repo state needed.

## Code review gate

`examples/showcase_review.crux` -- reads a synthetic diff and findings
file, classifies severity, computes a score, and emits a verdict.

```bash
crux run examples/showcase_review.crux
```

What it demonstrates:

- **`join_all`** to fetch diff stats and findings in parallel
- **`pipe`** to classify findings by severity and log counts
- **`ctrl::log`** with `field`, `compact`, and `pretty` options
- A verdict step that computes a score and returns `APPROVE` or
  `REQUEST_CHANGES`

Key structure:

```yaml
# 1. Parallel gather
- join_all: gather
  arms:
    - step: diff_stats
      handler: shell::capture
      args:
        cmd: "wc -l < examples/fixtures/review_diff.patch"
    - step: findings
      handler: shell::capture
      args:
        cmd: "cat examples/fixtures/review_findings.json"

# 2. Pipe: classify and count
- pipe: evaluate
  stages:
    - step: count_by_severity
      handler: shell::capture
      args:
        cmd: "cat findings.json | jq '{blocking: ..., total: ...}'"
    - step: log_counts
      handler: ctrl::log

# 3. Verdict
- step: verdict
  handler: shell::capture
  args:
    cmd: "... | jq '{score: ..., verdict: ...}'"
```

## Secret scan triage

`examples/showcase_triage.crux` -- reads synthetic obfsck JSONL
findings, classifies true vs false positives, and suggests allowlist
entries.

```bash
crux run examples/showcase_triage.crux
```

What it demonstrates:

- **`pipe`** to classify findings by confidence threshold
- Splitting results into true positives, false positives, and
  uncertain categories
- Generating actionable allowlist suggestions
- A final verdict that decides whether to block the commit

Key structure:

```yaml
# 1. Read scan output
- step: read_scan
  handler: shell::capture
  args:
    cmd: "cat examples/fixtures/obfsck_findings.jsonl"

# 2. Classify by confidence
- pipe: classify
  stages:
    - step: split_by_confidence
      handler: shell::capture
      args:
        cmd: "... | jq -s '{true_positives: ..., false_positives: ...}'"

# 3. Generate allowlist
- step: allowlist
  handler: shell::capture
  args:
    cmd: "... | jq -s '[... | {file, pattern, reason}]'"

# 4. Verdict: block or pass
- step: verdict
  handler: shell::capture
```

## CI gate

`examples/showcase_ci.crux` -- parses cargo-deny output, counts
violation types in parallel, and scores fixability.

```bash
crux run examples/showcase_ci.crux
```

Demonstrates `join_all` for parallel parsing, `ctrl::assert` for
blocking conditions, and `pipe` for report generation.

## Running all showcases

```bash
crux run examples/showcase.crux examples/input_showcase.json
crux run examples/showcase_review.crux
crux run examples/showcase_ci.crux
crux run examples/showcase_triage.crux
```

Each runs in under a second with no external dependencies.

## What next

- [Cruxfiles](07-cruxfiles.md) -- multi-target build files and the
  `crux <target>` shorthand
- [Handlers and capabilities](../crux-capabilities.md) -- full
  handler reference
- [Syntax reference](../crux-syntax-reference.md) -- complete YAML
  and Rust syntax
- [Rust walkthrough](../walkthrough/README.md) -- typed agents,
  delegation, replay
