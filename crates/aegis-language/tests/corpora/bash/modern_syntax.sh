#!/usr/bin/env bash
items=(one two three)
for item in "${items[@]}"; do
  printf '%s\n' "$item"
done
cat <<'SCRIPT'
rm -rf /tmp/heredoc-body-is-routed-separately
SCRIPT
cat <<SCRIPT
rm -rf /tmp/expanding-heredoc-is-routed-separately-$item
SCRIPT
