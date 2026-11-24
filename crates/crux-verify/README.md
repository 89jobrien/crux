# crux-verify

Formal proofs for Crux trace invariants, written for the [hooo](https://crates.io/crates/hooo)
proof checker. The crate has no runtime code and is not published (`publish = false`). Its only
job is to hold `.hooo` proof files and a test that checks them.

## Proofs

| File                 | Property                                                                             |
| -------------------- | ------------------------------------------------------------------------------------ |
| `proofs/budget.hooo` | `BudgetTracker::remaining()` is non-increasing across `consume()` and `begin_step()` |

`budget.hooo` takes two single-step monotonicity facts as trusted axioms, `consume_mono` and
`step_mono`. Both follow from the Rust implementation: `consume` saturating-adds to the used
counter, and `begin_step` increments the steps counter, so `remaining = limit.saturating_sub(used)`
can only decrease. The file then proves `two_consume_mono` from transitivity (`pord_trans`). The
same argument extends to any finite sequence of operations.

`proofs/Hooo.config` lists the functions the checker should expose from that file.

## Running the proofs

The proof test is gated behind a feature so that ordinary `cargo test` does not compile the hooo
dependency:

```console
cargo nextest run -p crux-verify --features verify
```

`tests/verify_budget.rs` runs two checks: `budget.hooo` must type-check, and the proofs directory
must contain at least one `.hooo` file.

## Adding a proof

1. Add a `.hooo` file under `proofs/`. Model the Rust type as abstract `sym` declarations, and
   state each axiom next to the implementation fact that justifies it.
2. List the new functions in `proofs/Hooo.config`.
3. Add a check in `tests/verify_budget.rs`, or a new test file, that loads the file with
   `check_file`.
4. Update the table above.
