# dynamic_operand.py — narrowness corpus: dynamic operands.
#
# Expected: three operations, all with Dynamic certainty and NO nested payload.
# `os.remove(path)` still emits its FilesystemDelete Match — a Dynamic operand
# never lowers risk and never hides the operation (ADR-022 §3) — but the path is
# a variable, not a recovered literal, so certainty is Dynamic. The two
# execution sinks `os.system(cmd)` and `subprocess.run(cmd)` keep their
# CodeExecution Match but record NO nested target: a dynamic payload is never
# enqueued or evaluated (ADR-022 §7). Bounded symbol resolution is a deferred
# slice, so a variable holding a literal is still Dynamic. Parses cleanly.
import os
import subprocess

path = "/tmp/x"
os.remove(path)

cmd = "rm -rf /tmp/x"
os.system(cmd)
subprocess.run(cmd)