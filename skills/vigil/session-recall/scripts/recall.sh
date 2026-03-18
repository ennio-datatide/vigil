#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: recall.sh [--session-id <id>] [--project-path <path>]

Retrieve session information from the Vigil daemon.

Arguments:
  --session-id <id>       Get a specific session by ID
  --project-path <path>   Filter sessions by project path (when listing)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
SESSION_ID=""
PROJECT_PATH=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --session-id)
            SESSION_ID="$2"
            shift 2
            ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

# Build URL
if [[ -n "$SESSION_ID" ]]; then
    URL="${DAEMON_URL}/api/sessions/${SESSION_ID}"
else
    URL="${DAEMON_URL}/api/sessions"
fi

RESPONSE=$(curl -s -w "\n%{http_code}" -X GET "$URL" 2>/dev/null) || {
    echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
    exit 1
}

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY_RESP=$(echo "$RESPONSE" | sed '$d')

if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    echo "Error: API returned HTTP ${HTTP_CODE}: ${BODY_RESP}" >&2
    exit 1
fi

# Client-side filter by project path if listing all sessions
if [[ -z "$SESSION_ID" && -n "$PROJECT_PATH" ]]; then
    echo "$BODY_RESP" | jq --arg pp "$PROJECT_PATH" '[.[] | select(.projectPath == $pp)]' 2>/dev/null || echo "$BODY_RESP"
else
    echo "$BODY_RESP"
fi
