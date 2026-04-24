#!/usr/bin/env bash
# Generate fake monitoring events to test the TUI monitor
set -euo pipefail

STATE_HOME="${XDG_STATE_HOME:-$HOME/.local/state}"
SESSIONS_DIR="$STATE_HOME/consult-llm/sessions"
mkdir -p "$SESSIONS_DIR"

emit() {
  local file="$1" ts
  shift
  ts=$(date -u +"%Y-%m-%dT%H:%M:%S.000Z")
  local event="$1"
  echo "{\"ts\":\"$ts\",$event}" >> "$file"
}

# Server 1: active with gemini consultation (with progress events)
S1_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
S1_FILE="$SESSIONS_DIR/$S1_ID.jsonl"
emit "$S1_FILE" "\"type\":\"server_started\",\"version\":\"2.5.5+abc1234\",\"pid\":$$"
echo "Server 1: $S1_ID (PID $$)"

sleep 1
emit "$S1_FILE" "\"type\":\"consult_started\",\"id\":\"c_001\",\"model\":\"gemini-3.1-pro-preview\",\"backend\":\"gemini_cli\""
echo "  → Started consultation c_001 (gemini)"

sleep 1
emit "$S1_FILE" "\"type\":\"consult_progress\",\"id\":\"c_001\",\"stage\":{\"type\":\"responding\"}"
echo "  → Progress: Responding..."

sleep 2
emit "$S1_FILE" "\"type\":\"consult_progress\",\"id\":\"c_001\",\"stage\":{\"type\":\"tool_use\",\"tool\":\"read_file\"}"
echo "  → Progress: Tool: read_file"

sleep 2
emit "$S1_FILE" "\"type\":\"consult_progress\",\"id\":\"c_001\",\"stage\":{\"type\":\"tool_result\",\"tool\":\"read_file\",\"success\":true}"
echo "  → Progress: Tool done: read_file"

sleep 1
emit "$S1_FILE" "\"type\":\"consult_progress\",\"id\":\"c_001\",\"stage\":{\"type\":\"tool_use\",\"tool\":\"glob\"}"
echo "  → Progress: Tool: glob"

# Server 2: active with codex consultation (with progress events)
S2_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
S2_FILE="$SESSIONS_DIR/$S2_ID.jsonl"
emit "$S2_FILE" "\"type\":\"server_started\",\"version\":\"2.5.5+abc1234\",\"pid\":$$"
echo "Server 2: $S2_ID (PID $$)"

sleep 1
emit "$S2_FILE" "\"type\":\"consult_started\",\"id\":\"c_002\",\"model\":\"gpt-5.4\",\"backend\":\"codex_cli\""
echo "  → Started consultation c_002 (codex)"

sleep 1
emit "$S2_FILE" "\"type\":\"consult_progress\",\"id\":\"c_002\",\"stage\":{\"type\":\"thinking\"}"
echo "  → Progress: Thinking..."

sleep 2
emit "$S2_FILE" "\"type\":\"consult_progress\",\"id\":\"c_002\",\"stage\":{\"type\":\"tool_use\",\"tool\":\"wc\"}"
echo "  → Progress: Tool: wc"

sleep 1
emit "$S2_FILE" "\"type\":\"consult_progress\",\"id\":\"c_002\",\"stage\":{\"type\":\"tool_result\",\"tool\":\"wc\",\"success\":true}"
echo "  → Progress: Tool done: wc"

sleep 1
emit "$S2_FILE" "\"type\":\"consult_progress\",\"id\":\"c_002\",\"stage\":{\"type\":\"responding\"}"
echo "  → Progress: Responding..."

sleep 2
# Finish gemini
emit "$S1_FILE" "\"type\":\"consult_finished\",\"id\":\"c_001\",\"duration_ms\":10000,\"success\":true"
echo "  → Finished c_001 (gemini)"

sleep 2
# Finish codex
emit "$S2_FILE" "\"type\":\"consult_finished\",\"id\":\"c_002\",\"duration_ms\":12000,\"success\":true"
echo "  → Finished c_002 (codex)"

sleep 2

# Stop servers
emit "$S1_FILE" "\"type\":\"server_stopped\""
emit "$S2_FILE" "\"type\":\"server_stopped\""
echo "Both servers stopped."
echo ""
echo "Files:"
echo "  $S1_FILE"
echo "  $S2_FILE"
