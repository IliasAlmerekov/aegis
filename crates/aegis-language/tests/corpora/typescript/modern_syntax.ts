// Corpus: modern TypeScript syntax (plan Iteration 7, ADR-022 §3).
//
// Proves the pinned tree-sitter-typescript 0.23.2 grammar parses current
// TypeScript cleanly and that the adapter surfaces no false operations over
// modern syntax constructs. Covers generics, arrow generics (the `<T,>`
// disambiguation), optional chaining, nullish coalescing, `as const`/`as`
// casts, `satisfies`, decorators, `import type`, interfaces, type aliases,
// enums, mapped types, conditional types, and `infer`.
//
// Every call site here is on a NON-tracked identifier (`push`, `first`, `id`,
// `console`) or a `this.`-qualified member chain (`this.items.push` — the
// query matches `object: (identifier)` only, so a `member_expression` object
// does not capture), so none matches a tracked destructive/execution-sink API
// spelling — the corpus must yield zero ops.

interface Box<T> {
  value: T;
}

type Pair<T, U> = [T, U];

type Mapped<T> = {
  [K in keyof T]: T[K];
};

type Unwrap<T> = T extends Promise<infer U> ? U : T;

function first<T>(arr: T[]): T | undefined {
  return arr[0];
}

const id = <T,>(x: T): T => x;

const data: Box<number> = {value: 1};
const name = obj?.name ?? "anon";
const lit = "x" as const;
const cfg = {n: 1} satisfies Record<string, number>;

enum Color {
  Red,
  Green,
  Blue,
}

function log<T>(c: T): T {
  return c;
}

@log
class Service<T> {
  private items: T[] = [];

  add(i: T): number {
    this.items.push(i);
    return this.items.length;
  }
}

import type { PathLike } from "fs";

console.log(first<number>([1]));