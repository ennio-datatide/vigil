#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: evaluate.sh --project-path <path> --session-id <id> --criteria <text>

Evaluate a worker's output and optionally spawn a refinement worker.

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --session-id <id>       Session ID of the completed worker to evaluate (required)
  --criteria <text>       What to evaluate against (required)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
SESSION_ID=""
CRITERIA=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        --session-id)
            SESSION_ID="$2"
            shift 2
            ;;
        --criteria)
            CRITERIA="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

# Validate required arguments
if [[ -z "$PROJECT_PATH" ]]; then
    echo "Error: --project-path is required" >&2
    exit 1
fi
if [[ -z "$SESSION_ID" ]]; then
    echo "Error: --session-id is required" >&2
    exit 1
fi
if [[ -z "$CRITERIA" ]]; then
    echo "Error: --criteria is required" >&2
    exit 1
fi

# Helper: poll a session until terminal state
poll_session() {
    local sid="$1"
    local timeout=600
    local interval=3
    local elapsed=0

    while [[ $elapsed -lt $timeout ]]; do
        sleep "$interval"
        elapsed=$((elapsed + interval))

        local resp
        resp=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/sessions/${sid}" 2>/dev/null) || continue

        local code
        code=$(echo "$resp" | tail -1)
        local body
        body=$(echo "$resp" | sed '$d')

        if [[ "$code" -lt 200 || "$code" -ge 300 ]]; then
            continue
        fi

        local status
        status=$(echo "$body" | jq -r '.status // empty' 2>/dev/null) || continue

        case "$status" in
            completed|failed|cancelled|interrupted|needs_input)
                echo "$body"
                return 0
                ;;
            *)
                ;;
        esac
    done

    echo '{"status":"timeout"}'
    return 0
}

# Step 1: Get the original worker's output
ORIG_RESP=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/sessions/${SESSION_ID}" 2>/dev/null) || {
    echo "Error: Failed to connect to daemon at ${DAEMON_URL}" >&2
    exit 1
}

ORIG_CODE=$(echo "$ORIG_RESP" | tail -1)
ORIG_BODY=$(echo "$ORIG_RESP" | sed '$d')

if [[ "$ORIG_CODE" -lt 200 || "$ORIG_CODE" -ge 300 ]]; then
    echo "Error: API returned HTTP ${ORIG_CODE} for session ${SESSION_ID}" >&2
    exit 1
fi

ORIG_OUTPUT=$(echo "$ORIG_BODY" | jq -r '.output // ""' 2>/dev/null) || {
    echo "Error: Failed to parse output from session ${SESSION_ID}" >&2
    exit 1
}

if [[ -z "$ORIG_OUTPUT" ]]; then
    echo "Error: Session ${SESSION_ID} has no output" >&2
    exit 1
fi

# Step 2: Spawn evaluator worker
EVAL_PROMPT="Evaluate the following output against these criteria: ${CRITERIA}

OUTPUT TO EVALUATE:
${ORIG_OUTPUT}

You MUST respond with ONLY a JSON object in this exact format, no other text:
{\"quality\": <number 1-10>, \"issues\": [\"issue1\", \"issue2\"], \"suggestions\": [\"suggestion1\", \"suggestion2\"]}

Be strict but fair. A score of 7+ means acceptable quality."

EVAL_BODY=$(jq -n \
    --arg projectPath "$PROJECT_PATH" \
    --arg prompt "$EVAL_PROMPT" \
    '{projectPath: $projectPath, prompt: $prompt}')

EVAL_RESP=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/sessions" \
    -H "Content-Type: application/json" \
    -d "$EVAL_BODY" 2>/dev/null) || {
    echo "Error: Failed to spawn evaluator worker" >&2
    exit 1
}

EVAL_HTTP=$(echo "$EVAL_RESP" | tail -1)
EVAL_SPAWN=$(echo "$EVAL_RESP" | sed '$d')

if [[ "$EVAL_HTTP" -lt 200 || "$EVAL_HTTP" -ge 300 ]]; then
    echo "Error: Failed to spawn evaluator: HTTP ${EVAL_HTTP}" >&2
    exit 1
fi

EVAL_SID=$(echo "$EVAL_SPAWN" | jq -r '.id // empty' 2>/dev/null) || {
    echo "Error: Failed to parse evaluator session ID" >&2
    exit 1
}

if [[ -z "$EVAL_SID" ]]; then
    echo "Error: No session ID for evaluator" >&2
    exit 1
fi

# Step 3: Poll evaluator until complete
EVAL_RESULT=$(poll_session "$EVAL_SID")
EVAL_STATUS=$(echo "$EVAL_RESULT" | jq -r '.status // "unknown"' 2>/dev/null) || true

# Handle non-completed evaluator states
if [[ "$EVAL_STATUS" == "needs_input" ]]; then
    QUESTION=$(echo "$EVAL_RESULT" | jq -r '.output // ""' 2>/dev/null) || true
    jq -n --arg sid "$EVAL_SID" --arg question "$QUESTION" \
        '{"session_id": $sid, "status": "needs_input", "question": $question}'
    exit 0
fi

if [[ "$EVAL_STATUS" != "completed" ]]; then
    EVAL_OUTPUT=$(echo "$EVAL_RESULT" | jq -r '.output // ""' 2>/dev/null) || true
    jq -n --arg sid "$SESSION_ID" --arg evalSid "$EVAL_SID" --arg status "$EVAL_STATUS" --arg output "$ORIG_OUTPUT" --arg evalOutput "$EVAL_OUTPUT" \
        '{"session_id": $sid, "status": "eval_" + $status, "output": $output, "evaluator_session_id": $evalSid, "evaluator_output": $evalOutput}'
    exit 0
fi

# Step 4: Parse evaluation output
EVAL_OUTPUT=$(echo "$EVAL_RESULT" | jq -r '.output // ""' 2>/dev/null) || true

# Try to extract JSON from evaluator output (it may be wrapped in text)
EVAL_JSON=$(echo "$EVAL_OUTPUT" | jq -r '.' 2>/dev/null) || true
if [[ -z "$EVAL_JSON" ]] || ! echo "$EVAL_OUTPUT" | jq -e '.quality' >/dev/null 2>&1; then
    # Try to find JSON embedded in the output
    EVAL_JSON=$(echo "$EVAL_OUTPUT" | grep -oP '\{[^{}]*"quality"[^{}]*\}' 2>/dev/null | head -1) || true
fi

QUALITY=$(echo "$EVAL_JSON" | jq -r '.quality // 0' 2>/dev/null) || QUALITY=0
ISSUES=$(echo "$EVAL_JSON" | jq -c '.issues // []' 2>/dev/null) || ISSUES="[]"
SUGGESTIONS=$(echo "$EVAL_JSON" | jq -c '.suggestions // []' 2>/dev/null) || SUGGESTIONS="[]"

# If we couldn't parse quality, default to passing (avoid unnecessary refinement)
if [[ "$QUALITY" == "0" || "$QUALITY" == "null" ]]; then
    jq -n \
        --arg sid "$SESSION_ID" \
        --arg output "$ORIG_OUTPUT" \
        --arg evalOutput "$EVAL_OUTPUT" \
        '{"session_id": $sid, "status": "completed", "output": $output, "evaluation": {"quality": "unknown", "raw": $evalOutput}, "refined": false}'
    exit 0
fi

# Step 5: If quality >= 7, return original output with evaluation
if [[ "$QUALITY" -ge 7 ]]; then
    jq -n \
        --arg sid "$SESSION_ID" \
        --arg output "$ORIG_OUTPUT" \
        --argjson quality "$QUALITY" \
        --argjson issues "$ISSUES" \
        --argjson suggestions "$SUGGESTIONS" \
        '{"session_id": $sid, "status": "completed", "output": $output, "evaluation": {"quality": $quality, "issues": $issues, "suggestions": $suggestions}, "refined": false}'
    exit 0
fi

# Step 6: Quality < 7 -- spawn refinement worker
REFINE_PROMPT="Improve the following output based on evaluator feedback.

ORIGINAL OUTPUT:
${ORIG_OUTPUT}

EVALUATOR FEEDBACK:
Quality score: ${QUALITY}/10
Issues: $(echo "$ISSUES" | jq -r '.[]' 2>/dev/null | sed 's/^/- /')
Suggestions: $(echo "$SUGGESTIONS" | jq -r '.[]' 2>/dev/null | sed 's/^/- /')

Please produce an improved version that addresses all the issues and incorporates the suggestions. Output ONLY the improved version, no commentary."

REFINE_BODY=$(jq -n \
    --arg projectPath "$PROJECT_PATH" \
    --arg prompt "$REFINE_PROMPT" \
    '{projectPath: $projectPath, prompt: $prompt}')

REFINE_RESP=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/sessions" \
    -H "Content-Type: application/json" \
    -d "$REFINE_BODY" 2>/dev/null) || {
    echo "Error: Failed to spawn refinement worker" >&2
    exit 1
}

REFINE_HTTP=$(echo "$REFINE_RESP" | tail -1)
REFINE_SPAWN=$(echo "$REFINE_RESP" | sed '$d')

if [[ "$REFINE_HTTP" -lt 200 || "$REFINE_HTTP" -ge 300 ]]; then
    echo "Error: Failed to spawn refinement worker: HTTP ${REFINE_HTTP}" >&2
    exit 1
fi

REFINE_SID=$(echo "$REFINE_SPAWN" | jq -r '.id // empty' 2>/dev/null) || {
    echo "Error: Failed to parse refinement session ID" >&2
    exit 1
}

if [[ -z "$REFINE_SID" ]]; then
    echo "Error: No session ID for refinement worker" >&2
    exit 1
fi

# Step 7: Poll refinement worker
REFINE_RESULT=$(poll_session "$REFINE_SID")
REFINE_STATUS=$(echo "$REFINE_RESULT" | jq -r '.status // "unknown"' 2>/dev/null) || true
REFINE_OUTPUT=$(echo "$REFINE_RESULT" | jq -r '.output // ""' 2>/dev/null) || true

if [[ "$REFINE_STATUS" == "needs_input" ]]; then
    QUESTION=$(echo "$REFINE_RESULT" | jq -r '.output // ""' 2>/dev/null) || true
    jq -n --arg sid "$REFINE_SID" --arg question "$QUESTION" \
        '{"session_id": $sid, "status": "needs_input", "question": $question}'
    exit 0
fi

# Return refined result
jq -n \
    --arg sid "$REFINE_SID" \
    --arg origSid "$SESSION_ID" \
    --arg status "$REFINE_STATUS" \
    --arg output "$REFINE_OUTPUT" \
    --argjson quality "$QUALITY" \
    --argjson issues "$ISSUES" \
    --argjson suggestions "$SUGGESTIONS" \
    '{"session_id": $sid, "original_session_id": $origSid, "status": $status, "output": $output, "evaluation": {"quality": $quality, "issues": $issues, "suggestions": $suggestions}, "refined": true}'
exit 0
