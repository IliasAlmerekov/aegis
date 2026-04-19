#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex PreToolUse hook — denies unwrapped Bash commands.
# Installed to: ~/.codex/hooks/aegis-pre-tool-use.sh

set -u

emit_deny() {
  reason=$1
  cat <<EOF
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "deny",
    "permissionDecisionReason": "$reason"
  }
}
EOF
}

is_exact_aegis_wrapper() {
  command=$1
  command -v python3 >/dev/null 2>&1 || return 1

  python3 - "$command" <<'PY' >/dev/null 2>&1
import shlex
import sys

command = sys.argv[1]
prefix = "aegis --command "

if not command.startswith(prefix):
    raise SystemExit(1)

payload = command[len(prefix):]
if not payload or payload[0] != "'" or payload[-1] != "'":
    raise SystemExit(1)

try:
    parts = shlex.split(payload, posix=True)
except ValueError:
    raise SystemExit(1)

if len(parts) != 1:
    raise SystemExit(1)

def shell_quote(value: str) -> str:
    return "'" + value.replace("'", r"'\''") + "'"

if prefix + shell_quote(parts[0]) != command:
    raise SystemExit(1)
PY
}

if ! command -v jq >/dev/null 2>&1; then
  emit_deny "Run through aegis: unable to inspect command because jq is missing"
  exit 0
fi

input_json=$(cat)

if [ -z "$input_json" ]; then
  emit_deny "Run through aegis: unable to inspect command because stdin was empty"
  exit 0
fi

command_value=$(
  printf '%s' "$input_json" | jq -r '
    if (.tool_input.command? | type == "string") and (.tool_input.command | length > 0)
    then .tool_input.command
    else empty
    end
  ' 2>/dev/null
) || {
  emit_deny "Run through aegis: unable to inspect command because input JSON could not be parsed"
  exit 0
}

if [ -z "$command_value" ]; then
  emit_deny "Run through aegis: unable to inspect command because .tool_input.command was missing, null, empty, or unexpected"
  exit 0
fi

if is_exact_aegis_wrapper "$command_value"; then
  exit 0
fi

case $command_value in
  aegis\ *)
  emit_deny "Run through aegis: invalid aegis wrapper syntax"
  exit 0
    ;;
esac

quoted_command=$(printf '%s' "$command_value" | jq -Rrs @sh)

jq -n \
  --arg reason "Run through aegis: aegis --command ${quoted_command}" \
  '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "deny",
      "permissionDecisionReason": $reason
    }
  }'
