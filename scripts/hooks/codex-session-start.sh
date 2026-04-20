#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex SessionStart hook — injects the aegis command-routing instruction.
# Installed to: ~/.codex/hooks/aegis-session-start.sh

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

cat <<'JSON'
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "context": "IMPORTANT: All Bash tool commands must be routed through aegis. Run every shell command as: aegis --command '<original command>'. Commands that do not start with 'aegis' will be blocked by the PreToolUse hook. This is a security requirement enforced by aegis."
  }
}
JSON
