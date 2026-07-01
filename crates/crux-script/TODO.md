---
crate: crux-script
total: 3
by_status:
  open: 3
  done: 0
items:
  - id: 82
    status: open
    priority: high
    area: validation
    title: "Pipeline validation pass"
    location: "src/lib.rs::module:5"
  - id: 63
    status: open
    priority: high
    area: execution
    title: "Implement real step runners"
    location: "src/step_runner.rs::default:89"
  - id: null
    status: open
    priority: low
    area: expressions
    title: "Review trim_start_matches behavior in expr"
    location: "src/expr.rs::resolve_path:124"
---

# TODO: crux-script

- [ ] **#82** Pipeline validation pass — catch bad refs, missing
  handlers, type mismatches, and unreachable steps before execution
  starts (static analysis)
- [ ] **#63** Implement real step runners — all return `Value::Null`
  today
- [ ] Review `trim_start_matches` in `expr.rs` — strips repeated
  `"output."` prefixes; verify this is correct behavior
