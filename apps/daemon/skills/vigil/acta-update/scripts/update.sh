#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: update.sh --project-path <path> --content <text>

Update the project acta (briefing document).

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --content <text>        Updated acta content (required)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
CONTENT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        --content)
            CONTENT="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$PROJECT_PATH" ]]; then
    echo "Error: --project-path is required" >&2
    exit 1
fi
if [[ -z "$CONTENT" ]]; then
    echo "Error: --content is required" >&2
    exit 1
fi

BODY=$(jq -n --arg projectPath "$PROJECT_PATH" --arg content "$CONTENT" \
    '{"projectPath": $projectPath, "content": $content}')

RESPONSE=$(curl -s -w "\n%{http_code}" -X PUT "${DAEMON_URL}/api/vigil/acta" \
    -H "Content-Type: application/json" \
    -d "$BODY" 2>/dev/null) || {
    echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
    exit 1
}

HTTP_CODE=$(echo "$RESPONSE" | tail -1)
BODY_RESP=$(echo "$RESPONSE" | sed '$d')

if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
    echo "Error: API returned HTTP ${HTTP_CODE}: ${BODY_RESP}" >&2
    exit 1
fi

echo "$BODY_RESP"
