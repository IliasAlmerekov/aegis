// Corpus: filesystem deletion (plan Iteration 7, ADR-022 §3).
//
// Four destructive delete call sites spanning the `fs` deletion surface the
// adapter tracks: `unlinkSync` (file), `rmdirSync` (empty dir), and `rmSync`
// with a `recursive: true` options object in both the identifier- and
// string-keyed literal shapes. Each operand is a pure string literal, so
// every operation is `Known`. `rmSync` resolves its `recursive` modifier from
// the options object; `unlinkSync`/`rmdirSync` are never recursive.
//
// TypeScript enrichment: the first call carries an explicit type argument
// (`fs.unlinkSync<void>(...)`) — TypeScript-only syntax the JS adapter does
// not exercise. The `calls.scm` query still binds the `function` field to the
// `member_expression` because `type_arguments` is a separate child, so the op
// surfaces unchanged (pinned in `languages::typescript` unit tests).

fs.unlinkSync<void>("a.txt");
fs.rmdirSync("d1");
fs.rmSync("d2", {recursive: true});
fs.rmSync("d3", {"recursive": true});