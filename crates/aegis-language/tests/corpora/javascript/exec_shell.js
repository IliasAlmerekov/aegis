// Corpus: execution sinks producing shell payloads (plan Iteration 7, ADR-022
// §3, §7 cross-language nesting).
//
// `child_process.exec` and `child_process.execSync` take a string that the
// shell interprets, so a literal argument is recursively analyzable as Bash.
// Each call site fires as `CodeExecution` with `Known` certainty and a Bash
// nested payload carrying the literal shell source (escapes as-written; no
// decoding this slice). A `child_process.spawn`/`execFile`/`fork` argv form is
// covered by `dynamic_operand.js` and the adapter unit tests, not here.

child_process.exec("rm -rf /tmp/x");
child_process.execSync("rm /tmp/y");
child_process.exec("rm -rf /tmp/z");