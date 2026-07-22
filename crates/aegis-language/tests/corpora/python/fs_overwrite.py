# fs_overwrite.py — positive corpus: overwrite / truncation via open().
#
# Expected: two FilesystemOverwrite operations. `open(..., "w")` truncates an
# existing file → destructive_mode = true; `open(..., "a")` appends → overwrite
# without destructive_mode. `open(..., "r")` (read) and a no-mode `open(...)`
# are not destructive and emit no operation. Operands are string literals →
# Known. Parses cleanly.
# Truncating write (destructive mode).
open("/tmp/w", "w")

# Append (overwrite, not destructive mode).
open("/tmp/a", "a")

# Read-only — not destructive (no operation).
open("/tmp/r", "r")

# No mode argument — not destructive (no operation).
open("/tmp/nomode")