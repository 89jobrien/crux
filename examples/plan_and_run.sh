#!/usr/bin/env bash
# Example: generate a pipeline from a goal, then execute it.
#
# Requires the `baml` feature and API keys injected via dotenvx.
# See CLAUDE.local.md for the dotenvx incantation.

set -euo pipefail

CRUX="./target/debug/crux"

# Generate a pipeline (default: yaml output)
$CRUX plan --goal "read a file and extract entities" -o /tmp/plan.yaml
echo "--- Generated pipeline ---"
cat /tmp/plan.yaml

# Pretty output with header
echo ""
echo "--- Pretty ---"
$CRUX plan --goal "read a file and extract entities" --output-type pretty

# Dry-run: show step names without executing
echo ""
echo "--- Dry run ---"
$CRUX plan --goal "read a file and extract entities" --output-type dry-run

# JSON output
echo ""
echo "--- JSON ---"
$CRUX plan --goal "read a file and extract entities" --output-type json

# HANDOFF output
echo ""
echo "--- HANDOFF ---"
$CRUX plan --goal "read a file and extract entities" --output-type handoff

# Execute the generated pipeline
echo ""
echo "--- Execute ---"
$CRUX run /tmp/plan.yaml

# Pipe mode: plan | run
echo ""
echo "--- Pipe mode ---"
$CRUX plan --goal "summarize text" | $CRUX run -
