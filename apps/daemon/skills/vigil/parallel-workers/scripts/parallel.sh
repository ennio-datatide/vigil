#!/bin/bash
set -euo pipefail

DAEMON_URL="${VIGIL_DAEMON_URL:-http://localhost:8000}"

show_help() {
    cat <<'HELP'
Usage: parallel.sh --project-path <path> --task <text> [--task <text> ...]

Spawn multiple Claude Code worker sessions in parallel and wait for all to complete.

Arguments:
  --project-path <path>   Absolute path to the project directory (required)
  --task <text>            Task prompt for a worker (required, repeatable)
  -h, --help              Show this help message

Environment:
  VIGIL_DAEMON_URL        Daemon URL (default: http://localhost:8000)
HELP
    exit 0
}

# Parse arguments
PROJECT_PATH=""
TASKS=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        -h|--help) show_help ;;
        --project-path)
            PROJECT_PATH="$2"
            shift 2
            ;;
        --task)
            TASKS+=("$2")
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
if [[ ${#TASKS[@]} -lt 1 ]]; then
    echo "Error: at least one --task is required" >&2
    exit 1
fi

# Spawn all workers concurrently
SESSION_IDS=()
PIDS=()
TMPDIR_BASE=$(mktemp -d)
trap 'rm -rf "$TMPDIR_BASE"' EXIT

for i in "${!TASKS[@]}"; do
    TASK="${TASKS[$i]}"
    OUTFILE="${TMPDIR_BASE}/spawn_${i}.json"

    (
        BODY=$(jq -n \
            --arg projectPath "$PROJECT_PATH" \
            --arg prompt "$TASK" \
            '{projectPath: $projectPath, prompt: $prompt}')

        RESPONSE=$(curl -s -w "\n%{http_code}" -X POST "${DAEMON_URL}/api/sessions" \
            -H "Content-Type: application/json" \
            -d "$BODY" 2>/dev/null) || {
            echo '{"error":"connection_failed"}' > "$OUTFILE"
            exit 0
        }

        HTTP_CODE=$(echo "$RESPONSE" | tail -1)
        BODY_RESP=$(echo "$RESPONSE" | sed '$d')

        if [[ "$HTTP_CODE" -lt 200 || "$HTTP_CODE" -ge 300 ]]; then
            echo "{\"error\":\"http_${HTTP_CODE}\",\"body\":$(echo "$BODY_RESP" | jq -Rs '.')}" > "$OUTFILE"
            exit 0
        fi

        SESSION_ID=$(echo "$BODY_RESP" | jq -r '.id // empty' 2>/dev/null) || true
        if [[ -z "$SESSION_ID" ]]; then
            echo '{"error":"no_session_id"}' > "$OUTFILE"
            exit 0
        fi

        echo "{\"session_id\":\"${SESSION_ID}\"}" > "$OUTFILE"
    ) &
    PIDS+=($!)
done

# Wait for all spawn requests to finish
for pid in "${PIDS[@]}"; do
    wait "$pid" || true
done

# Collect session IDs, check for spawn errors
for i in "${!TASKS[@]}"; do
    OUTFILE="${TMPDIR_BASE}/spawn_${i}.json"
    if [[ ! -f "$OUTFILE" ]]; then
        echo "Error: spawn for task ${i} produced no output" >&2
        exit 1
    fi

    ERROR=$(jq -r '.error // empty' "$OUTFILE" 2>/dev/null) || true
    if [[ -n "$ERROR" ]]; then
        echo "Error: failed to spawn task ${i}: $(cat "$OUTFILE")" >&2
        exit 1
    fi

    SID=$(jq -r '.session_id' "$OUTFILE" 2>/dev/null) || {
        echo "Error: failed to parse session ID for task ${i}" >&2
        exit 1
    }
    SESSION_IDS+=("$SID")
done

# Poll all sessions until all reach terminal state
TIMEOUT=600
POLL_INTERVAL=3
ELAPSED=0

# Track which sessions are done
declare -A DONE_MAP
for sid in "${SESSION_IDS[@]}"; do
    DONE_MAP["$sid"]="false"
done

while [[ $ELAPSED -lt $TIMEOUT ]]; do
    ALL_DONE=true

    for sid in "${SESSION_IDS[@]}"; do
        if [[ "${DONE_MAP[$sid]}" == "true" ]]; then
            continue
        fi

        POLL_RESP=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/sessions/${sid}" 2>/dev/null) || {
            ALL_DONE=false
            continue
        }

        POLL_CODE=$(echo "$POLL_RESP" | tail -1)
        POLL_BODY=$(echo "$POLL_RESP" | sed '$d')

        if [[ "$POLL_CODE" -lt 200 || "$POLL_CODE" -ge 300 ]]; then
            ALL_DONE=false
            continue
        fi

        STATUS=$(echo "$POLL_BODY" | jq -r '.status // empty' 2>/dev/null) || {
            ALL_DONE=false
            continue
        }

        case "$STATUS" in
            needs_input)
                # Return immediately -- Vigil needs to ask the user
                QUESTION=$(echo "$POLL_BODY" | jq -r '.output // ""' 2>/dev/null) || true
                RESULTS="["
                for j in "${!SESSION_IDS[@]}"; do
                    s="${SESSION_IDS[$j]}"
                    if [[ "$s" == "$sid" ]]; then
                        RESULTS+=$(jq -n --arg sid "$s" --arg question "$QUESTION" \
                            '{"session_id": $sid, "status": "needs_input", "question": $question}')
                    else
                        RESULTS+=$(jq -n --arg sid "$s" --arg status "running" \
                            '{"session_id": $sid, "status": "running"}')
                    fi
                    if [[ $j -lt $((${#SESSION_IDS[@]} - 1)) ]]; then
                        RESULTS+=","
                    fi
                done
                RESULTS+="]"
                jq -n --argjson results "$RESULTS" '{"results": $results}'
                exit 0
                ;;
            completed|failed|cancelled|interrupted)
                DONE_MAP["$sid"]="true"
                ;;
            *)
                ALL_DONE=false
                ;;
        esac
    done

    if [[ "$ALL_DONE" == "true" ]]; then
        break
    fi

    sleep "$POLL_INTERVAL"
    ELAPSED=$((ELAPSED + POLL_INTERVAL))
done

# Build final results
RESULTS="["
for i in "${!SESSION_IDS[@]}"; do
    sid="${SESSION_IDS[$i]}"

    POLL_RESP=$(curl -s -w "\n%{http_code}" -X GET "${DAEMON_URL}/api/sessions/${sid}" 2>/dev/null) || true
    POLL_CODE=$(echo "$POLL_RESP" | tail -1)
    POLL_BODY=$(echo "$POLL_RESP" | sed '$d')

    STATUS=$(echo "$POLL_BODY" | jq -r '.status // "unknown"' 2>/dev/null) || true
    OUTPUT=$(echo "$POLL_BODY" | jq -r '.output // ""' 2>/dev/null) || true
    ERROR=$(echo "$POLL_BODY" | jq -r '.error // ""' 2>/dev/null) || true

    if [[ "$STATUS" == "failed" && -n "$ERROR" ]]; then
        RESULT=$(jq -n --arg sid "$sid" --arg status "$STATUS" --arg output "$OUTPUT" --arg error "$ERROR" \
            '{"session_id": $sid, "status": $status, "output": $output, "error": $error}')
    elif [[ "$ELAPSED" -ge "$TIMEOUT" && "${DONE_MAP[$sid]}" != "true" ]]; then
        RESULT=$(jq -n --arg sid "$sid" \
            '{"session_id": $sid, "status": "timeout", "message": "Worker still running after 600s. Use session-recall to check status."}')
    else
        RESULT=$(jq -n --arg sid "$sid" --arg status "$STATUS" --arg output "$OUTPUT" \
            '{"session_id": $sid, "status": $status, "output": $output}')
    fi

    RESULTS+="$RESULT"
    if [[ $i -lt $((${#SESSION_IDS[@]} - 1)) ]]; then
        RESULTS+=","
    fi
done
RESULTS+="]"

jq -n --argjson results "$RESULTS" '{"results": $results}'
exit 0
