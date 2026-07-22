; JavaScript call- and new-expression captures for the Aegis language-aware
; adapter (ADR-022 §3). The adapter runs this query to find every call site and
; `new`-expression, then inspects the captured function text and arguments in
; Rust to classify it into a language-neutral Detected operation — the query is
; structural capture only; semantic interpretation (which API spelling maps to
; which OperationKind) is typed Rust code in `javascript.rs`, never a private
; copy of the shared classifier (Iteration 5 REVIEW GATE).
;
; Three patterns: member calls (`fs.unlinkSync(...)`, `child_process.exec(...)`),
; bare-identifier calls (`eval(...)`), and `new`-expressions with an identifier
; constructor (`new Function(...)`). Each match captures the whole call
; (`@call`), its `arguments` (`@args`), and either the object/property identifiers
; (`@obj`/`@attr`) or the function/constructor name (`@fname`). Deeper member
; chains (`a.b.c()`) do not match the `object: (identifier)` form and are
; intentionally left to a future resolution slice.

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