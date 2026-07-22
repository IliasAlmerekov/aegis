// Corpus: negatives — constructs that must NOT surface as operations (plan
// Iteration 7, ADR-022 §3).
//
// Pins the adapter's narrowness: a destructive API spelling inside a comment
// or string literal, a member reference with no call, an unrelated call, and
// CommonJS/ESM imports (imports are resolution input only and emit no operation
// themselves — bounded symbol resolution is a later slice). The TypeScript-only
// declarations — `import type`, `interface`, `type` alias, `enum`, `as` cast,
// `satisfies`, and a decorator — produce no `call_expression`/`new_expression`
// capture the query tracks, so each must parse cleanly and yield zero ops.

// fs.unlinkSync("x");
"fs.unlinkSync('x')";
const ref = fs.unlinkSync;
console.log("hello");
import fs from "fs";
import type { PathLike } from "fs";

interface Config {
  path: string;
}

type Handler = (p: string) => void;

enum Mode {
  Read,
  Write,
}

const lit = "y" as const;
const cfg = { path: "x" } satisfies Config;

function log<T>(c: T): T {
  return c;
}

@log
class Service {
  enabled = true;
}