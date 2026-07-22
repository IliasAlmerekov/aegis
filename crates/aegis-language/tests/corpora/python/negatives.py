# negatives.py — negative / narrowness corpus: references without calls, and
# comments or strings that merely mention a tracked API.
#
# Expected: zero operations. A comment or string literal that mentions
# `os.remove` is not a call; an attribute reference without a call (`os.remove`
# bound to a name) is not an operation; unrelated calls (print, len) are not
# tracked destructive or execution-sink APIs. Parses cleanly.
import os

# A comment mentioning the API is not a call.
# os.remove("/tmp/x")

# A string mentioning the API is not a call.
doc = "call os.remove('/tmp/x') to delete"

# An attribute reference without a call is not an operation.
handler = os.remove

# Unrelated calls are not tracked operations.
print(doc)
len(doc)