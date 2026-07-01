---
crate: crux-types
total: 4
by_status:
  open: 4
  done: 0
items:
  - id: 75
    status: open
    priority: medium
    area: architecture
    title: "Schema/runtime split"
    location: "src/lib.rs::module:7"
  - id: 76
    status: open
    priority: medium
    area: streaming
    title: "Streaming step subscriptions"
    location: "src/step.rs::module:4"
  - id: 79
    status: open
    priority: low
    area: observability
    title: "Cited findings on failures"
    location: "src/step.rs::module:8"
  - id: 81
    status: open
    priority: medium
    area: types
    title: "Step output type safety"
    location: "src/step.rs::module:11"
---

# TODO: crux-types

- [ ] **#75** Schema/runtime split — push all combinators into a
  `crux-schema` crate (no tokio, no LLM deps) so external consumers
  can use crux traces without pulling the full runtime
- [ ] **#76** Streaming step subscriptions — formalize
  `events: Vec<Value>` as a proper subscription model
- [ ] **#79** Cited findings on failures — add a `cited_reason` field
  with source references to step failures
- [ ] **#81** Step output type safety — step outputs are all `Value`
  today; explore typed outputs
