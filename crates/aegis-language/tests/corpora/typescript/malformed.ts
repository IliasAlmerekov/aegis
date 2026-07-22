// Corpus: malformed source (plan Iteration 7, ADR-022 §3).
//
// An unterminated call expression: the argument list is never closed, so the
// grammar produces an ERROR node and the adapter reports a nonzero parse-error
// count. The root mapping turns that into `IncompleteSyntax` degradation. The
// exact count is an implementation detail; the spec-level invariant is "not
// zero".

fs.unlinkSync("x"