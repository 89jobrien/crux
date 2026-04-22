#!/usr/bin/env bats
load "${BATS_TEST_DIRNAME}/helpers"

setup() {
    rm -rf "$BATS_TMPDIR/repo"
    export GIT_STUB_CHANGED=""
    unset PRE_PUSH_SHARED_CRATES
    unset PRE_PUSH_CONFORMANCE_TEST
    unset STUB_RECORD
    unset GIT_STUB_TOPLEVEL
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
    [ "$status" -eq 0 ]
    [ -f "$REPO/crates/cruxx-agentic/src/baml_client/mod.rs" ]
}

@test "pre-commit: staged .rs file invokes cargo fmt check" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
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
    export STUB_RECORD="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/cargo"
    chmod +x "$STUBS_DIR/cargo"
    fake_stage "src/main.rs" "fn main(){}"
    run run_pre_commit
    [ "$status" -eq 0 ]
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
    export STUB_RECORD="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/prettier"
    chmod +x "$STUBS_DIR/prettier"
    fake_stage "README.md" "# hello"
    run run_pre_commit
    [ "$status" -eq 0 ]
    grep -q "markdown" "$STUB_RECORD"
}

@test "pre-commit: staged .yaml file invokes prettier yaml" {
    setup_repo
    export STUB_RECORD="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    rm -f "$STUB_RECORD"
    printf '%s\n' '#!/usr/bin/env bash' 'echo "$*" >> "$STUB_RECORD"' 'exit 0' > "$STUBS_DIR/prettier"
    chmod +x "$STUBS_DIR/prettier"
    fake_stage "config.yaml" "key: value"
    run run_pre_commit
    [ "$status" -eq 0 ]
    grep -q "yaml" "$STUB_RECORD"
}

# ── pre-push ─────────────────────────────────────────────────────────────────

# Helper: build a push ref line for run_pre_push
push_ref() {
    echo "refs/heads/main $1 refs/heads/main $2"
}

ZERO="0000000000000000000000000000000000000000"
LOCAL="aabbccddeeff00112233445566778899aabbccdd"
REMOTE="11223344556677889900aabbccddeeff11223344"

@test "pre-push: no lintable changed files exits 0" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="docs/notes.txt"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    [[ "$output" == *"No lintable changes"* ]]
}

@test "pre-push: new branch (zero remote OID) runs checks" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="src/main.rs"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $ZERO)"
    [ "$status" -eq 0 ]
    grep -q "check" "$record"
}

@test "pre-push: deleted branch (zero local OID) exits 0 with no checks" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="src/main.rs"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $ZERO $REMOTE)"
    [ "$status" -eq 0 ]
    ! grep -q "check" "$record" 2>/dev/null
}

@test "pre-push: changed .rs triggers cargo check --workspace" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="src/main.rs"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    grep -q "check --workspace" "$record"
}

@test "pre-push: changed crate dir triggers per-crate clippy" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    mkdir -p "$REPO/mycrate"
    printf '[package]\nname = "mycrate"\nversion = "0.1.0"\nedition = "2021"\n' \
        > "$REPO/mycrate/Cargo.toml"
    export GIT_STUB_CHANGED="mycrate/src/lib.rs"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    grep -q "clippy" "$record"
}

@test "pre-push: PRE_PUSH_SHARED_CRATES triggers workspace-wide clippy" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    mkdir -p "$REPO/shared-core"
    printf '[package]\nname = "shared-core"\nversion = "0.1.0"\nedition = "2021"\n' \
        > "$REPO/shared-core/Cargo.toml"
    export GIT_STUB_CHANGED="shared-core/src/lib.rs"
    export PRE_PUSH_SHARED_CRATES="shared-core"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    grep -q -- "--workspace" "$record"
}

@test "pre-push: PRE_PUSH_CONFORMANCE_TEST='' disables conformance run" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="src/main.rs"
    export PRE_PUSH_CONFORMANCE_TEST=""
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/cargo" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/cargo"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    if [[ -f "$record" ]]; then
        ! grep -q "nextest" "$record"
    fi
}

@test "pre-push: changed .sh triggers shellcheck" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="scripts/deploy.sh"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/shellcheck" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/shellcheck"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    grep -q "deploy.sh" "$record"
}

@test "pre-push: changed .md triggers markdownlint" {
    setup_repo
    export GIT_STUB_TOPLEVEL="$REPO"
    export GIT_STUB_CHANGED="README.md"
    local record="$BATS_TMPDIR/${BATS_TEST_NAME//[^a-zA-Z0-9]/_}"
    export STUB_RECORD="$record"
    cat > "$STUBS_DIR/markdownlint-cli2" <<'EOF'
#!/usr/bin/env bash
echo "$*" >> "$STUB_RECORD"
exit 0
EOF
    chmod +x "$STUBS_DIR/markdownlint-cli2"
    run run_pre_push "$(push_ref $LOCAL $REMOTE)"
    [ "$status" -eq 0 ]
    grep -q "README.md" "$record"
}
