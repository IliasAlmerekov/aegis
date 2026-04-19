#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex SessionStart hook — injects the aegis command-routing instruction.
# Installed to: ~/.codex/hooks/aegis-session-start.sh

set -u

cat <<'JSON'
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "context": "IMPORTANT: All Bash tool commands must be routed through aegis. Run every shell command as: aegis --command '<original command>'. Commands that do not start with 'aegis' will be blocked by the PreToolUse hook. This is a security requirement enforced by aegis."
  }
}
JSON
