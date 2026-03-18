#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: save.sh --project-path <path> --content <text> --type <type>

Save a new memory for a project.

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --content <text>        The memory content to persist (required)
  --type <type>           Memory type: lesson, fact, preference, decision (required)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
CONTENT=""
MEMORY_TYPE=""

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
        --type)
            MEMORY_TYPE="$2"
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
if [[ -z "$MEMORY_TYPE" ]]; then
    echo "Error: --type is required" >&2
    exit 1
fi

BODY=$(jq -n --arg content "$CONTENT" --arg memoryType "$MEMORY_TYPE" --arg projectPath "$PROJECT_PATH" \
    '{"content": $content, "memoryType": $memoryType, "projectPath": $projectPath}')

RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/memory" \
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
