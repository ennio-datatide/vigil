#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: recall.sh --project-path <path> --query <text> [--limit <n>]

Search project memories by semantic similarity.

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --query <text>          Natural-language search query (required)
  --limit <n>             Maximum number of results
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
QUERY=""
LIMIT=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        --query)
            QUERY="$2"
            shift 2
            ;;
        --limit)
            LIMIT="$2"
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
if [[ -z "$QUERY" ]]; then
    echo "Error: --query is required" >&2
    exit 1
fi

# Build JSON body
if [[ -n "$LIMIT" ]]; then
    BODY=$(jq -n --arg query "$QUERY" --arg projectPath "$PROJECT_PATH" --argjson limit "$LIMIT" \
        '{"query": $query, "projectPath": $projectPath, "limit": $limit}')
else
    BODY=$(jq -n --arg query "$QUERY" --arg projectPath "$PROJECT_PATH" \
        '{"query": $query, "projectPath": $projectPath}')
fi

RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/memory/search" \
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
