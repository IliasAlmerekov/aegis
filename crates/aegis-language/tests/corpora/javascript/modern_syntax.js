// Corpus: modern syntax (plan Iteration 7, ADR-022 §3).
//
// Proves the pinned tree-sitter-javascript 0.25.0 grammar parses current
// JavaScript cleanly and that the adapter surfaces no false operations over
// modern syntax constructs. Covers optional chaining, nullish coalescing,
// logical assignment, class fields / private fields / private methods /
// getters, async/await, arrow functions, template literals with and without
// interpolation, destructuring with defaults/rest, spread in call and array,
// exponentiation, numeric separators, BigInt, and optional catch binding.
//
// Every call site here is on a NON-tracked identifier (`fetch`, `fn`, `go`,
// `console`) or an optional-chaining member access, so none matches a tracked
// destructive/execution-sink API spelling — the corpus must yield zero ops.

const x = data?.items?.[0]?.name;
const y = data ?? "default";
config ||= {};
config.timeout ??= 5;

class Counter {
  #priv = 0;
  static count = 0;
  get value() {
    return this.#priv;
  }
  #bump() {
    this.#priv += 1;
    return this.#priv;
  }
}

async function go() {
  const r = await fetch(url);
  return r;
}
const inc = (n) => n + 1;

const msg = `hello ${name}`;
const lit = `plain`;

const [a, ...rest] = arr;
const {p, q = 2} = obj;
fn(...args);
const big = 1_000_000n;
const exp = 2 ** 10;

try {
  f();
} catch {}