#!/bin/bash
# emit-event.sh — reads hook JSON from stdin, forwards to praefectus server
# Uses a temp file + curl -d @file to avoid shell escaping issues with JSON
INPUT=$(cat)
SESSION_ID="__SESSION_ID__"
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT
printf '{"session_id":"%s","data":%s}' "$SESSION_ID" "$INPUT" > "$TMPFILE"
curl -s -X POST "http://localhost:__SERVER_PORT__/events" \
  -H "Content-Type: application/json" \
  -d @"$TMPFILE" \
  > /dev/null 2>&1 &
