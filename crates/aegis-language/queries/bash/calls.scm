; Bash command- and redirect-captures for the Aegis language-aware adapter
; (ADR-022 §3). The adapter runs this query to find every simple command and
; file redirect, then inspects the captured command name, arguments, and
; redirect operator in Rust to classify each into a language-neutral Detected
; operation — the query is structural capture only; semantic interpretation
; (which command spelling maps to which OperationKind, redirect-operator
; semantics, nested payload recovery) is typed Rust code in `bash.rs`, never a
; private copy of the shared classifier (Iteration 5 REVIEW GATE).
;
; Two patterns:
;   * every simple `command` with its `command_name` (`rm -rf x`, `bash -c …`,
;     `eval …`, `chmod 777 f`, `python3 -c …`, …). `declaration_command`
;     (`declare`/`export`/`local`/`readonly`), `unset_command`, and
;     `test_command` (`[ … ]` / `[[ … ]]`) are distinct node types and are
;     intentionally not matched here — they are not destructive or
;     execution-sink operations in the L1 scope.
;   * every `file_redirect` (`> f`, `>> f`, `echo hi > f`, `cat > f`, …). The
;     operator (`>`, `>>`, `<`, `>&`, …) is an anonymous token child read in
;     Rust; `heredoc_redirect` is input (a heredoc body), not a destructive file
;     write, and is intentionally not matched.
;
; Tree-sitter matches patterns recursively, so commands and redirects nested
; inside command substitution `$(…)`, subshells `(…)`, compound statements
; `{…}`, and `list`/`pipeline` tails are captured as well. Heredoc *bodies* are
; not parsed into commands by the bash grammar (a heredoc body is one
; `heredoc_body` text node); the orchestration layer re-feeds a quoted heredoc
; body as its own Bash target (plan Iteration 4), so the adapter sees those
; commands at the top level of that separate target — they are not missed, just
; not re-parsed inside the parent's heredoc node.

(command
  name: (command_name) @name) @cmd

(file_redirect) @redirect