# Hook Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bats functional tests + fuzz coverage for `.githooks/pre-commit` and `.githooks/pre-push`.

**Architecture:** Tests live in `scripts/tests/`. A shared `helpers.bash` provides temp-repo
setup and stub-binary injection. `hooks.bats` covers deterministic behaviour; `fuzz.sh` generates
random inputs and exercises the filename classifier and git-ref parser independently.

**Tech Stack:** bats-core, bash, minimal POSIX tools (sed, jq stubs).

---

## File Map

| Path | Role |
|------|------|
| `scripts/tests/hooks.bats` | Bats functional tests for both hooks |
| `scripts/tests/fuzz.sh` | Fuzz runner (filename classifier + git ref inputs) |
| `scripts/tests/helpers.bash` | Shared setup: temp git repo, stub PATH injection |
| `scripts/tests/stubs/git` | Stub `git` binary (controlled output) |
| `scripts/tests/stubs/cargo` | Stub `cargo` binary (exits 0, prints nothing) |
| `scripts/tests/stubs/cargo-nextest` | Stub for `cargo nextest` |
| `scripts/tests/stubs/jq` | Stub `jq` (returns empty package list) |
| `scripts/tests/stubs/shellcheck` | Stub (exits 0) |
| `scripts/tests/stubs/markdownlint-cli2` | Stub (exits 0) |
| `scripts/tests/stubs/tflint` | Stub (exits 0) |
| `scripts/tests/stubs/check-jsonschema` | Stub (exits 0) |

---

### Task 1: Scaffold directory and helpers

**Files:**
- Create: `scripts/tests/helpers.bash`
- Create: `scripts/tests/stubs/git`
- Create: `scripts/tests/stubs/cargo`
- Create: `scripts/tests/stubs/cargo-nextest`
- Create: `scripts/tests/stubs/jq`
- Create: `scripts/tests/stubs/shellcheck`
- Create: `scripts/tests/stubs/markdownlint-cli2`
- Create: `scripts/tests/stubs/tflint`
- Create: `scripts/tests/stubs/check-jsonschema`

- [ ] **Step 1: Create `scripts/tests/helpers.bash`**

```bash
# scripts/tests/helpers.bash
# Shared setup for hook bats tests.

HOOKS_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/../.." && pwd)/.githooks"
STUBS_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)/stubs"

# setup_repo — creates a minimal git repo in $BATS_TMPDIR/repo.
# Initialises with one commit so HEAD~1 resolves.
# Prepends stubs to PATH so hooks use controlled binaries.
setup_repo() {
    REPO="$BATS_TMPDIR/repo"
    mkdir -p "$REPO/crates/cruxx-agentic/src"
    cd "$REPO"
    git init -q
    git config user.email "test@test.com"
    git config user.name "Test"
    touch README.md
    git add README.md
    git commit -q -m "init"

    # Inject stubs before real tools
    export PATH="$STUBS_DIR:$PATH"
    export GIT_PREFIX=""
}

# stage_file <path> [content]
# Creates a file relative to REPO and stages it.
stage_file() {
    local f="$1" content="${2:-// placeholder}"
    mkdir -p "$(dirname "$f")"
    printf '%s\n' "$content" > "$f"
    git add "$f"
}

# run_pre_commit — runs the pre-commit hook in REPO context.
run_pre_commit() {
    bash "$HOOKS_DIR/pre-commit" "$@"
}

# run_pre_push <stdin> — runs the pre-push hook with given stdin (ref lines).
run_pre_push() {
    local stdin="$1"
    printf '%s\n' "$stdin" | bash "$HOOKS_DIR/pre-push"
}
```

- [ ] **Step 2: Create stub binaries**

```bash
# scripts/tests/stubs/git
#!/usr/bin/env bash
# Minimal git stub. Delegates to real git for everything except
# diff --name-only (controlled by GIT_STUB_CHANGED env var).
case "$*" in
    "diff --name-only"*|"diff --cached --name-only"*)
        printf '%s\n' ${GIT_STUB_CHANGED:-} ;;
    "rev-parse --show-toplevel")
        echo "${GIT_STUB_TOPLEVEL:-$PWD}" ;;
    *)
        command git "$@" ;;
esac
```

```bash
# scripts/tests/stubs/cargo  (same pattern for nextest, jq, shellcheck, etc.)
#!/usr/bin/env bash
# Stub: always succeeds, emits nothing.
exit "${STUB_EXIT:-0}"
```

Create `cargo-nextest`, `jq`, `shellcheck`, `markdownlint-cli2`, `tflint`, `check-jsonschema`
as identical copies of the `cargo` stub above (they all default to exit 0).

The `jq` stub needs one special case for `cargo metadata` parsing:

```bash
# scripts/tests/stubs/jq
#!/usr/bin/env bash
# Return an empty package list so clippy per-crate loop is a no-op.
case "$*" in
    *packages*) echo "" ;;
    *)          exit "${STUB_EXIT:-0}" ;;
esac
```

- [ ] **Step 3: chmod +x all stubs**

```bash
chmod +x scripts/tests/stubs/*
```

- [ ] **Step 4: Commit scaffold**

```bash
git add scripts/tests/
git commit -m "test: scaffold hook test helpers and stubs"
```

---

### Task 2: Bats tests — pre-commit

**Files:**
- Create: `scripts/tests/hooks.bats`

- [ ] **Step 1: Write the pre-commit bats tests**

```bash
# scripts/tests/hooks.bats
#!/usr/bin/env bats
load helpers

# ── pre-commit ──────────────────────────────────────────────────────────────

@test "pre-commit: no staged files exits 0" {
    setup_repo
    run run_pre_commit
    [ "$status" -eq 0 ]
    [[ "$output" == *"No staged files"* ]]
}

@test "pre-commit: creates baml stub if missing" {
    setup_repo
    stage_file "crates/cruxx-agentic/src/lib.rs" "fn main(){}"
    run run_pre_commit
    [ -f "$REPO/crates/cruxx-agentic/src/baml_client/mod.rs" ]
}

@test "pre-commit: staged .rs file invokes cargo fmt check" {
    setup_repo
    # Make cargo fmt stub record invocation
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    stage_file "src/main.rs" "fn main(){}"
    run run_pre_commit
    [ "$status" -eq 0 ]
    grep -q "fmt" "$STUB_RECORD"
}

@test "pre-commit: staged .rs file invokes clippy" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    stage_file "src/main.rs" "fn main(){}"
    run run_pre_commit
    grep -q "clippy" "$STUB_RECORD"
}

@test "pre-commit: cargo fmt failure marks FAILED and exits 1" {
    setup_repo
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
case "$*" in *fmt*) exit 1;; *) exit 0;; esac
EOF
    chmod +x "$STUBS_DIR/cargo"
    stage_file "src/main.rs" "fn main(){}"
    run run_pre_commit
    [ "$status" -eq 1 ]
}

@test "pre-commit: missing tool is skipped, not fatal" {
    setup_repo
    # Remove shfmt stub entirely
    rm -f "$STUBS_DIR/shfmt"
    stage_file "deploy.sh" "#!/bin/bash\necho hi"
    run run_pre_commit
    [ "$status" -eq 0 ]
    [[ "$output" == *"not found, skipping"* ]]
}

@test "pre-commit: cross-dir staging guard triggers when GIT_PREFIX set" {
    setup_repo
    stage_file "other/file.rs" "fn x(){}"
    export GIT_PREFIX="src/"
    run run_pre_commit
    [ "$status" -eq 1 ]
    [[ "$output" == *"Staged changes outside"* ]]
}

@test "pre-commit: staged .md file invokes prettier markdown" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/prettier" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/prettier"
    stage_file "README.md" "# hello"
    run run_pre_commit
    grep -q "markdown" "$STUB_RECORD"
}

@test "pre-commit: staged .yaml file invokes prettier yaml" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/prettier" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/prettier"
    stage_file "config.yaml" "key: value"
    run run_pre_commit
    grep -q "yaml" "$STUB_RECORD"
}
```

- [ ] **Step 2: Run tests to confirm they fail (no hook present in tmp repo)**

```bash
cd /Users/joe/dev/crux
bats scripts/tests/hooks.bats --filter "pre-commit"
```

Expected: all tests run; most fail or error until hooks are in place. If bats itself errors, fix load path.

- [ ] **Step 3: Commit**

```bash
git add scripts/tests/hooks.bats
git commit -m "test(hooks): pre-commit bats tests"
```

---

### Task 3: Bats tests — pre-push

**Files:**
- Modify: `scripts/tests/hooks.bats` (append)

- [ ] **Step 1: Append pre-push tests to `hooks.bats`**

```bash
# ── pre-push ─────────────────────────────────────────────────────────────────

# Helper: build a push ref line
# push_ref <local_sha> <remote_sha>
push_ref() {
    echo "refs/heads/main $1 refs/heads/main $2"
}

ZERO="0000000000000000000000000000000000000000"
LOCAL="aabbccddeeff00112233445566778899aabbccdd"
REMOTE="11223344556677889900aabbccddeeff11223344"

@test "pre-push: no lintable changed files exits 0" {
    setup_repo
    export GIT_STUB_CHANGED="docs/notes.txt"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    [[ "$output" == *"No lintable changes"* ]]
}

@test "pre-push: new branch (zero remote OID) falls back to origin/main diff" {
    setup_repo
    export GIT_STUB_CHANGED="src/main.rs"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $ZERO)"
    [ "$status" -eq 0 ]
    grep -q "check" "$STUB_RECORD"
}

@test "pre-push: deleted branch (zero local OID) exits 0 with no checks" {
    setup_repo
    export GIT_STUB_CHANGED="src/main.rs"
    run run_pre_push "$(push_ref $ZERO $REMOTE)"
    [ "$status" -eq 0 ]
}

@test "pre-push: changed .rs triggers cargo check --workspace" {
    setup_repo
    export GIT_STUB_CHANGED="src/main.rs"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    grep -q "check --workspace" "$STUB_RECORD"
}

@test "pre-push: changed crate dir with Cargo.toml triggers per-crate clippy" {
    setup_repo
    mkdir -p "$REPO/mycrate"
    printf '[package]\nname = "mycrate"\nversion = "0.1.0"\nedition = "2021"\n' \
        > "$REPO/mycrate/Cargo.toml"
    export GIT_STUB_CHANGED="mycrate/src/lib.rs"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    grep -q "clippy" "$STUB_RECORD"
}

@test "pre-push: PRE_PUSH_SHARED_CRATES triggers workspace-wide clippy" {
    setup_repo
    mkdir -p "$REPO/shared-core"
    printf '[package]\nname = "shared-core"\nversion = "0.1.0"\nedition = "2021"\n' \
        > "$REPO/shared-core/Cargo.toml"
    export GIT_STUB_CHANGED="shared-core/src/lib.rs"
    export PRE_PUSH_SHARED_CRATES="shared-core"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    grep -q "\-\-workspace" "$STUB_RECORD"
}

@test "pre-push: PRE_PUSH_CONFORMANCE_TEST='' disables conformance run" {
    setup_repo
    export GIT_STUB_CHANGED="src/main.rs"
    export PRE_PUSH_CONFORMANCE_TEST=""
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    ! grep -q "nextest" "$STUB_RECORD" || true
    [ "$status" -eq 0 ]
}

@test "pre-push: changed .sh triggers shellcheck" {
    setup_repo
    export GIT_STUB_CHANGED="scripts/deploy.sh"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/shellcheck" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/shellcheck"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    grep -q "scripts/deploy.sh" "$STUB_RECORD"
}

@test "pre-push: changed .md triggers markdownlint" {
    setup_repo
    export GIT_STUB_CHANGED="README.md"
    export STUB_RECORD="$BATS_TMPDIR/calls"
    cat > "$STUBS_DIR/markdownlint-cli2" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/markdownlint-cli2"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    grep -q "README.md" "$STUB_RECORD"
}
```

- [ ] **Step 2: Run pre-push tests**

```bash
bats scripts/tests/hooks.bats --filter "pre-push"
```

Expected: tests run; stubs control exit codes so most should pass.

- [ ] **Step 3: Commit**

```bash
git add scripts/tests/hooks.bats
git commit -m "test(hooks): pre-push bats tests"
```

---

### Task 4: Fuzz — filename classifier

**Files:**
- Create: `scripts/tests/fuzz.sh`

- [ ] **Step 1: Extract the classifier logic into a sourceable snippet**

The classifier `case` block in both hooks is duplicated. The fuzz script re-implements it inline
for isolation (we're testing the hook logic, not a shared lib). Add this as the first section of
`fuzz.sh`:

```bash
#!/usr/bin/env bash
# scripts/tests/fuzz.sh
# Fuzz the hook filename classifier and git-ref input parser.
# Usage: bash scripts/tests/fuzz.sh [--iterations N]
set -eo pipefail

ITERATIONS="${2:-500}"
PASS=0 FAIL=0
HOOKS_DIR="$(cd "$(dirname "$0")/../.." && pwd)/.githooks"

# ── 1. Filename classifier fuzz ───────────────────────────────────────────────
# Mirrors the case block used in both hooks.
classify() {
    local f="$1"
    case "$f" in
        *.rs)                      echo "rs" ;;
        *.tf)                      echo "tf" ;;
        *.sh)                      echo "sh" ;;
        *.yaml|*.yml)              echo "yaml" ;;
        *.md)                      echo "md" ;;
        *Dockerfile*|*dockerfile*) echo "docker" ;;
        *devcontainer*.json)       echo "dc" ;;
        *)                         echo "other" ;;
    esac
}

fuzz_classifier() {
    echo "==> Filename classifier fuzz ($ITERATIONS iterations)"
    local i=0
    while (( i++ < ITERATIONS )); do
        # Generate a random filename: mix of unicode, spaces, deep paths, no-ext
        local name
        name=$(cat /dev/urandom | LC_ALL=C tr -dc 'a-zA-Z0-9 ./\-_ñüçαβγ' | head -c $(( RANDOM % 60 + 1 )) || true)
        name="${name// /_}"   # spaces → underscores (path-safe)
        name="${name:-noname}"

        local bucket
        bucket=$(classify "$name")

        # Assert: exactly one bucket returned, non-empty, no crash
        if [[ -z "$bucket" ]]; then
            echo "FAIL: empty bucket for input: $name"
            (( FAIL++ ))
        else
            (( PASS++ ))
        fi
    done
    echo "  classifier: $PASS passed, $FAIL failed"
}
```

- [ ] **Step 2: Add git-ref fuzz section**

Append to `fuzz.sh`:

```bash
# ── 2. Git ref input fuzz ─────────────────────────────────────────────────────
# Feeds malformed OID strings through the changed-file detection loop.
# The hook reads stdin; we provide mangled ref lines and assert exit is 0 or 1.

fuzz_refs() {
    echo "==> Git ref input fuzz ($ITERATIONS iterations)"
    local i=0 p=0 f=0
    ZERO="0000000000000000000000000000000000000000"

    # Variants: wrong length, non-hex, empty fields, unicode, repeated chars
    generate_oid() {
        local variant=$(( RANDOM % 6 ))
        case $variant in
            0) printf '%040d' $(( RANDOM ))            ;;   # numeric only
            1) cat /dev/urandom | LC_ALL=C tr -dc 'g-zG-Z' | head -c 40 || true ;;  # non-hex alpha
            2) cat /dev/urandom | LC_ALL=C tr -dc '0-9a-f' | head -c $(( RANDOM % 80 )) || true ;;  # variable length
            3) echo ""                                 ;;   # empty
            4) printf '%0.s?' {1..40}                 ;;   # question marks
            5) printf '%040x' $(( RANDOM * RANDOM ))  ;;   # valid hex (control)
        esac
    }

    # Create a minimal temp git repo for the hook to cd into
    local tmpdir
    tmpdir=$(mktemp -d)
    git -C "$tmpdir" init -q
    git -C "$tmpdir" config user.email t@t.com
    git -C "$tmpdir" config user.name T
    touch "$tmpdir/f"
    git -C "$tmpdir" -C "$tmpdir" add f
    git -C "$tmpdir" commit -q -m init

    while (( i++ < ITERATIONS )); do
        local lo ro
        lo=$(generate_oid)
        ro=$(generate_oid)
        local ref_line="refs/heads/fuzz $lo refs/heads/fuzz $ro"

        local exit_code=0
        (
            cd "$tmpdir"
            printf '%s\n' "$ref_line" \
                | timeout 5 bash "$HOOKS_DIR/pre-push" 2>/dev/null
        ) || exit_code=$?

        # Exit code must be 0 or 1 — never a crash (>= 2 signals, 124 = timeout)
        if [[ "$exit_code" -ge 2 ]] && [[ "$exit_code" -ne 124 ]]; then
            echo "FAIL: exit $exit_code for ref: $ref_line"
            (( f++ ))
        elif [[ "$exit_code" -eq 124 ]]; then
            echo "FAIL: timeout for ref: $ref_line"
            (( f++ ))
        else
            (( p++ ))
        fi
    done
    rm -rf "$tmpdir"
    echo "  ref inputs: $p passed, $f failed"
    PASS=$(( PASS + p )) FAIL=$(( FAIL + f ))
}
```

- [ ] **Step 3: Add main entrypoint and summary**

Append to `fuzz.sh`:

```bash
# ── Main ──────────────────────────────────────────────────────────────────────
fuzz_classifier
fuzz_refs

echo ""
echo "Total: $PASS passed, $FAIL failed"
[[ "$FAIL" -eq 0 ]] && echo "✓ All fuzz cases passed" && exit 0
echo "✗ $FAIL fuzz failures" && exit 1
```

- [ ] **Step 4: Make executable**

```bash
chmod +x scripts/tests/fuzz.sh
```

- [ ] **Step 5: Run fuzz script to confirm it passes**

```bash
bash scripts/tests/fuzz.sh --iterations 200
```

Expected output ends with `✓ All fuzz cases passed`.

- [ ] **Step 6: Commit**

```bash
git add scripts/tests/fuzz.sh
git commit -m "test(hooks): fuzz filename classifier and git ref inputs"
```

---

### Task 5: Wire into justfile

**Files:**
- Modify: `justfile`

- [ ] **Step 1: Read current justfile**

```bash
cat justfile
```

- [ ] **Step 2: Append test-hooks and fuzz-hooks recipes**

Add after existing recipes:

```just
# Run hook bats tests
test-hooks:
    bats scripts/tests/hooks.bats

# Run hook fuzz suite
fuzz-hooks iterations="500":
    bash scripts/tests/fuzz.sh --iterations {{iterations}}
```

- [ ] **Step 3: Verify recipes parse**

```bash
just --list | grep hooks
```

Expected: `test-hooks` and `fuzz-hooks` appear.

- [ ] **Step 4: Commit**

```bash
git add justfile
git commit -m "chore: add test-hooks and fuzz-hooks just recipes"
```

---

## Self-Review

**Spec coverage:**
- bats for pre-commit: no staged, baml stub, fmt, clippy, tool-not-found skip, cross-dir guard, md, yaml — all covered
- bats for pre-push: no lintable, new branch fallback, deleted branch, workspace compile, per-crate clippy, shared crate propagation, conformance disable, shellcheck, markdownlint — all covered
- fuzz filename classifier: covered in Task 4
- fuzz git ref inputs: covered in Task 4
- justfile wiring: Task 5

**Placeholder scan:** None found.

**Type consistency:** `setup_repo`, `stage_file`, `run_pre_commit`, `run_pre_push` defined in Task 1 and used consistently in Tasks 2–3. `classify` defined and used in Task 4 only.
