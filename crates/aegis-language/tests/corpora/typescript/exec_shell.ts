// Corpus: execution sinks producing Bash (shell) payloads (plan Iteration 7,
// ADR-022 §3, §7 cross-language nesting).
//
// `child_process.exec` and `execSync` run a literal argument through a shell,
// so a string-literal argument is recursively analyzable as Bash. Each fires
// as `CodeExecution` with `Known` certainty and a Bash nested payload carrying
// the literal source (escapes as-written; no decoding this slice).
//
// TypeScript enrichment: the third call carries an explicit type argument
// (`child_process.exec<void>(...)`). The query still binds `function` to the
// `member_expression` and the first positional argument is still the payload,
// so the Bash nested target is recovered unchanged.

child_process.exec("rm -rf /tmp/x");
child_process.execSync("rm /tmp/y");
child_process.exec<void>("rm -rf /tmp/z");