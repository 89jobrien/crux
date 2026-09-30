#!/usr/bin/env bash
# Uncredentialed Crux CLI smoke test. Resolves and enters the repository root.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$ROOT"
BIN="$ROOT/target/debug/crux"

echo "== build =="
cargo build -p crux-cli

echo "== help =="
"$BIN" --help

echo "== list =="
"$BIN" list examples

echo "== validate =="
"$BIN" run examples/read_and_pick.crux --check

echo "== execute uncredentialed example =="
"$BIN" run examples/read_and_pick.crux examples/input_read_and_pick.json

echo "== rule planner =="
"$BIN" plan --goal "fetch data and summarize it"

echo "== smoke OK =="
