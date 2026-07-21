; Python call-expression captures for the Aegis language-aware adapter
; (ADR-022 §3). The adapter runs this query to find every call site, then
; inspects the captured function text and arguments in Rust to classify it into
; a language-neutral Detected operation — the query is structural capture only;
; semantic interpretation (which API spelling maps to which OperationKind) is
; typed Rust code in `python.rs`, never a private copy of the shared classifier
; (Iteration 5 REVIEW GATE).
;
; Two patterns: attribute calls (`os.remove(...)`, `shutil.rmtree(...)`) and
; bare-identifier calls (`eval(...)`, `exec(...)`, `open(...)`). Each match
; captures the whole call (`@call`), its `argument_list` (`@args`), and either
; the object/attribute identifiers (`@obj`/`@attr`) or the function name
; (`@fname`). Deeper chains (`os.path.join`) do not match the `object:
; (identifier)` form and are intentionally left to a future resolution slice.

(call
  function: (attribute
    object: (identifier) @obj
    attribute: (identifier) @attr)
  arguments: (argument_list) @args) @call

(call
  function: (identifier) @fname
  arguments: (argument_list) @args) @call