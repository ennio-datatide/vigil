#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: delete.sh --memory-id <id>

Delete a specific memory by its ID.

Arguments:
  --memory-id <id>        ID of the memory to delete (required)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
MEMORY_ID=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --memory-id)
            MEMORY_ID="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$MEMORY_ID" ]]; then
    echo "Error: --memory-id is required" >&2
    exit 1
fi

RESPONSE=$(curl -s -w "\n%{http_code}" -X DELETE "${DAEMON_URL}/api/memory/${MEMORY_ID}" 2>/dev/null) || {
    echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
    exit 1
}

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY_RESP=$(echo "$RESPONSE" | sed '$d')

if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    echo "Error: API returned HTTP ${HTTP_CODE}: ${BODY_RESP}" >&2
    exit 1
fi

if [[ -z "$BODY_RESP" ]]; then
    jq -n --arg id "$MEMORY_ID" '{"deleted": $id}'
else
    echo "$BODY_RESP"
fi
