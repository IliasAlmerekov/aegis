// Corpus: dynamic operands (plan Iteration 7, ADR-022 §3, §7).
//
// ADR-022 §3/§7: a dynamic operand never lowers risk and never hides the
// operation, but a dynamic payload is never enqueued or evaluated. Bounded
// symbol resolution is deferred, so a variable holding a literal is still
// `Dynamic` at this seam. A template literal with interpolation is computed,
// so it is `Dynamic` too.
//
// TypeScript enrichment: the final delete takes an `as`-cast operand
// (`pathArg as string`). An `as` expression is not a string literal, so the
// operand is `Dynamic` and no nested target is recovered — the same narrowness
// a plain variable operand exhibits, exercised on TypeScript-only syntax.

const p = getPath();
fs.unlinkSync(p);
child_process.exec(cmd);
child_process.exec(`rm -rf ${dir}`);
fs.unlinkSync(pathArg as string);