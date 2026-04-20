#!/usr/bin/env bash
# Example: generate a pipeline from a goal, then execute it.
#
# Requires the `baml` feature and API keys injected for LLM inference (I use dotenvx)
# See CLAUDE.local.md for the dotenvx incantation.

set -euo pipefail

CRUX="./target/debug/cruxx"
INPUT="examples/input_plugin_plan.json"
GOAL="read a markdown plan file and decompose it into implementation tasks"

# Generate a pipeline (default: yaml output)
$CRUX plan --goal "$GOAL" -o /tmp/plan.yaml
echo "--- Generated pipeline ---"
cat /tmp/plan.yaml

# Pretty output with header
echo ""
echo "--- Pretty ---"
$CRUX plan --goal "$GOAL" --output-type pretty

# Dry-run: show step names without executing
echo ""
echo "--- Dry run ---"
$CRUX plan --goal "$GOAL" --output-type dry-run

# JSON output
echo ""
echo "--- JSON ---"
$CRUX plan --goal "$GOAL" --output-type json

# HANDOFF output
echo ""
echo "--- HANDOFF ---"
$CRUX plan --goal "$GOAL" --output-type handoff

# Execute the generated pipeline with input
echo ""
echo "--- Execute ---"
$CRUX run /tmp/plan.yaml "$INPUT"

# Pipe mode: plan | run
echo ""
echo "--- Pipe mode ---"
$CRUX plan --goal "$GOAL" | $CRUX run - "$INPUT"
