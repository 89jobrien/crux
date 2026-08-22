# Cruxfiles

A pipeline is one execution flow. A **Cruxfile** is a set of named
targets with dependencies between them -- a make-style DAG that the
runner resolves topologically before executing anything.

Same YAML dialect, same handlers, same trace. The difference is the
top-level shape.

## Pipeline vs Cruxfile

The runner picks the parser by looking for a `targets:` key at the start
of a line. Everything else is parsed as a pipeline.

| | Pipeline | Cruxfile |
| --- | --- | --- |
| Top-level key | `pipeline:` | `targets:` |
| Shape | ordered `steps:` list | named targets + `depends:` |
| Typical name | `something.crux` | `Cruxfile` |
| Accepts JSON input | yes | no |
| Supports `--replay` | yes | no |

Both are validated by `crux check`, which reports which one it found:

```console
$ crux check Cruxfile
ok: Cruxfile (Cruxfile, 8 targets, default: ci)
```

## A minimal Cruxfile

```yaml
project: demo
default: all

targets:
  fmt:
    steps:
      - step: fmt_check
        handler: shell::capture
        args:
          cmd: "cargo fmt --all -- --check"

  lint:
    depends: [fmt]
    steps:
      - step: clippy
        handler: shell::capture
        args:
          cmd: "cargo clippy --all-targets -- -D warnings"

  all:
    depends: [fmt, lint]
```

Three keys carry the file:

- **`project:`** -- a name for the file, shown in the run summary
- **`default:`** -- the target used when none is named
- **`targets:`** -- a map of target name to definition

Each target takes `depends:` (a list of target names), `steps:` (exactly
the step list a pipeline uses), and an optional `budget:`.

## Running targets

Name a target directly. `crux <target>` is shorthand for
`crux run --target <target>`:

```bash
crux lint          # run `lint` and everything it depends on
crux run           # run the default target (`all`)
crux lint -v       # per-target timing and step counts
crux lint -n       # print the execution plan, run nothing
```

`-n` is the fastest way to see what a target actually pulls in:

```console
$ crux lint -n
Cruxfile: demo (target: lint)
Execution order: fmt -> lint

   1. fmt (1 steps: shell::capture)
   2. lint (1 steps: shell::capture)
```

Asking for `lint` ran `fmt` too. A target's dependencies are always part
of the plan -- there is no way to run a target in isolation short of
removing its `depends:`.

### Reserved names

`list`, `check`, `run`, `plan` and `help` are crux subcommands, and they
win over a target of the same name. A target called `check` is still
reachable, just not as `crux check`:

```bash
crux run --target check
```

The simpler fix is to not name a target after a subcommand. The crux
repo's own Cruxfile calls its `cargo check` target `typecheck` for
exactly this reason.

### Running a file instead

A first argument that names an existing file runs as a pipeline, so the
shorthand covers both:

```bash
crux examples/showcase.crux              # same as `crux run examples/...`
crux examples/showcase.crux input.json
```

## Execution order

`TargetResolver` walks `depends:` to collect every target reachable from
the requested one, then topologically sorts that subgraph. Independent
targets are ordered by name, so the plan is deterministic across runs.

Unknown dependencies and cycles are rejected when the file loads, before
any step executes. So is a `default:` that names a target that doesn't
exist.

A typo in the target name prints what is available:

```console
$ crux lnt
error: unknown target: lnt
available targets: fmt, lint, typecheck, build, test, deny, lint-crux, ci
```

## Aggregation targets

`steps:` is optional. A target with only `depends:` executes nothing of
its own and exists to name a group:

```yaml
  ci:
    depends: [fmt, lint, typecheck, build, test]
```

`crux ci` then runs the whole closure. Dry-run marks it for what it is:

```text
   8. ci (aggregation target)
```

## Fail-fast

The first target that fails stops the run. Every target after it is
reported as skipped, and the process exits 1:

```console
$ crux ci
  [ok] fmt (123ms)
  [ERR] lint (2ms)
[crux] target 'lint' failed: step 'shell::capture' failed: command exited 101:
[crux] skipped due to failure: typecheck, build, test, deny, lint-crux, ci
Cruxfile: crux [ci] 1/8 targets OK, 1 failed, 6 skipped (126ms)
```

Note that `shell::capture` fails the step on a non-zero exit, which is
what makes this work. `shell::exec` does not -- it captures output and
returns successfully whatever the exit code. A CI gate built on
`shell::exec` will pass no matter what the command does.

## Budgets

A Cruxfile can set a file-level `budget:`, and any target can override
it:

```yaml
budget: { calls: 20, duration_ms: 1800000 }

targets:
  test:
    budget: { calls: 5, duration_ms: 3600000 }
    steps:
      - step: nextest
        handler: shell::capture
        args:
          cmd: "cargo nextest run"
```

Budgets are **per target**, not per run. Each target executes in its own
`CruxCtx`, so `calls: 20` means twenty calls for that target, and the
count resets at the next one.

## What Cruxfiles don't do

Two flags on `crux run` apply to pipelines only, and are ignored without
error when the file is a Cruxfile:

- **JSON input.** Targets are always invoked with a null input. There is
  no `{{ input.* }}` to interpolate -- a target's steps get their values
  from `args:` and from each other.
- **`--replay`.** Replay matches steps against a previous trace, which is
  a single-pipeline notion. Use it on the pipeline files a target calls.

`--save-trace` does work, and writes one file per target, suffixed with
the target name:

```bash
crux ci --save-trace target/ci-trace
# -> target/ci-trace.fmt.json, target/ci-trace.lint.json, ...
```

`--strict` also works, and is worth knowing about: without it, a handler
that isn't registered gets a stub returning `{"_stub": ...}` at
confidence 0.5, plus a warning on stderr. That mid-range confidence is
deliberate -- it keeps a `route_on_confidence` keyed off a feature-gated
handler routing instead of failing. With `--strict`, an unregistered
handler is a hard error listing every name that failed to resolve.
Prefer `--strict` in CI.

## Discovery

With no path argument, `crux run` looks for a file named `Cruxfile` in
the current directory. If there isn't one, it says so rather than
guessing:

```console
$ crux run
error: no pipeline file specified and no Cruxfile found in cwd
```

`crux list` is unrelated -- it walks a directory tree for `.crux`
pipeline files, respecting `.gitignore`, and does not report Cruxfiles.

## Worked example: the crux repo's own CI gate

The `Cruxfile` at the repo root mirrors `just ci` as a fail-fast chain,
cheapest gate first:

```text
fmt -> lint -> typecheck -> build -> test -> deny -> lint-crux -> ci
```

Each target is one `shell::capture` step wrapping the command `just`
would run. Chaining them linearly rather than leaving them independent
is deliberate: it costs no parallelism the runner would have used --
targets execute one at a time in plan order, and so do the steps within
a target -- and it guarantees a formatting error surfaces in a second
rather than after a full test run.

Concurrency inside a Cruxfile comes from the combinators, not from the
target graph: a target whose steps include a `join_all:` fans out across
its arms exactly as it would in a standalone pipeline.

Try it without running anything:

```bash
crux run -n           # the whole chain
crux typecheck -n     # just up to the type check
```

## What next

- [Handlers and capabilities](../crux-capabilities.md) -- full handler
  reference
- [Real-world examples](06-real-world-examples.md) -- complete pipelines
- [Plugin system](../crux-plugins.md) -- adding handlers out of process
