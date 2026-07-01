---
crate: crux-runtime
total: 3
by_status:
  open: 3
  done: 0
items:
  - id: 78
    status: open
    priority: medium
    area: events
    title: "EDDOS-style event aggregation"
    location: "src/lib.rs::module:5"
  - id: 80
    status: open
    priority: low
    area: scheduling
    title: "Token-shape step priority"
    location: "src/agent.rs::module:6"
  - id: 72
    status: open
    priority: medium
    area: planner
    title: "Planner-based action dispatch"
    location: "src/ctx.rs::module:5"
---

# TODO: crux-runtime

- [ ] **#78** EDDOS-style event aggregation — unify heterogeneous step
  types into a typed event stream (MPSC -> enrichment -> batching ->
  broadcast) for analytics, replay filtering, and multi-agent
  coordination
- [ ] **#80** Token-shape step priority — infer priority from naming
  convention
- [ ] **#72** Planner-based action dispatch — refactor
  step/delegate/speculate to return actions through the planner gate
