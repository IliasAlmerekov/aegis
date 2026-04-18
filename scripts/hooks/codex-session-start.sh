#!/usr/bin/env bash
# aegis-hook-version: 2
# Codex SessionStart hook — injects the aegis command-routing instruction.
# Installed to: ~/.codex/hooks/aegis-session-start.sh

set -u

cat <<'JSON'
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "context": "Route every Bash command through aegis --command '<original command>'. Unwrapped Bash commands will be blocked by the fail-closed PreToolUse hook."
  }
}
JSON
