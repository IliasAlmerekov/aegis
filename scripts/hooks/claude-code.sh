#!/usr/bin/env bash
# aegis-hook-version: 1
# Claude Code PreToolUse hook — rewrites Bash tool commands through aegis.
# Installed to: ~/.claude/hooks/aegis-rewrite.sh
# Requires: jq, aegis

set -u

if ! command -v jq >/dev/null 2>&1; then
  echo "[aegis] WARNING: jq not installed. Hook cannot rewrite commands." >&2
  exit 0
fi

if ! command -v aegis >/dev/null 2>&1; then
  echo "[aegis] WARNING: aegis not in PATH. Hook disabled." >&2
  exit 0
fi

input_json=$(cat)

if [ -z "$input_json" ]; then
  exit 0
fi

command_value=$(printf '%s' "$input_json" | jq -r '.tool_input.command // empty')

if [ -z "$command_value" ]; then
  exit 0
fi

case "$command_value" in
  aegis*)
    exit 0
    ;;
esac

quoted_command=$(printf '%s' "$command_value" | jq -Rrs @sh)
rewritten_command="aegis --command ${quoted_command}"

original_input=$(printf '%s' "$input_json" | jq -c '.tool_input')
updated_input=$(printf '%s' "$original_input" | jq --arg command "$rewritten_command" '.command = $command')

jq -n \
  --argjson updatedInput "$updated_input" \
  '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "allow",
      "permissionDecisionReason": "Aegis intercept",
      "updatedInput": $updatedInput
    }
  }'
