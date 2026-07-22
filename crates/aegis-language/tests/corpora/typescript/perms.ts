// Corpus: permission / ownership changes (plan Iteration 7, ADR-022 §3).
//
// `chmodSync` and `chownSync` both fire as `PermissionOrOwnershipChange` with
// default modifiers and `Known` string-literal operands. Mirrors the JavaScript
// corpus's perms file.

fs.chmodSync("x", 0o777);
fs.chownSync("y", 1000, 1000);