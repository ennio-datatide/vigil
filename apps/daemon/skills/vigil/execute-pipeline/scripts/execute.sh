#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: execute.sh --project-path <path> --prompt <text> [--pipeline-id <id>]

Execute a multi-step development pipeline.

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --prompt <text>         Instructions for the pipeline (required)
  --pipeline-id <id>      Pipeline ID (default: uses the default pipeline)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
PROMPT=""
PIPELINE_ID=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        --prompt)
            PROMPT="$2"
            shift 2
            ;;
        --pipeline-id)
            PIPELINE_ID="$2"
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
if [[ -z "$PROMPT" ]]; then
    echo "Error: --prompt is required" >&2
    exit 1
fi

# Resolve pipeline ID if not provided
if [[ -z "$PIPELINE_ID" ]]; then
    PIPELINES_RESP=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/pipelines" 2>/dev/null) || {
        echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
        exit 1
    }

    PIPE_CODE=$(echo "$PIPELINES_RESP" | tail -1)
    PIPE_BODY=$(echo "$PIPELINES_RESP" | sed '$d')

    if [[ "$PIPE_CODE" -lt 200 || "$PIPE_CODE" -ge 300 ]]; then
        echo "Error: Failed to list pipelines (HTTP ${PIPE_CODE}): ${PIPE_BODY}" >&2
        exit 1
    fi

    PIPELINE_ID=$(echo "$PIPE_BODY" | jq -r '[.[] | select(.isDefault == true)] | .[0].id // empty' 2>/dev/null) || true

    if [[ -z "$PIPELINE_ID" ]]; then
        echo "Error: No default pipeline found" >&2
        exit 1
    fi
fi

# Execute pipeline
BODY=$(jq -n --arg projectPath "$PROJECT_PATH" --arg prompt "$PROMPT" \
    '{"projectPath": $projectPath, "prompt": $prompt}')

RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/pipelines/${PIPELINE_ID}/execute" \
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

# Extract execution ID and return structured output
EXEC_ID=$(echo "$BODY_RESP" | jq -r '.id // empty' 2>/dev/null) || true

if [[ -n "$EXEC_ID" ]]; then
    jq -n --arg eid "$EXEC_ID" '{"execution_id": $eid, "status": "started"}'
else
    echo "$BODY_RESP"
fi
