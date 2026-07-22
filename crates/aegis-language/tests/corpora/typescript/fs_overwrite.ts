// Corpus: filesystem overwrite (plan Iteration 7, ADR-022 §3).
//
// `writeFileSync` truncates, so it carries `destructive_mode`; `appendFileSync`
// does not. Each operand is a string literal, so both are `Known`. Mirrors the
// JavaScript corpus's overwrite file; the destructive-mode distinction is the
// spec-level invariant pinned here.

fs.writeFileSync("a", "b");
fs.appendFileSync("a", "b");