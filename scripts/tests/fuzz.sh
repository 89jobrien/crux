#!/usr/bin/env bash
# Fuzz the hook filename classifier and git-ref input parser.
# Usage: bash scripts/tests/fuzz.sh [--iterations N]
# GIT_STUB_CHANGED: space-separated list of changed paths (no paths with spaces).
set -eo pipefail

ITERATIONS="${2:-500}"
PASS=0 FAIL=0
HOOKS_DIR="$(cd "$(dirname "$0")/../.." && pwd)/.githooks"

# Portable timeout wrapper: uses system timeout/gtimeout if available, else perl alarm.
run_timeout() {
    local secs=$1; shift
    if command -v timeout &>/dev/null; then
        timeout "$secs" "$@"
    elif command -v gtimeout &>/dev/null; then
        gtimeout "$secs" "$@"
    else
        perl -e 'alarm $ARGV[0]; exec @ARGV[1..$#ARGV]' "$secs" "$@"
    fi
}

# ── 1. Filename classifier fuzz ───────────────────────────────────────────────
# Mirrors the case block in both hooks.
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
    local i=0 pass=0 fail=0
    while (( i++ < ITERATIONS )); do
        local name
        name=$(LC_ALL=C tr -dc 'a-zA-Z0-9./\-_' < /dev/urandom | head -c $(( RANDOM % 60 + 1 )) 2>/dev/null || true)
        name="${name:-noname}"

        local bucket
        bucket=$(classify "$name")

        if [[ -z "$bucket" ]]; then
            echo "FAIL: empty bucket for input: $name"
            fail=$(( fail + 1 ))
        else
            pass=$(( pass + 1 ))
        fi
    done
    echo "  classifier: $pass passed, $fail failed"
    PASS=$(( PASS + pass )) FAIL=$(( FAIL + fail ))
}

# ── 2. Git ref input fuzz ─────────────────────────────────────────────────────
# Feeds malformed OID strings through pre-push via stdin.
# Asserts exit code is 0 or 1 only (no crashes, no hangs).

fuzz_refs() {
    echo "==> Git ref input fuzz ($ITERATIONS iterations)"
    local i=0 p=0 f=0

    generate_oid() {
        local variant=$(( RANDOM % 6 ))
        case $variant in
            0) printf '%040d' $(( RANDOM )) ;;
            1) LC_ALL=C tr -dc 'g-zG-Z' < /dev/urandom | head -c 40 2>/dev/null || true ;;
            2) LC_ALL=C tr -dc '0-9a-f' < /dev/urandom | head -c $(( RANDOM % 80 )) 2>/dev/null || true ;;
            3) echo "" ;;
            4) printf '%0.s?' {1..40} ;;
            5) printf '%040x' $(( RANDOM * RANDOM )) ;;
        esac
    }

    local tmpdir
    tmpdir=$(mktemp -d)
    GIT_CONFIG_NOSYSTEM=1 HOME="$tmpdir" git -C "$tmpdir" init -q
    GIT_CONFIG_NOSYSTEM=1 HOME="$tmpdir" git -C "$tmpdir" config user.email t@t.com
    GIT_CONFIG_NOSYSTEM=1 HOME="$tmpdir" git -C "$tmpdir" config user.name T
    touch "$tmpdir/f"
    GIT_CONFIG_NOSYSTEM=1 HOME="$tmpdir" git -C "$tmpdir" add f
    GIT_CONFIG_NOSYSTEM=1 HOME="$tmpdir" git -C "$tmpdir" -c core.hooksPath=/dev/null commit -q -m init

    while (( i++ < ITERATIONS )); do
        local lo ro
        lo=$(generate_oid)
        ro=$(generate_oid)
        local ref_line="refs/heads/fuzz $lo refs/heads/fuzz $ro"

        local exit_code=0
        (
            cd "$tmpdir"
            export HOME="$tmpdir" GIT_CONFIG_NOSYSTEM=1
            printf '%s\n' "$ref_line" \
                | run_timeout 5 bash "$HOOKS_DIR/pre-push" 2>/dev/null
        ) || exit_code=$?

        # 124 = timeout exit code; 142 = SIGALRM (perl timeout); both are timeout signals.
        if [[ "$exit_code" -eq 124 || "$exit_code" -eq 142 ]]; then
            echo "FAIL: timeout for ref: $ref_line"
            f=$(( f + 1 ))
        elif [[ "$exit_code" -ge 2 ]]; then
            echo "FAIL: exit $exit_code for ref: $ref_line"
            f=$(( f + 1 ))
        else
            p=$(( p + 1 ))
        fi
    done
    rm -rf "$tmpdir"
    echo "  ref inputs: $p passed, $f failed"
    PASS=$(( PASS + p )) FAIL=$(( FAIL + f ))
}

# ── Main ──────────────────────────────────────────────────────────────────────
fuzz_classifier
fuzz_refs

echo ""
echo "Total: $PASS passed, $FAIL failed"
if [[ "$FAIL" -eq 0 ]]; then
    echo "✓ All fuzz cases passed"
    exit 0
fi
echo "✗ $FAIL fuzz failures"
exit 1
