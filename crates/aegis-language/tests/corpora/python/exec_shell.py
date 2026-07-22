# exec_shell.py — positive corpus: process / shell execution sinks.
#
# Expected: six CodeExecution operations. Each first positional argument is a
# string literal → Known certainty, and the literal is a shell command → a
# nested recursive target in Bash (ADR-022 §7 cross-language nesting). Covers
# os.system and the subprocess.{run,call,Popen,check_call,check_output} family.
# Parses cleanly.
import os
import subprocess

os.system("rm -rf /tmp/x")
subprocess.run("rm /tmp/y")
subprocess.call("rm /tmp/z")
subprocess.Popen("rm /tmp/w")
subprocess.check_call("rm /tmp/v")
subprocess.check_output("rm /tmp/u")