// Corpus: execution sinks producing JavaScript payloads (plan Iteration 7,
// ADR-022 §3, §7 cross-language nesting).
//
// `eval` and the `Function` constructor both evaluate JavaScript source, so a
// literal argument is recursively analyzable as JavaScript (the shared
// `family` module tags these as `SourceLanguage::JavaScript`, not TypeScript —
// `eval`/`Function` evaluate JS regardless of the enclosing file's language).
// `eval` takes its payload as the first argument; `new Function(...)` takes the
// function body as the final string argument. Each fires as `CodeExecution`
// with `Known` certainty and a JavaScript nested payload (escapes as-written;
// no decoding this slice).
//
// TypeScript enrichment: the `new Function` call carries an explicit type
// argument (`new Function<string>(...)`). The query's `new_expression` pattern
// still binds `constructor` to the `identifier` because `type_arguments` is a
// separate child, and the final string argument is still the body, so the
// JavaScript nested target is recovered unchanged.

eval("fs.unlinkSync('x')");
new Function<string>("return fs.unlinkSync('y')");