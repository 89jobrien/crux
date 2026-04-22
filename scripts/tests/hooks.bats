#!/usr/bin/env bats
load "${BATS_TEST_DIRNAME}/helpers"

setup() {
    rm -rf "$BATS_TMPDIR/repo"
}

# fake_stage: create a file and tell the git stub it is staged, without running
# `git add` through the stub (which would infinitely recurse via `command git`).
fake_stage() {
    local f="$1" content="${2:-// placeholder}"
    mkdir -p "$(dirname "$f")"
    printf '%s\n' "$content" > "$f"
    export GIT_STUB_CHANGED="${GIT_STUB_CHANGED:+$GIT_STUB_CHANGED }$f"
}

# ── pre-commit ──────────────────────────────────────────────────────────────

@test "pre-commit: no staged files exits 0" {
    setup_repo
    export GIT_STUB_CHANGED=""
    run run_pre_commit
    [ "$status" -eq 0 ]
    [[ "$output" == *"No staged files"* ]]
}

@test "pre-commit: creates baml stub if missing" {
    setup_repo
    rm -f "$REPO/crates/cruxx-agentic/src/baml_client/mod.rs"
    fake_stage "crates/cruxx-agentic/src/lib.rs" "fn main(){}"
    run run_pre_commit
    [ -f "$REPO/crates/cruxx-agentic/src/baml_client/mod.rs" ]
}

@test "pre-commit: staged .rs file invokes cargo fmt check" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls-fmt"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/cargo"
    chmod +x "$STUBS_DIR/cargo"
    fake_stage "src/main.rs" "fn main(){}"
    run run_pre_commit
    [ "$status" -eq 0 ]
    grep -q "fmt" "$STUB_RECORD"
}

@test "pre-commit: staged .rs file invokes clippy" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls-clippy"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/cargo"
    chmod +x "$STUBS_DIR/cargo"
    fake_stage "src/main.rs" "fn main(){}"
    run run_pre_commit
    grep -q "clippy" "$STUB_RECORD"
}

@test "pre-commit: cargo fmt failure marks FAILED and exits 1" {
    setup_repo
    printf '%s\n' '#!/usr/bin/env bash' 'case "$*" in *fmt*) exit 1;; *) exit 0;; esac' > "$STUBS_DIR/cargo"
    chmod +x "$STUBS_DIR/cargo"
    fake_stage "src/main.rs" "fn main(){}"
    run run_pre_commit
    [ "$status" -eq 1 ]
}

@test "pre-commit: missing tool is skipped, not fatal" {
    setup_repo
    rm -f "$STUBS_DIR/shfmt"
    # Restrict PATH so the real shfmt (if installed) is not reachable.
    export PATH="$STUBS_DIR:/usr/bin:/bin"
    fake_stage "deploy.sh" "#!/bin/bash"
    run run_pre_commit
    [ "$status" -eq 0 ]
    [[ "$output" == *"not found, skipping"* ]]
}

@test "pre-commit: cross-dir staging guard triggers when GIT_PREFIX set" {
    setup_repo
    fake_stage "other/file.rs" "fn x(){}"
    export GIT_PREFIX="src/"
    run run_pre_commit
    [ "$status" -eq 1 ]
    [[ "$output" == *"Staged changes outside"* ]]
}

@test "pre-commit: staged .md file invokes prettier markdown" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls-prettier-md"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/prettier"
    chmod +x "$STUBS_DIR/prettier"
    fake_stage "README.md" "# hello"
    run run_pre_commit
    grep -q "markdown" "$STUB_RECORD"
}

@test "pre-commit: staged .yaml file invokes prettier yaml" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/calls-prettier-yaml"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/prettier"
    chmod +x "$STUBS_DIR/prettier"
    fake_stage "config.yaml" "key: value"
    run run_pre_commit
    grep -q "yaml" "$STUB_RECORD"
}
