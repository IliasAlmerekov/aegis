# perms.py — positive corpus: permission and ownership changes.
#
# Expected: three PermissionOrOwnershipChange operations (os.chmod, os.chown,
# shutil.chown), no modifiers, Known certainty (first positional arg is a string
# literal path). Parses cleanly.
import os
import shutil

os.chmod("/tmp/x", 0o777)
os.chown("/tmp/x", 1000, 1000)
shutil.chown("/tmp/x", "user")