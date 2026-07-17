# Language grammar manifest

> **Status:** L1 foundation — Iteration 0 prototype. The four foundation
> grammars below are statically linked and parse-qualified on the host build,
> and the 4-target release matrix (ADR-022 §8) is exercised as a build/link
> gate by the `cross-matrix` CI job. Runtime behavior beyond parse-only is not
> yet wired.

This document is the human-readable release grammar manifest required by
[ADR-022 §8](adr/adr-022-language-aware-analysis-is-an-additive-isolated-stage.md):
official release binaries statically link every production-qualified grammar
and never download grammars at runtime. The authoritative machine-readable
form is `aegis_language::manifest::BUILTIN_MANIFEST`, validated by
`aegis_language::manifest::validate_manifest`; this prose must stay in sync
with it.

## Qualified grammars (L1 foundation)

| Language    | Crate                    | Version  | Upstream                                               | License | ABI |
|-------------|--------------------------|----------|--------------------------------------------------------|---------|-----|
| Python      | `tree-sitter-python`     | `0.25.0` | <https://github.com/tree-sitter/tree-sitter-python>     | MIT     | 15  |
| JavaScript  | `tree-sitter-javascript` | `0.25.0` | <https://github.com/tree-sitter/tree-sitter-javascript> | MIT     | 15  |
| TypeScript  | `tree-sitter-typescript` | `0.23.2` | <https://github.com/tree-sitter/tree-sitter-typescript> | MIT     | 14  |
| Shell/Bash  | `tree-sitter-bash`       | `0.25.1` | <https://github.com/tree-sitter/tree-sitter-bash>        | MIT     | 15  |

The TypeScript grammar is generated for Tree-sitter ABI 14; the pinned
runtime (`tree-sitter` 0.26, ABI 15) accepts it as backwards-compatible
(`MIN_COMPATIBLE_LANGUAGE_VERSION..=LANGUAGE_VERSION`, i.e. ABI 13–15). The
`builtin_manifest_abi_matches_the_live_tree_sitter_runtime` test guards the
recorded ABI against drift from the live grammar.

## Runtime

| Crate           | Version  | Upstream                                   | License |
|-----------------|----------|--------------------------------------------|---------|
| `tree-sitter`   | `0.26.11`| <https://github.com/tree-sitter/tree-sitter> | MIT     |
| `tree-sitter-language` | `0.1.7` | (crates.io, MIT)                     | MIT     |

Grammar crates depend on `tree-sitter-language` (a C-ABI `LanguageFn`
function-pointer wrapper), not on `tree-sitter` directly, so grammars and the
runtime can be versioned independently while staying ABI-compatible. All five
crates resolve to a single `tree-sitter-language 0.1.7` (no duplicate
versions).

## Build inputs and native source inventory

The plan's Iteration 0 GREEN list requires an inventory of every `build.rs`,
native source file, license, upstream, pin, Rust binding, and transitive
dependency. This is that inventory, as resolved in `Cargo.lock`.

### Build scripts and the C toolchain

Every crate below compiles bundled C at build time via the `cc` build-time
driver (`cc 1.2.64`); there is no runtime code generation and no runtime grammar
download. The build scripts are vendored inside each crate:

| Crate            | Build script                | Native C compiled                                                        |
|------------------|-----------------------------|-------------------------------------------------------------------------|
| `tree-sitter`    | `binding_rust/build.rs` (`links = "tree-sitter"`) | unity build of `src/lib.c` (pulls in `parser.c`, `lexer.c`, `node.c`, `stack.c`, `subtree.c`, `query.c`, `language.c`, `tree.c`, … under `src/`) |
| `tree-sitter-python`     | `bindings/rust/build.rs` | `src/parser.c` + external `src/scanner.c`                       |
| `tree-sitter-javascript` | `bindings/rust/build.rs` | `src/parser.c` + external `src/scanner.c`                       |
| `tree-sitter-typescript` | `bindings/rust/build.rs` | `typescript/src/parser.c` + `typescript/src/scanner.c` (the wired `LANGUAGE_TYPESCRIPT`; the crate also ships an unused `tsx/` dialect) |
| `tree-sitter-bash`       | `bindings/rust/build.rs` | `src/parser.c` + external `src/scanner.c`                       |

Each grammar also vendors the generated headers `src/tree_sitter/{parser,alloc,array}.h`.
The runtime's `wasm_store.c` is **not** compiled: the `wasm` feature is off (see
rejected inputs below).

### Transitive dependency closure

The crates added to the **default-feature** graph by `aegis-language`
(`cargo tree -p aegis-language`), each pinned in `Cargo.lock`:

| Crate                    | Version   | License | Notes                                             |
|--------------------------|-----------|---------|---------------------------------------------------|
| `tree-sitter`            | `0.26.11` | MIT     | runtime; deps: `cc`, `regex`, `regex-syntax`, `serde_json`, `streaming-iterator`, `tree-sitter-language` |
| `tree-sitter-language`   | `0.1.7`   | MIT     | C-ABI `LanguageFn` wrapper; no further deps       |
| `tree-sitter-python`     | `0.25.0`  | MIT     | deps: `cc`, `tree-sitter-language`                |
| `tree-sitter-javascript` | `0.25.0`  | MIT     | deps: `cc`, `tree-sitter-language`                |
| `tree-sitter-typescript` | `0.23.2`  | MIT     | deps: `cc`, `tree-sitter-language`                |
| `tree-sitter-bash`       | `0.25.1`  | MIT     | deps: `cc`, `tree-sitter-language`                |
| `cc`                     | `1.2.64`  | MIT OR Apache-2.0 | build-dependency; the C compiler driver   |
| `streaming-iterator`     | `0.1.9`   | MIT OR Apache-2.0 | new leaf pulled in by the runtime         |

`regex`, `regex-syntax`, and `serde_json` are runtime deps of `tree-sitter` but
were already in the workspace graph before this crate, so they are not new
native/link inputs. All licenses are on the `deny.toml` permissive allow-list;
`cargo deny check` is green (see `docs/performance-baseline.md` REVIEW GATE).

## Rejected grammars and targets (evidence)

Per ADR-022 §8 ("document rejected grammars or targets with evidence"), the
inputs evaluated and **excluded** from the L1 foundation, and why:

| Excluded input                     | Reason |
|------------------------------------|--------|
| Runtime `wasm` feature / `wasm_store.c` | ADR-022 forbids dynamic/runtime grammar loading; static linking only. Not compiled. |
| `LANGUAGE_TSX` (TypeScript crate's `tsx/` dialect) | Only `LANGUAGE_TYPESCRIPT` is wired for L1; TSX is a separate dialect qualified on demand, not part of the four-language foundation. |
| Go, PHP, Ruby, PowerShell, Perl, Lua grammars | Staged 1.x adapters (ADR-022 §9). Each must pass its own qualification gate before it may be linked; none is compiled into L1. |
| GNU-libc Linux, Windows, and other targets | The release set is the four static-friendly targets below. musl (not gnu) is chosen for fully static Linux binaries; Windows is out of scope (M4 drop-Windows). |

## Staged 1.x adapters (NOT qualified for L1)

Go, PHP, Ruby, PowerShell, Perl, and Lua are staged 1.x adapters
(ADR-022 §9). They have no entry above and must not be compiled into the
release binary until each passes its independent qualification gate: pinned
grammar and license evidence, all-target artifact parity, positive/negative/
alias/literal/malformed/new-syntax corpora, worker crash/timeout/protocol
tests, resource-limit and recursive cross-language tests, adapter and protocol
fuzzing, Audit v1/v2 compatibility, Shell/Watch/Hook/CI integration, and
safe-hot-path plus slow-path latency/memory/binary-size budgets. An
unqualified adapter degrades honestly when its source is encountered rather
than receiving a weaker best-effort parser.

## Non-configurable static linking

Per ADR-022 §8, official release binaries never download grammars at runtime
and never load dynamic grammar libraries. The qualified set must be identical
across `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`,
`x86_64-apple-darwin`, and `aarch64-apple-darwin` — verified by the 4-target
cross-compile release matrix (Iteration 0 RED #2b).