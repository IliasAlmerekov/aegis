// Corpus: permission / ownership changes (plan Iteration 7, ADR-022 §3).
//
// `chmodSync` and `chownSync` are both `PermissionOrOwnershipChange` with the
// default modifiers (no recursive/forced/destructive-mode flag applies). The
// first positional argument is a literal path, so each is `Known`.

fs.chmodSync("f", 0o644);
fs.chownSync("f", 0, 0);