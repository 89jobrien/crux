# crux-typesafe

TypeSafe System One judgment handlers for `crux-script`. Provides `judge::score`, which rates some
state against an ordered rubric and reports a **calibrated** confidence that `route_on_confidence`
can route on.

## Why: calibration, not reachability

YAML pipelines could route on confidence before this crate existed. `crux-baml` already declares
`ConfidenceCapability::Always` on five handlers — `llm::analyze`, `llm::confidence`, `llm::invoke`,
`llm::invoke_with_fallback`, and `llm::stream` — so `judge::score` declaring it is table stakes
rather than the contribution.

What the existing confidence sources cannot offer is a number that means one specific thing.
`crux-baml` documents its own self-report as *"only loosely calibrated — a confident-sounding answer
is not necessarily a correct one"*, and the `crux-stdlib` handler families (`ctrl::*`, `shell::*`,
`fs::*`, `git::*`, `text::*`, `json::*`) declare no confidence capability at all. Absence is not an
error at read time either: `HandlerOutput::confidence_or_default` reports a neutral `0.5`, so a step
that measured nothing routes as though it were half-sure.

A TypeSafe `Score` answer carries an ordered rubric of concrete situations (`criteria`), a
probability-weighted position along it (`score`), and the distribution over levels (`probabilities`).
That position is normalized onto `0..=1` and discounted by retries:

```text
normalized = score / (levels - 1)          # puts an N-level rubric on a 0..=1 axis
confidence = normalized * 0.8 ^ (attempts - 1)
```

Dividing by the top level number is what makes a 3-level and a 4-level rubric comparable, so a
threshold means the same thing however many levels a pipeline author wrote. The retry term is
deliberately identical to `crux-baml`'s, so scores discount the same way in both crates.

TypeSafe's own `confidence` field measures how *concentrated* a distribution is, not how correct the
answer is. It is published as `distribution_confidence` in the payload and never routed on.

## Usage

`route_on_confidence` requires a `routes:` list, and every entry needs a `range:`, a `label:`, and a
`handler:`. Register the handler, then route on the calibrated value:

```yaml
pipeline: triage
steps:
  - step: classify
    handler: judge::score
    args:
      state: "The export button crashes the settings page in Safari."
      instructions: How severe is the reported issue?
      criteria:
        - Cosmetic; no impact
        - Degraded; workaround exists
        - Blocking; no workaround
  - route_on_confidence: act
    value: "{{ steps.classify.confidence }}"
    routes:
      - range: "[0.0, 0.34)"
        label: low
        handler: low
      - range: "[0.34, 0.67)"
        label: mid
        handler: mid
      - range: "[0.67, 1.0]"
        label: high
        handler: high
```

A score of `2.0` on that rubric normalizes to `1.0` and lands in the top band. The raw `2.0` would
not, which is what makes the normalization load-bearing rather than cosmetic.

## The `JudgmentClient` seam

`register` takes an `Arc<dyn JudgmentClient>`, which is the whole injection point:

```rust
use std::sync::Arc;

use crux_script::HandlerRegistry;

let mut registry = HandlerRegistry::new();
crux_typesafe::register(&mut registry, Arc::new(my_client));
```

Production wires `HttpJudgmentClient` (default `http` feature); tests wire `CannedJudgmentClient`.
The trait is dyn-compatible so one registered handler serves either backend.

## Testing without credentials

Nothing in the handler or calibration path needs an API key or network access, so routing is asserted
in CI against a canned response:

```console
cargo nextest run -p crux-typesafe
cargo test -p crux-typesafe --doc
cargo clippy -p crux-typesafe --all-targets --all-features -- -D warnings
```

`tests/routing.rs` runs the YAML pipeline above at each confidence band, and the crate-level doctest
in `src/lib.rs` executes the same schema end to end — so a change to the documented YAML that breaks
the schema fails the test suite rather than silently misleading a reader.