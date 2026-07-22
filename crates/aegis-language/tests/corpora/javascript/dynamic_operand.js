// Corpus: dynamic operands (plan Iteration 7, ADR-022 §3, §7).
//
// ADR-022 §3/§7: a dynamic operand never lowers risk and never hides the
// operation, but a dynamic payload is never enqueued or evaluated. Bounded
// symbol resolution is deferred, so a variable holding a literal is still
// `Dynamic` at this seam. Four call sites: a variable path to a delete, a
// variable shell payload, a variable JavaScript payload, and a template
// literal with interpolation (computed, not a known literal). Each surfaces its
// operation with `Dynamic` certainty and no nested payload.

fs.unlinkSync(path);
child_process.exec(cmd);
eval(userInput);
fs.unlinkSync(`${name}`);