#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex SessionStart hook — injects the aegis command-routing instruction.
# Installed to: ~/.codex/hooks/aegis-session-start.sh

set -u

AEGIS_TOGGLE_HELPER="${HOME}/.aegis/lib/toggle-state.sh"
[ -r "${AEGIS_TOGGLE_HELPER}" ] || exit 0
. "${AEGIS_TOGGLE_HELPER}"

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
