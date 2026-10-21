# Crux Hooklings Preflight Configuration

Automated pre-commit validation for the Crux project using hooklings.

## Overview

The `hooklings.crux` file at the repository root defines a complete preflight pipeline that runs before commits. It validates:

1. **Environment** — Shell detection, required tools, working directory
2. **Git State** — Uncommitted changes, recent commits
3. **Build Gates** — Rust formatting, clippy lints, nextest suite
4. **Documentation** — Doc tests and examples

## Usage

### Run Full Preflight Before Committing

```bash
cd /Users/joe/dev/crux
hooklings preflight
```

This runs all checks in sequence and reports pass/fail status.

### Run Specific Checks

```bash
# Run a single check
hooklings check detect_shell
hooklings check crux_test

# Run just the formatting check
hooklings check crux_fmt_check
```

### Dry Run (Preview)

```bash
hooklings preflight --dry-run
```

Shows which checks would run without executing them.

## Pipeline Steps

| Step | Purpose | Command | Required |
|------|---------|---------|----------|
| `detect_shell` | Detect shell environment | — | Optional |
| `check_tools` | Verify cargo, rustfmt, clippy available | — | Optional |
| `check_pwd` | Validate working directory | — | Optional |
| `git::status` | Check uncommitted changes | — | Optional |
| `git::log` | Inspect recent commits | — | Optional |
| `op::auth_check` | Verify 1Password auth (for BAML tests) | — | Optional |
| `crux_fmt_check` | Rust formatting | `just fmt` | **Required** |
| `crux_lint` | Clippy lints | `just lint` | **Required** |
| `crux_test` | Test suite | `just test` | **Required** |
| `crux_doc_tests` | Documentation tests | `cargo test --doc` | **Required** |

**Optional steps** continue on failure (report status but don't block)  
**Required steps** block on failure

## Pre-Commit Hook Integration

To run hooklings automatically before every commit, create a Git pre-commit hook:

```bash
# .git/hooks/pre-commit (executable)
#!/bin/bash
hooklings preflight || exit 1
```

Or update the repository's `.githooks/` directory and run:

```bash
just hooks
```

## Configuration

**File**: `hooklings.crux`  
**Format**: Crux DSL  
**Location**: Repository root

To modify checks:
1. Edit `hooklings.crux`
2. Run `hooklings check <step>` to test individual steps
3. Run `hooklings preflight` to verify the full pipeline
4. Commit the updated config

## Common Workflows

### Before a Feature Branch Commit

```bash
hooklings preflight
git commit -m "feat: description"
```

### While Developing (Quick Test)

```bash
# Run just the fast checks
hooklings check git::status
hooklings check crux_fmt_check
```

### Full Pre-Release Validation

```bash
# Runs all checks including docs and all tests
hooklings preflight
```

### Debugging a Failed Check

```bash
# Run a specific failing check with output
hooklings check crux_test

# Or run the command directly to see details
just test
```

## Troubleshooting

### "Command not found: hooklings"
Install hooklings:
```bash
cargo install --path ~/.local/src/hooklings
```

Or use the full path:
```bash
~/.cargo/bin/hooklings preflight
```

### "1Password auth check failed"
This is optional and only matters if running BAML-dependent tests. Skip with `--skip op::auth_check` or unlock 1Password.

### "cargo fmt check failed"
Run the fix:
```bash
just fix
```

Then re-run the preflight.

### "Test suite failed"
Run tests with details:
```bash
just test
```

Or for a specific crate:
```bash
cargo nextest run -p crux-runtime
```

## Integration with CI/CD

In GitHub Actions or other CI, run hooklings as a gate before allowing commits:

```yaml
- name: Run preflight checks
  run: |
    cd /Users/joe/dev/crux
    hooklings preflight
```

## References

- **Crux Build Commands**: `CLAUDE.md` → "Build Commands" section
- **Hooklings CLI**: `hooklings --help`
- **Crux Preflight Config**: `hooklings.crux` (this directory's root)
- **Hooklings Documentation**: `/Users/joe/dev/hooklings/docs/` (hooklings repo)
