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
  prefix='aegis --command '

  case $command in
    "$prefix"*) ;;
    *) return 1 ;;
  esac

  payload=${command#"$prefix"}

  [ -n "$payload" ] || return 1

  first_char=${payload%"${payload#?}"}
  [ "$first_char" = "'" ] || return 1

  len=${#payload}
  i=1

  while [ "$i" -lt "$len" ]; do
    c=${payload:$i:1}

    if [ "$c" != "'" ]; then
      i=$((i + 1))
      continue
    fi

    if [ $((i + 1)) -eq "$len" ]; then
      return 0
    fi

    if [ "${payload:$((i + 1)):1}" != "\\" ]; then
      return 1
    fi

    if [ "${payload:$((i + 2)):1}" != "'" ]; then
      return 1
    fi

    if [ "${payload:$((i + 3)):1}" != "'" ]; then
      return 1
    fi

    i=$((i + 4))
  done

  return 1
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

quoted_command=$(printf '%s' "$command_value" | jq -Rrsr @sh)

jq -n \
  --arg reason "Run through aegis: aegis --command ${quoted_command}" \
  '{
    "hookSpecificOutput": {
      "hookEventName": "PreToolUse",
      "permissionDecision": "deny",
      "permissionDecisionReason": $reason
    }
  }'
