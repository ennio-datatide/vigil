#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: reply.sh --session-id <id> --message <text>

Send a reply to a worker that needs user input, then wait for completion.

Arguments:
  --session-id <id>       Session ID of the worker (required)
  --message <text>        The user's answer to relay (required)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
SESSION_ID=""
MESSAGE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --session-id)
            SESSION_ID="$2"
            shift 2
            ;;
        --message)
            MESSAGE="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

# Validate required arguments
if [[ -z "$SESSION_ID" ]]; then
    echo "Error: --session-id is required" >&2
    exit 1
fi
if [[ -z "$MESSAGE" ]]; then
    echo "Error: --message is required" >&2
    exit 1
fi

# Send reply
BODY=$(jq -n --arg input "$MESSAGE" '{"input": $input}')

RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/sessions/${SESSION_ID}/input" \
    -H "Content-Type: application/json" \
    -d "$BODY" 2>/dev/null) || {
    echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
    exit 1
}

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY_RESP=$(echo "$RESPONSE" | sed '$d')

if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    echo "Error: Failed to send reply (HTTP ${HTTP_CODE}): ${BODY_RESP}" >&2
    exit 1
fi

# Poll for completion
TIMEOUT=600
POLL_INTERVAL=3
ELAPSED=0

while [[ $ELAPSED -lt $TIMEOUT ]]; do
    sleep "$POLL_INTERVAL"
    ELAPSED=$((ELAPSED + POLL_INTERVAL))

    POLL_RESP=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/sessions/${SESSION_ID}" 2>/dev/null) || continue

    POLL_CODE=$(echo "$POLL_RESP" | tail -1)
    POLL_BODY=$(echo "$POLL_RESP" | sed '$d')

    # Skip non-200 responses (transient errors)
    if [[ "$POLL_CODE" -lt 200 || "$POLL_CODE" -ge 300 ]]; then
        continue
    fi

    STATUS=$(echo "$POLL_BODY" | jq -r '.status // empty' 2>/dev/null) || continue
    OUTPUT=$(echo "$POLL_BODY" | jq -r '.output // ""' 2>/dev/null) || true

    case "$STATUS" in
        completed)
            jq -n --arg sid "$SESSION_ID" --arg output "$OUTPUT" \
                '{"session_id": $sid, "status": "completed", "output": $output}'
            exit 0
            ;;
        failed)
            ERROR=$(echo "$POLL_BODY" | jq -r '.error // ""' 2>/dev/null) || true
            jq -n --arg sid "$SESSION_ID" --arg output "$OUTPUT" --arg error "$ERROR" \
                '{"session_id": $sid, "status": "failed", "output": $output, "error": $error}'
            exit 0
            ;;
        cancelled)
            jq -n --arg sid "$SESSION_ID" --arg output "$OUTPUT" \
                '{"session_id": $sid, "status": "cancelled", "output": $output}'
            exit 0
            ;;
        interrupted)
            jq -n --arg sid "$SESSION_ID" --arg output "$OUTPUT" \
                '{"session_id": $sid, "status": "interrupted", "output": $output}'
            exit 0
            ;;
        needs_input)
            QUESTION=$(echo "$POLL_BODY" | jq -r '.output // ""' 2>/dev/null) || true
            jq -n --arg sid "$SESSION_ID" --arg question "$QUESTION" \
                '{"session_id": $sid, "status": "needs_input", "question": $question}'
            exit 0
            ;;
        *)
            # Still running, keep polling
            ;;
    esac
done

# Timeout
jq -n --arg sid "$SESSION_ID" \
    '{"session_id": $sid, "status": "timeout", "message": "Worker still running after 600s. Use session-recall to check status."}'
exit 0
