#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
WORK_DIR="$ROOT/target/fault-resume-demo"
CRUX_BIN="$ROOT/target/debug/crux"
PIPELINE="$ROOT/examples/fault_resume.crux"
SERVER="$ROOT/examples/fault_resume_server.rs"
ADDRESS_FILE="$WORK_DIR/address"
STATE_FILE="$WORK_DIR/server-state.json"
INPUT="$WORK_DIR/input.json"
FAILED_TRACE="$WORK_DIR/failed.json"
RESUMED_TRACE="$WORK_DIR/resumed.json"

for tool in rust-script jq; do
	if ! command -v "$tool" >/dev/null 2>&1; then
		printf 'error: %s is required for this demo\n' "$tool" >&2
		exit 1
	fi
done
if [ ! -x /usr/bin/curl ]; then
	printf 'error: /usr/bin/curl is required for this demo\n' >&2
	exit 1
fi

rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR"

printf '\n== Build and validate ==\n'
cargo build --manifest-path "$ROOT/Cargo.toml" -p crux-cli --bin crux
"$CRUX_BIN" run "$PIPELINE" --check

SERVER_PID=''
cleanup() {
	if [ -n "$SERVER_PID" ]; then
		kill "$SERVER_PID" 2>/dev/null || true
		wait "$SERVER_PID" 2>/dev/null || true
	fi
}
on_signal() {
	trap - EXIT
	cleanup
	exit "$1"
}
trap cleanup EXIT
trap 'on_signal 130' INT
trap 'on_signal 143' TERM

rust-script "$SERVER" \
	--address-file "$ADDRESS_FILE" \
	--state-file "$STATE_FILE" \
	--crux-bin "$CRUX_BIN" \
	--pipeline "$PIPELINE" \
	--input "$INPUT" \
	--work-dir "$WORK_DIR" \
	>"$WORK_DIR/server.log" 2>&1 &
SERVER_PID=$!

attempt=0
while [ ! -s "$ADDRESS_FILE" ]; do
	if ! kill -0 "$SERVER_PID" 2>/dev/null; then
		printf 'error: local HTTP server exited during startup; see %s\n' "$WORK_DIR/server.log" >&2
		exit 1
	fi
	attempt=$((attempt + 1))
	if [ "$attempt" -ge 1200 ]; then
		printf 'error: local HTTP server did not start\n' >&2
		exit 1
	fi
	sleep 0.1
done

IFS= read -r ADDRESS <"$ADDRESS_FILE"
BASE_URL="http://$ADDRESS"

DASHBOARD_HTML=$(/usr/bin/curl --fail --silent --show-error "$BASE_URL/")
case "$DASHBOARD_HTML" in
*'CRUX // FAULT &amp; RESUME'*) ;;
*)
	printf 'error: dashboard route did not return the expected UI\n' >&2
	exit 1
	;;
esac
case "$DASHBOARD_HTML" in
*'id="control-next"'*'id="detail-json"'*) ;;
*)
	printf 'error: dashboard controls or trace inspector are missing\n' >&2
	exit 1
	;;
esac
STATE_JSON=$(/usr/bin/curl --fail --silent --show-error "$BASE_URL/state")
printf '%s' "$STATE_JSON" | jq -e '.phase == "idle"' >/dev/null
METHOD_STATUS=$(/usr/bin/curl --silent --output /dev/null --write-out '%{http_code}' \
	"$BASE_URL/control/reset")
if [ "$METHOD_STATUS" != "405" ]; then
	printf 'error: control endpoint accepted GET with status %s\n' "$METHOD_STATUS" >&2
	exit 1
fi

post_control() {
	/usr/bin/curl --fail --silent --show-error --request POST \
		"$BASE_URL/control/$1" >/dev/null
}

wait_for_state() {
	filter=$1
	attempt=0
	until jq -e "$filter" "$STATE_FILE" >/dev/null 2>&1; do
		if ! kill -0 "$SERVER_PID" 2>/dev/null; then
			printf 'error: local HTTP server exited during the demo\n' >&2
			exit 1
		fi
		attempt=$((attempt + 1))
		if [ "$attempt" -ge 300 ]; then
			printf 'error: dashboard state did not satisfy %s\n' "$filter" >&2
			exit 1
		fi
		sleep 0.05
	done
}

printf '\nDashboard: %s/\n' "$BASE_URL"
if [ "${CRUX_DEMO_NO_OPEN:-0}" != "1" ]; then
	/usr/bin/open "$BASE_URL/"
else
	printf 'Headless mode: browser launch skipped.\n'
fi

printf '\n== Run interactive fault and replay sequence ==\n'
if [ "${CRUX_DEMO_NO_OPEN:-0}" = "1" ]; then
	INVALID_STATUS=$(/usr/bin/curl --silent --output /dev/null --write-out '%{http_code}' \
		--request POST "$BASE_URL/control/replay")
	if [ "$INVALID_STATUS" != "409" ]; then
		printf 'error: replay from idle returned %s instead of 409\n' "$INVALID_STATUS" >&2
		exit 1
	fi
	OVERSIZED_BODY=$(jq -nr '"x" * 65537')
	BODY_STATUS=$(printf '%s' "$OVERSIZED_BODY" |
		/usr/bin/curl --silent --output /dev/null --write-out '%{http_code}' \
			--request POST --data-binary @- "$BASE_URL/control/next")
	if [ "$BODY_STATUS" != "413" ]; then
		printf 'error: oversized control body returned %s instead of 413\n' "$BODY_STATUS" >&2
		exit 1
	fi

	post_control next
	wait_for_state '.phase == "ready" and (.current_trace | length) == 1 and .busy == false'
	post_control next
	wait_for_state '.phase == "ready" and (.current_trace | length) == 2 and .busy == false'
	post_control next
	wait_for_state '.busy == false and .finalize_in_flight == true'
	RESET_STATUS=$(/usr/bin/curl --silent --output /dev/null --write-out '%{http_code}' \
		--request POST "$BASE_URL/control/reset")
	if [ "$RESET_STATUS" != "409" ]; then
		printf 'error: reset during finalize returned %s instead of 409\n' "$RESET_STATUS" >&2
		exit 1
	fi
	wait_for_state '.phase == "faulted" and (.failed_trace | length) == 3 and .can_replay == true'
	post_control replay
	wait_for_state '.phase == "complete" and .busy == false'
	post_control reset
	wait_for_state '.phase == "idle" and .busy == false'
fi

post_control run-all
wait_for_state '.phase == "complete" and .busy == false'

printf '\n== Failed trace ==\n'
"$CRUX_BIN" trace "$FAILED_TRACE"
printf '\n== Resumed trace ==\n'
"$CRUX_BIN" trace "$RESUMED_TRACE"

jq -e '.phase == "complete" and .context == 1 and .evaluate == 1 and .finalize == 2' \
	"$STATE_FILE" >/dev/null
FINAL_STATE=$(/usr/bin/curl --fail --silent --show-error "$BASE_URL/state")
printf '%s' "$FINAL_STATE" |
	jq -e '.phase == "complete" and .context == 1 and .evaluate == 1 and .finalize == 2' \
		>/dev/null
jq -e '[.steps[].origin] == ["replayed", "replayed", "live"]' \
	"$RESUMED_TRACE" >/dev/null
jq -e '[.steps[0].duration_ms, .steps[1].duration_ms] == [0, 0]' \
	"$RESUMED_TRACE" >/dev/null
jq -e '[.current_trace[].origin] == ["replayed", "replayed", "live"]' \
	"$STATE_FILE" >/dev/null
jq -e '[.failed_trace[].status] == ["ok", "ok", "err"]' \
	"$STATE_FILE" >/dev/null

printf '\nPASS: Steps 1 and 2 were replayed at 0ms; only Step 3 made a second HTTP call.\n'
printf 'Artifacts: %s\n' "$WORK_DIR"

if [ "${CRUX_DEMO_NO_OPEN:-0}" != "1" ]; then
	printf 'Dashboard remains live. Press Ctrl-C to stop.\n'
	while kill -0 "$SERVER_PID" 2>/dev/null; do
		sleep 1
	done
fi
