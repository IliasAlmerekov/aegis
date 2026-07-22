# exec_python.py — positive corpus: eval / exec execution sinks.
#
# Expected: two CodeExecution operations. Each first positional argument is a
# string literal of Python source → Known certainty, and the literal is Python
# source → a nested recursive target in Python (ADR-022 §7). Parses cleanly.
# Python payload (nested target parsed as Python).
eval("__import__('os').remove('x')")
exec("shutil.rmtree('/tmp/x')")