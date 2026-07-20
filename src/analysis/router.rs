//! Production source-target router (ADR-022 §6, L1 Iteration 4 slices 1-3).
//!
//! Detects analyzable source in an intercepted command, reusing the real
//! `aegis-parser` tokenizer and `Effective program` resolution instead of the
//! ad hoc helpers in the Iteration-0 `aegis_language::router` prototype
//! (`aegis-language` cannot depend on `aegis-parser` to reuse them directly —
//! ADR-022 §4's leaf-crate boundary, pinned by
//! `tests/aegis_language_boundary.rs`). This module lives in the root `aegis`
//! crate, which already depends on both.
//!
//! [`route`] is pure and performs no filesystem access — it only decides
//! *what* to analyze ([`RoutedTarget::Inline`] source it already has in hand,
//! or a [`RoutedTarget::ScriptFile`] path it has not read yet). Turning a
//! `ScriptFile` route into an actual [`aegis_language::SourceTarget`] is
//! [`resolve`]'s job, which defers to [`crate::analysis::source_reader`] for
//! the bounded, catch-only read (ADR-022 §6).
//!
//! Slice 1 (explicit interpreter, versioned-basename normalization,
//! trusted-alias resolution), slice 2 (script-file argv routing, verified
//! shebang, direct-exec-by-shebang), slice 3 (heredoc/here-string/
//! literal-producer stdin, via [`crate::analysis::heredoc`]), slice 4
//! (literal top-level `cd -- <path> &&` tracking), and the deferred
//! same-command heredoc-to-file reuse ([`heredoc_write_then_exec_reuse`],
//! narrowly scoped to `cat > PATH`/`tee PATH <<HEREDOC && <interp> PATH`) are
//! in scope. `aegis-config` budget/trusted-alias wiring lands in a later
//! slice per `docs/plans/2026-07-16-language-aware-analysis.md`.

use std::path::{Path, PathBuf};

use aegis_language::{SourceLanguage, SourceTarget};
use aegis_types::DegradationReason;

use super::heredoc::{self, StdinRoute};
use super::source_reader::{self, SourceReadError};

/// A source-analysis route decided without (for `Inline`) or before (for
/// `ScriptFile`) any filesystem access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutedTarget {
    /// An inline interpreter body (`-c` / `-e`) — the source is already in
    /// hand from the command string.
    Inline {
        /// The language the inline body should be parsed as.
        language: SourceLanguage,
        /// The inline source body.
        source: String,
    },
    /// A script file named in argv, or directly executed with a verified
    /// shebang. Not yet read — [`resolve`] performs the bounded read.
    ScriptFile {
        /// The language the file should be parsed as.
        language: SourceLanguage,
        /// The path as it appeared in the command (not yet canonicalized).
        path: PathBuf,
    },
    /// The interpreter has a source (stdin, a dynamic pipeline, …) that could
    /// not be statically recovered. Never claims safety — always resolves to
    /// typed degradation.
    Dynamic {
        /// The language that would have been analyzed.
        language: SourceLanguage,
        /// Why the source could not be recovered.
        reason: DegradationReason,
    },
    /// A path-like program token (`./script.py`, `/abs/path/script`) executed
    /// directly, with no known interpreter naming it. [`resolve`] reads the
    /// file and only treats it as a target if its first line is a verified
    /// shebang (ADR-022 §6) — no `PATH`/`--version`/content-guessing probes.
    DirectExec {
        /// The path as it appeared in the command.
        path: PathBuf,
    },
}

/// A route that did not resolve into an analyzable [`SourceTarget`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedTarget {
    /// The route that failed to resolve.
    pub language: SourceLanguage,
    /// Why it did not resolve.
    pub reason: DegradationReason,
}

/// A known interpreter invocation shape.
struct Interpreter {
    /// The canonical registry program name (after basename/version normalization).
    program: &'static str,
    /// The inline-source flag, e.g. `-c` (Python/Bash) or `-e` (Node).
    inline_flag: &'static str,
    /// The language the interpreter's source should be parsed as.
    language: SourceLanguage,
}

/// The L1 foundation interpreters that expose inline or file source. Shell
/// `sh` is mapped onto the Bash grammar (the L1 Shell/Bash adapter, ADR-022
/// §9).
const INTERPRETERS: &[Interpreter] = &[
    Interpreter {
        program: "python3",
        inline_flag: "-c",
        language: SourceLanguage::Python,
    },
    Interpreter {
        program: "python",
        inline_flag: "-c",
        language: SourceLanguage::Python,
    },
    Interpreter {
        program: "bash",
        inline_flag: "-c",
        language: SourceLanguage::Bash,
    },
    Interpreter {
        program: "sh",
        inline_flag: "-c",
        language: SourceLanguage::Bash,
    },
    Interpreter {
        program: "node",
        inline_flag: "-e",
        language: SourceLanguage::JavaScript,
    },
];

/// Route analyzable source in `command`.
///
/// `trusted_aliases` maps a trusted global alias (e.g. a wrapper script name)
/// to the canonical registry `program` name it stands in for (e.g. `"py"` →
/// `"python3"`). It is a caller-supplied parameter rather than an
/// `aegis-config` read: config wiring for trusted aliases is a follow-up
/// slice.
#[must_use]
pub fn route(command: &str, trusted_aliases: &[(&str, &str)]) -> Vec<RoutedTarget> {
    let (cwd, rest_command) = strip_cd_prefix(command);
    let targets = route_after_cd(rest_command, trusted_aliases);
    match cwd {
        Some(cwd) => targets
            .into_iter()
            .filter_map(|t| apply_cwd(t, &cwd))
            .collect(),
        None => targets,
    }
}

/// A resolved (or provably unresolved) top-level `cd` cwd change.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CwdRoute {
    /// A literal `cd -- <path> &&` prefix (ADR-022 §6).
    Literal(PathBuf),
    /// Any other `cd`/`pushd` form: dynamic, substituted, or otherwise
    /// unresolved.
    Dynamic,
}

/// Detect and strip a literal top-level `cd -- <path> &&` prefix.
///
/// Only this exact form is tracked; any other leading `cd`/`pushd` invocation
/// (no `--`, a dynamic path, no trailing `&&`) is reported as
/// [`CwdRoute::Dynamic`] with an empty remainder, since the true cwd for
/// whatever follows cannot be established (ADR-022 §6).
fn strip_cd_prefix(command: &str) -> (Option<CwdRoute>, &str) {
    let trimmed = command.trim_start();
    let Some(after_cd) = trimmed.strip_prefix("cd ") else {
        return (None, command);
    };

    if let Some(after_dashdash) = after_cd.trim_start().strip_prefix("-- ")
        && let Some((path, rest)) = after_dashdash.split_once("&&")
        && is_literal_path(path.trim())
    {
        return (
            Some(CwdRoute::Literal(PathBuf::from(path.trim()))),
            rest.trim_start(),
        );
    }

    // Dynamic: still route whatever follows `&&` (if any), so the language
    // can still be identified — only path resolution is degraded.
    let rest = after_cd
        .split_once("&&")
        .map_or("", |(_, rest)| rest.trim_start());
    (Some(CwdRoute::Dynamic), rest)
}

/// A path with no substitution, expansion, or glob syntax.
fn is_literal_path(path: &str) -> bool {
    !path.is_empty() && !path.contains(['$', '`', '*', '~', '?', '[', ']', '{', '}'])
}

/// Rebase a route's relative path onto a resolved `cd`, or degrade/drop it
/// when the `cd` itself was unresolved. Absolute paths are unaffected either
/// way. A relative [`RoutedTarget::DirectExec`] after a `Dynamic` `cd` is
/// dropped outright (`None`) rather than read against a possibly-wrong
/// directory: unlike `ScriptFile`, it carries no language to attach to a
/// [`RoutedTarget::Dynamic`] degradation, and misleading evidence from the
/// wrong file is worse than none.
fn apply_cwd(target: RoutedTarget, cwd: &CwdRoute) -> Option<RoutedTarget> {
    match (target, cwd) {
        (RoutedTarget::ScriptFile { language, path }, CwdRoute::Literal(base))
            if path.is_relative() =>
        {
            Some(RoutedTarget::ScriptFile {
                language,
                path: base.join(path),
            })
        }
        (RoutedTarget::ScriptFile { language, path }, CwdRoute::Dynamic) if path.is_relative() => {
            Some(RoutedTarget::Dynamic {
                language,
                reason: DegradationReason::DynamicSource,
            })
        }
        (RoutedTarget::DirectExec { path }, CwdRoute::Literal(base)) if path.is_relative() => {
            Some(RoutedTarget::DirectExec {
                path: base.join(path),
            })
        }
        (RoutedTarget::DirectExec { path }, CwdRoute::Dynamic) if path.is_relative() => None,
        (other, _) => Some(other),
    }
}

fn route_after_cd(command: &str, trusted_aliases: &[(&str, &str)]) -> Vec<RoutedTarget> {
    if let Some(targets) = heredoc_write_then_exec_reuse(command, trusted_aliases) {
        return targets;
    }

    let owned_tokens = aegis_parser::split_tokens(command);
    if owned_tokens.is_empty() {
        return Vec::new();
    }
    let tokens: Vec<&str> = owned_tokens.iter().map(String::as_str).collect();

    let Some(slice) = aegis_parser::effective_token_slices(&tokens)
        .into_iter()
        .next()
    else {
        return Vec::new();
    };

    let Some(interp) = resolve_interpreter(slice.program, trusted_aliases) else {
        if let Some(targets) = pipeline_route(command, trusted_aliases) {
            return targets;
        }
        // `effective_token_slices` only strips launcher-prefix tokens and
        // basename-normalizes the program token it keeps — the rest of the
        // original tokens (including the directory component a `DirectExec`
        // path needs) are copied through unchanged. That means the effective
        // program's *original* token (with its directory, if any) is always
        // at this fixed offset in `tokens`, whether or not a launcher prefix
        // (`sudo`, `timeout 5`, `env FOO=bar`, …) preceded it.
        let effective_start = tokens.len() - slice.tokens.len();
        return direct_exec_route(tokens[effective_start])
            .into_iter()
            .collect();
    };

    let rest = &slice.tokens[1..];

    // The tokenizer has no heredoc-boundary awareness, so tokens *after* a
    // `<<WORD`/`<<<` marker are the heredoc/here-string *body*, not further
    // command arguments. Both the inline-flag scan and the file-argument scan
    // below must stop at the marker, or a crafted heredoc body could be
    // misread as the interpreter's own flag/argument instead of being
    // classified as stdin.
    let marker_pos = rest.iter().position(|tok| tok.starts_with("<<"));
    let before_marker = marker_pos.map_or(rest, |idx| &rest[..idx]);

    // A single left-to-right walk, mirroring how a real interpreter parses
    // its own argv: it keeps consuming flags (including the inline `-c`/`-e`
    // body, which wins immediately) and shell redirections (which the shell
    // strips before exec — the interpreter never sees them) until it hits
    // the first positional (non-flag, non-redirection) token, which is the
    // script file and ends option parsing right there — any flag-shaped
    // token *after* it belongs to the script's own argv, not the
    // interpreter, and must not be misread as the interpreter's inline flag
    // (ADR-022 §6).
    let mut pos = 0;
    while pos < before_marker.len() {
        let tok = before_marker[pos];
        if let Some(source) = inline_body(tok, interp.inline_flag, before_marker, pos) {
            if source.is_empty() {
                // Flag present but no inline body to analyze — not a source target.
                return Vec::new();
            }
            return vec![RoutedTarget::Inline {
                language: interp.language,
                source,
            }];
        }
        if is_redirection_operator(tok) {
            // A spaced-out redirection (`> file`, `2> file`, `>> file`) has
            // its target in the *next* token, which the interpreter never
            // sees either — skip both, not just the operator, or the target
            // filename would be misread as the script argument.
            pos += 2;
            continue;
        }
        if !tok.starts_with('-') && !tok.contains('<') && !tok.contains('>') {
            return vec![RoutedTarget::ScriptFile {
                language: interp.language,
                path: PathBuf::from(tok),
            }];
        }
        pos += 1;
    }

    // No inline flag and no leading file argument: fall back to heredoc/
    // here-string stdin, if any.
    if let Some(stdin_route) =
        heredoc::heredoc_stdin(command).or_else(|| heredoc::here_string_stdin(rest))
    {
        return vec![stdin_target(interp.language, stdin_route)];
    }

    Vec::new()
}

/// A standalone shell redirection operator token (`>`, `>>`, `<`, `2>`, …) —
/// an optional leading file-descriptor number followed by nothing but `<`/`>`
/// characters. A glued form (`>out.txt`, `2>&1`) is not standalone — it
/// carries its own target in the same token and needs no extra token
/// skipped, so it is deliberately excluded here.
fn is_redirection_operator(tok: &str) -> bool {
    let after_fd = tok.trim_start_matches(|c: char| c.is_ascii_digit());
    // `&>`/`&>>` (bash's combined stdout+stderr redirection) carry one
    // leading `&` before the `<`/`>` run; a glued fd-duplication form like
    // `>&2`/`2>&1` has `&` *after* the `<`/`>` instead and is deliberately
    // left unmatched here — it carries its own target in the same token, so
    // the generic "contains `<`/`>`" fallback already skips just that one
    // token, which is correct.
    let after_amp = after_fd.strip_prefix('&').unwrap_or(after_fd);
    !after_amp.is_empty() && after_amp.chars().all(|c| c == '<' || c == '>')
}

/// A bare path-like program token (`./script.py`, `/abs/path/script`) is a
/// candidate direct-exec target; a plain name (no `/`) would require `PATH`
/// resolution, which routing never performs (ADR-022 §6).
fn direct_exec_route(program_token: &str) -> Option<RoutedTarget> {
    program_token
        .contains('/')
        .then(|| RoutedTarget::DirectExec {
            path: PathBuf::from(program_token),
        })
}

fn stdin_target(language: SourceLanguage, route: StdinRoute) -> RoutedTarget {
    match route {
        StdinRoute::Literal(source) => RoutedTarget::Inline { language, source },
        StdinRoute::Dynamic(reason) => RoutedTarget::Dynamic { language, reason },
    }
}

/// Detect the narrow `<write-cmd> <<HEREDOC && <interp> <path>` shape (the
/// `&&`-chained exec lives on the same physical line as the heredoc redirect
/// — real shell grammar reads the heredoc body starting on the *next* line,
/// terminated by a bare delimiter line, regardless of what follows the
/// redirect on the opening line) and reuse the already-in-hand heredoc body
/// instead of routing a `ScriptFile` that would re-read the identical content
/// from disk.
///
/// Recognized exactly: a write command of `cat > PATH` or `tee PATH` before
/// the heredoc marker, exactly one top-level `&&` after it (checked by
/// rejecting any further separator token in the exec part), and a second
/// segment that is exactly `<interpreter> PATH` (no flags, no other
/// arguments) naming the identical literal path. Any other shape — no `&&`
/// chain, `;`/`||` instead, a mismatched path, or extra exec-segment tokens —
/// is not recognized here and falls through to the existing routing above,
/// per `docs/plans/2026-07-16-language-aware-analysis.md` Iteration 4.
fn heredoc_write_then_exec_reuse(
    command: &str,
    trusted_aliases: &[(&str, &str)],
) -> Option<Vec<RoutedTarget>> {
    let first_line = command.lines().next()?;
    let (before_marker, after_marker) = split_at_heredoc_marker(first_line)?;
    let write_path = heredoc_write_target(before_marker)?;

    let exec_part = after_marker.trim_start().strip_prefix("&&")?.trim_start();
    if exec_part.is_empty() {
        return None;
    }
    let exec_tokens = aegis_parser::split_tokens(exec_part);
    if exec_tokens
        .iter()
        .any(|tok| matches!(tok.as_str(), ";" | "&&" | "||" | "|"))
    {
        // A further top-level separator means this is not the narrow
        // exactly-two-segment shape this reuse is scoped to.
        return None;
    }
    let exec_refs: Vec<&str> = exec_tokens.iter().map(String::as_str).collect();
    let slice = aegis_parser::effective_token_slices(&exec_refs)
        .into_iter()
        .next()?;
    if slice.tokens.len() != 2 || Path::new(slice.tokens[1]) != write_path {
        return None;
    }
    let interp = resolve_interpreter(slice.program, trusted_aliases)?;

    let heredoc_body = aegis_parser::extract_heredoc_bodies(command)
        .into_iter()
        .next()?;
    let route = heredoc::classify(heredoc_body.body, heredoc_body.is_nowdoc);
    Some(vec![stdin_target(interp.language, route)])
}

/// Split `line` at its first heredoc marker (`<<WORD`, `<<'WORD'`, or the
/// `<<-` tab-stripping variants), returning the text before the marker and
/// the text immediately after it (which, per shell grammar, is still part of
/// the same command line — e.g. a `&&`-chained command).
///
/// Mirrors the marker grammar of `aegis-parser`'s private
/// `find_heredoc_marker` exactly (no double-quoted delimiter form; an
/// unquoted delimiter word is bounded by the first non-alphanumeric,
/// non-underscore character, matching real shell word lexing — a
/// metacharacter like `&` terminates it without needing whitespace) so the
/// two do not silently diverge on which markers they recognize.
fn split_at_heredoc_marker(line: &str) -> Option<(&str, &str)> {
    let start = line.find("<<")?;
    let before = &line[..start];
    let after_prefix = line[start + 2..]
        .strip_prefix('-')
        .unwrap_or(&line[start + 2..]);
    let rest = after_prefix.trim_start();
    if let Some(after_quote) = rest.strip_prefix('\'') {
        let end = after_quote.find('\'')?;
        return Some((before, &after_quote[end + 1..]));
    }
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some((before, &rest[end..]))
}

/// Recognize a literal `cat > PATH` or `tee PATH` write target — the text
/// before the heredoc marker on its opening line.
fn heredoc_write_target(before_marker: &str) -> Option<PathBuf> {
    let tokens = aegis_parser::split_tokens(before_marker.trim());
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    match refs.as_slice() {
        ["cat", ">", path] | ["tee", path] => Some(PathBuf::from(*path)),
        _ => None,
    }
}

/// Detect a two-stage pipeline whose last stage is a bare (no flags/file
/// argument) interpreter invocation, e.g. `producer | python3`.
///
/// Only a single, narrowly-proven literal-only producer (`printf '%s'
/// <literal>`) is treated as recoverable; every other producer is Dynamic —
/// its content is honestly unresolved, never evaluated or guessed at
/// (ADR-022 §6). Chains that are not exactly two stages, or whose last stage
/// carries flags/arguments, are out of this slice's scope and yield no route.
fn pipeline_route(command: &str, trusted_aliases: &[(&str, &str)]) -> Option<Vec<RoutedTarget>> {
    if !command.contains('|') {
        return None;
    }
    let chain = aegis_parser::top_level_pipelines(command)
        .into_iter()
        .next()?;
    if chain.segments.len() != 2 {
        return None;
    }

    let last_tokens = aegis_parser::split_tokens(&chain.segments[1].raw);
    let last_refs: Vec<&str> = last_tokens.iter().map(String::as_str).collect();
    let last_slice = aegis_parser::effective_token_slices(&last_refs)
        .into_iter()
        .next()?;
    let interp = resolve_interpreter(last_slice.program, trusted_aliases)?;
    if last_slice.tokens.len() > 1 {
        // The last stage has flags/arguments of its own — out of scope here.
        return None;
    }

    if let Some(literal) = printf_percent_s_literal(&chain.segments[0].raw) {
        return Some(vec![RoutedTarget::Inline {
            language: interp.language,
            source: literal,
        }]);
    }

    Some(vec![RoutedTarget::Dynamic {
        language: interp.language,
        reason: DegradationReason::DynamicSource,
    }])
}

/// Recognize `printf '%s' <literal>` exactly — a narrowly-proven
/// literal-only producer (ADR-022 §6) — and return the literal.
fn printf_percent_s_literal(segment: &str) -> Option<String> {
    let tokens = aegis_parser::split_tokens(segment);
    let refs: Vec<&str> = tokens.iter().map(String::as_str).collect();
    let slice = aegis_parser::effective_token_slices(&refs)
        .into_iter()
        .next()?;
    if slice.program != "printf" {
        return None;
    }
    let rest = &slice.tokens[1..];
    if rest.len() != 2 || rest[0] != "%s" {
        return None;
    }
    Some(rest[1].to_owned())
}

/// Resolve `program` to a registry [`Interpreter`], trying an exact match
/// first, then versioned-basename normalization, then `trusted_aliases`.
fn resolve_interpreter(
    program: &str,
    trusted_aliases: &[(&str, &str)],
) -> Option<&'static Interpreter> {
    if let Some(interp) = INTERPRETERS.iter().find(|i| i.program == program) {
        return Some(interp);
    }

    let base = strip_version_suffix(program);
    if base != program
        && let Some((_, canonical)) = BASENAME_FAMILIES.iter().find(|(family, _)| *family == base)
        && let Some(interp) = INTERPRETERS.iter().find(|i| i.program == *canonical)
    {
        return Some(interp);
    }

    let (_, canonical) = trusted_aliases
        .iter()
        .find(|(alias, _)| *alias == program)?;
    INTERPRETERS.iter().find(|i| i.program == *canonical)
}

/// Versioned-basename family prefixes mapped to their canonical registry
/// program name, e.g. `python3.11` and `python3` both normalize to `python3`.
const BASENAME_FAMILIES: &[(&str, &str)] = &[("python", "python3"), ("node", "node")];

/// Strip a trailing version suffix (digits and dots) from a program basename,
/// e.g. `python3.11` → `python`, `node20` → `node`. Returns `name` unchanged
/// if it has no trailing version suffix.
fn strip_version_suffix(name: &str) -> &str {
    let mut end = name.len();
    for (idx, ch) in name.char_indices().rev() {
        if ch.is_ascii_digit() || ch == '.' {
            end = idx;
        } else {
            break;
        }
    }
    &name[..end]
}

/// Extract the inline body for `flag` from token `tok`, returning `None` if
/// `tok` is not the flag. Handles both the standalone (`-c "code"`) and glued
/// (`-ccode`) forms.
fn inline_body(tok: &str, flag: &str, rest: &[&str], pos: usize) -> Option<String> {
    if tok == flag {
        // Standalone flag: the body is the next token, if any.
        return rest.get(pos + 1).map(|s| (*s).to_owned());
    }
    // Glued form: `-c<code>` (flag immediately followed by its body, no `-`
    // continuation, so a lookalike long flag like `-e-x` is not misread as
    // `-e` with body `-x`).
    let stripped = tok.strip_prefix(flag)?;
    if stripped.is_empty() || stripped.starts_with('-') {
        return None;
    }
    Some(stripped.to_owned())
}

/// Resolve every routed target into either an analyzable [`SourceTarget`] or
/// an [`UnresolvedTarget`] carrying typed degradation, performing the bounded
/// catch-only read for [`RoutedTarget::ScriptFile`] routes.
pub async fn resolve(
    routed: Vec<RoutedTarget>,
    script_file_limit_bytes: u64,
) -> Vec<Result<SourceTarget, UnresolvedTarget>> {
    let mut results = Vec::with_capacity(routed.len());
    for target in routed {
        if let Some(result) = resolve_one(target, script_file_limit_bytes).await {
            results.push(result);
        }
    }
    results
}

async fn resolve_one(
    target: RoutedTarget,
    script_file_limit_bytes: u64,
) -> Option<Result<SourceTarget, UnresolvedTarget>> {
    match target {
        RoutedTarget::Inline { language, source } => Some(Ok(SourceTarget { language, source })),
        RoutedTarget::ScriptFile { language, path } => {
            Some(
                match source_reader::read_script_file(&path, script_file_limit_bytes).await {
                    Ok(read) => Ok(SourceTarget {
                        language,
                        source: read.source,
                    }),
                    Err(err) => Err(UnresolvedTarget {
                        language,
                        reason: degradation_reason(&err),
                    }),
                },
            )
        }
        RoutedTarget::Dynamic { language, reason } => {
            Some(Err(UnresolvedTarget { language, reason }))
        }
        RoutedTarget::DirectExec { path } => {
            let read = source_reader::read_script_file(&path, script_file_limit_bytes)
                .await
                .ok()?;
            let first_line = read.source.lines().next().unwrap_or("");
            let language = verified_shebang_language(first_line)?;
            Some(Ok(SourceTarget {
                language,
                source: read.source,
            }))
        }
    }
}

fn degradation_reason(err: &SourceReadError) -> DegradationReason {
    match err {
        SourceReadError::NotFound
        | SourceReadError::NotRegularFile
        | SourceReadError::PermissionDenied
        | SourceReadError::Io(_) => DegradationReason::UnsafeSource,
        SourceReadError::TooLarge { .. } => DegradationReason::LimitExceeded,
        SourceReadError::InvalidUtf8 => DegradationReason::UnsupportedEncoding,
    }
}

/// Verify a file begins with a shebang naming a registry interpreter
/// (`#!/usr/bin/env python3`, `#!/usr/bin/python3`), without content-guessing
/// beyond that first line (ADR-022 §6: no `PATH`/`--version` probing).
#[must_use]
pub fn verified_shebang_language(first_line: &str) -> Option<SourceLanguage> {
    let rest = first_line.strip_prefix("#!")?;
    let mut words = rest.split_whitespace();
    let mut program = words.next()?;
    if program.ends_with("/env") || program == "env" {
        program = words.next()?;
    }
    let program = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    resolve_interpreter(program, &[]).map(|interp| interp.language)
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
