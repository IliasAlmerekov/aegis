# malformed.py — malformed-source corpus: a call that does not close its
# argument list.
#
# Expected: parse_errors > 0 (the tree has at least one ERROR node). The
# recoverable prefix before the error may still yield the `os.remove` call site,
# but the corpus only asserts that malformed source is reported as such — the
# root mapping turns a nonzero parse-error count into
# `DegradationReason::IncompleteSyntax`. The exact ERROR-node count is a
# grammar implementation detail and is not pinned here.
import os

# Unterminated call: missing closing parenthesis.
os.remove("/tmp/x"