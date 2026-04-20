#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex PreToolUse hook — denies unwrapped Bash commands.
# Installed to: ~/.codex/hooks/aegis-pre-tool-use.sh

set -u

aegis_truthy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_falsy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    0|false|no) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_ci_active() {
  if [ -n "${AEGIS_CI:-}" ]; then
    aegis_falsy "${AEGIS_CI}" && return 1
    aegis_truthy "${AEGIS_CI}" && return 0
  fi

  for key in CI GITHUB_ACTIONS GITLAB_CI CIRCLECI BUILDKITE TRAVIS TF_BUILD; do
    value="$(printenv "$key" 2>/dev/null || true)"
    if [ -n "${value}" ] && aegis_truthy "${value}"; then
      return 0
    fi
  done

  [ -n "${JENKINS_URL:-}" ]
}

aegis_disabled_locally() {
  [ -f "${HOME}/.aegis/disabled" ]
}

aegis_enforcement_enabled() {
  if aegis_ci_active; then
    return 0
  fi

  if aegis_disabled_locally; then
    return 1
  fi

  return 0
}

AEGIS_TOGGLE_HELPER="${HOME}/.aegis/lib/toggle-state.sh"
if [ -r "${AEGIS_TOGGLE_HELPER}" ]; then
  . "${AEGIS_TOGGLE_HELPER}"
fi

if ! aegis_enforcement_enabled; then
  exit 0
fi

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
