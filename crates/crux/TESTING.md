---
crate: crux
test_strategy: integration
inline_test_modules: 1
dedicated_test_files: 20
test_files:
  - path: tests/agent_macro.rs
    coverage: "agent macro expansion, Agent trait impl"
  - path: tests/combinators.rs
    coverage: "pipe(), join_all(), route_on_confidence()"
  - path: tests/delegation.rs
    coverage: "DelegationBuilder, budget scoping, child contexts"
  - path: tests/speculation.rs
    coverage: "SpeculationBuilder, pick_best_by, first_ok"
  - path: tests/task_registry.rs
    coverage: "Submit, get, update, checkpoint, CAS"
commands:
  default: "cargo nextest run -p crux"
  single: "cargo nextest run -p crux -- agent_macro"
  redb: "cargo nextest run -p crux --features redb"
---

# Testing: crux

## Integration Tests

All tests are in `tests/` (no inline unit tests beyond a facade smoke
test).

## Running

```bash
cargo nextest run -p crux
cargo nextest run -p crux -- agent_macro    # single test file
cargo nextest run -p crux --features redb   # redb backend tests
```
