# fs_delete.py — positive corpus: filesystem deletion operations.
#
# Expected: four FilesystemDelete operations in source order. The first three
# are single-file removes (no modifiers); `shutil.rmtree` carries the recursive
# modifier. Every operand is a string literal → Known certainty. No execution
# sinks, so no nested payloads. Parses cleanly.
import os
import shutil

# Single-file delete, three spellings.
os.remove("/tmp/a")
os.unlink("/tmp/b")
os.rmdir("/tmp/empty")

# Recursive directory-tree delete.
shutil.rmtree("/tmp/tree")