// Corpus: filesystem overwrite (plan Iteration 7, ADR-022 §3).
//
// `writeFileSync` truncates an existing file before writing, so it sets
// `destructive_mode`; `appendFileSync` appends without truncating, so it is an
// overwrite without `destructive_mode`. Both take a literal string path, so
// each is `Known`. Read-only or no-mode forms are out of scope for the JS
// adapter (it tracks only the synchronous `*Sync` spellings this slice).

fs.writeFileSync("f1", "x");
fs.appendFileSync("f2", "y");