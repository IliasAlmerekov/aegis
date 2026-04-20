#!/usr/bin/env bash
# aegis-hook-version: 1
# Claude Code PreToolUse hook — rewrites Bash tool commands through aegis.
# Installed to: ~/.claude/hooks/aegis-rewrite.sh
# Requires: jq, aegis

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

command_value=$(printf '%s' "$input_json" | jq -r '.tool_input.command // empty' 2>/dev/null) || {
  echo "[aegis] WARNING: malformed JSON input. Hook cannot rewrite commands." >&2
  exit 0
}

if [ -z "$command_value" ]; then
  exit 0
fi

case "$command_value" in
  aegis\ *)
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
