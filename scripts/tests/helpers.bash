# scripts/tests/helpers.bash
# Shared setup for hook bats tests.

HOOKS_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")/../.." && pwd)/.githooks"
STUBS_DIR="$(cd "$(dirname "$BATS_TEST_FILENAME")" && pwd)/stubs"

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
    # Real git used for setup above; stub takes over after PATH export below.
    export PATH="$STUBS_DIR:$PATH"
    export GIT_PREFIX=""
}

stage_file() {
    local f="$1" content="${2:-// placeholder}"
    mkdir -p "$(dirname "$f")"
    printf '%s\n' "$content" > "$f"
    git add "$f"
}

run_pre_commit() {
    bash "$HOOKS_DIR/pre-commit" "$@"
}

run_pre_push() {
    local stdin="$1"
    # stdin format: "<local-ref> <local-sha1> <remote-ref> <remote-sha1>"
    printf '%s\n' "$stdin" | bash "$HOOKS_DIR/pre-push"
}
