; TypeScript call- and new-expression captures for the Aegis language-aware
; adapter (ADR-022 §3). TypeScript is a syntactic superset of JavaScript, so
; this query is structurally identical to `queries/javascript/calls.scm`: the
; tree-sitter-typescript grammar reuses the JavaScript node types
; (`call_expression`, `member_expression`, `new_expression`, `arguments`,
; `identifier`, `property_identifier`). Type-only syntax (type annotations,
; generics, interfaces, `as`/`satisfies`, `import type`, decorators) produces
; no `call_expression`/`new_expression` capture here, so it surfaces no
; operation. The adapter runs this query to find every call site and `new`-
; expression, then inspects the captured function text and arguments in Rust
; via the shared `family` module — the query is structural capture only;
; semantic interpretation is typed Rust, never a private copy of the shared
; classifier (Iteration 5 REVIEW GATE).
;
; A call with explicit type arguments (`fs.unlinkSync<void>("x")`) still
; matches: `type_arguments` is a separate child of `call_expression`, not the
; `function` field, so the `function: (member_expression …)` pattern binds
; unchanged. Deeper member chains (`a.b.c()`) do not match the
; `object: (identifier)` form and are intentionally left to a future resolution
; slice.

(call_expression
  function: (member_expression
    object: (identifier) @obj
    property: (property_identifier) @attr)
  arguments: (arguments) @args) @call

(call_expression
  function: (identifier) @fname
  arguments: (arguments) @args) @call

(new_expression
  constructor: (identifier) @fname
  arguments: (arguments) @args) @call