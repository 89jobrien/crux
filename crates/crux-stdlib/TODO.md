---
crate: crux-stdlib
total: 1
by_status:
  open: 0
  done: 1
items:
  - id: 70
    status: done
    priority: medium
    area: json
    title: "Extend json::jq beyond dot-path"
    location: "src/json.rs::eval_jq"
---

# TODO: crux-stdlib

- [x] **#70** Extend `json::jq` beyond dot-path — added `[idx]` indexing,
  `|` pipes, `select(cond)`, `map(expr)`
