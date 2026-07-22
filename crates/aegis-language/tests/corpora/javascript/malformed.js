// Corpus: malformed source (plan Iteration 7, ADR-022 §4).
//
// An unterminated call expression must record a nonzero parse-error count. The
// root mapping turns `parse_errors` into `IncompleteSyntax` degradation; this
// corpus file only proves the adapter reports the malformed parse.

fs.unlinkSync(