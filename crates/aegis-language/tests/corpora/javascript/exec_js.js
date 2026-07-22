// Corpus: execution sinks producing JavaScript payloads (plan Iteration 7,
// ADR-022 §3, §7 cross-language nesting).
//
// `eval` and the `Function` constructor both evaluate JavaScript source, so a
// literal argument is recursively analyzable as JavaScript. `eval` takes its
// payload as the first argument; `new Function(...)` takes the function body as
// the final string argument. Each fires as `CodeExecution` with `Known`
// certainty and a JavaScript nested payload carrying the literal source
// (escapes as-written; no decoding this slice).

eval("fs.unlinkSync('x')");
new Function("return fs.unlinkSync('y')");