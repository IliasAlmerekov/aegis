// Corpus: negatives — constructs that must NOT surface as operations (plan
// Iteration 7, ADR-022 §3).
//
// Pins the adapter's narrowness: a destructive API spelling inside a comment
// or string literal, a member reference with no call, an unrelated call, and a
// CommonJS/ESM import (imports are resolution input only and emit no operation
// themselves — bounded symbol resolution is a later slice). Each must parse
// cleanly and yield zero operations.

// fs.unlinkSync("x");
"fs.unlinkSync('x')";
const ref = fs.unlinkSync;
console.log("hello");
import fs from "fs";