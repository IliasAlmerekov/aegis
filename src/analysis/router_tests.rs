use super::*;

#[test]
fn explicit_interpreter_python3_inline_body_is_detected() {
    let targets = route(r#"python3 -c "print(1)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(1)".to_owned(),
        }]
    );
}

#[test]
fn versioned_basename_python3_11_normalizes_to_python() {
    let targets = route(r#"python3.11 -c "print(2)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(2)".to_owned(),
        }]
    );
}

#[test]
fn trusted_alias_resolves_to_its_canonical_interpreter() {
    let targets = route(r#"py -c "print(3)""#, &[("py", "python3")]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(3)".to_owned(),
        }]
    );
}

#[test]
fn untrusted_program_name_yields_no_targets() {
    assert_eq!(
        route(r#"py -c "print(3)""#, &[("other-alias", "python3")]),
        Vec::new()
    );
}

#[test]
fn effective_program_resolves_through_stacked_launcher_prefixes() {
    // The Iteration-0 prototype's fixed `COMMAND_PREFIXES` list handled
    // only a single literal prefix and did not know `timeout <n>` consumes
    // an extra argument, nor that `env VAR=val` consumes assignments. This
    // pins that the `aegis-parser` `Effective program` resolution (which
    // production launcher-prefix detection already handles) now covers
    // both through the shared `effective_token_slices` call.
    let targets = route(r#"sudo timeout 5 python3 -c "print(4)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(4)".to_owned(),
        }]
    );

    let targets = route(r#"env FOO=bar python3 -c "print(5)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(5)".to_owned(),
        }]
    );
}

#[test]
fn exact_registry_match_wins_over_a_conflicting_trusted_alias() {
    // A misconfigured trusted-alias table must not be able to hijack a
    // canonical registry program name (ADR-022 §6: explicit interpreter
    // takes precedence).
    let targets = route(r#"python3 -c "print(6)""#, &[("python3", "bash")]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(6)".to_owned(),
        }]
    );
}

#[test]
fn explicit_interpreter_with_file_argument_is_a_script_file_route() {
    let targets = route("python3 script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("script.py"),
        }]
    );
}

#[test]
fn inline_flag_wins_over_a_file_argument_on_the_same_line() {
    // Malformed/adversarial input: both a flag and a bare word present.
    // Explicit interpreter's inline form takes precedence (ADR-022 §6).
    let targets = route(r#"python3 -c "print(7)" ignored.py"#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(7)".to_owned(),
        }]
    );
}

#[test]
fn a_file_argument_before_the_inline_flag_ends_interpreter_option_parsing() {
    // Real interpreter argv semantics: once the first positional (script)
    // argument is seen, everything after it is the *script's* argv, not the
    // interpreter's own flags. A trailing `-c "..."` after a file argument
    // must not be misread as an inline body that shadows the real,
    // unanalyzed file — that would let an adversarial command hide its real
    // source behind a decoy `-c` string (ADR-022 §6).
    let targets = route(r#"python3 malicious.py -c "print(1)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("malicious.py"),
        }]
    );
}

#[test]
fn a_spaced_redirection_target_is_not_misread_as_the_script_file() {
    // The shell strips `> file`/`2> file`/`>> file`/`< file` from argv
    // before exec — the interpreter never sees the redirect target. A
    // redirection preceding the real script argument must not let its
    // target token win the walk and shadow the file that actually executes.
    for command in [
        "python3 > out.txt malicious.py",
        "python3 >> out.txt malicious.py",
        "python3 2> out.txt malicious.py",
        "python3 < in.txt malicious.py",
    ] {
        assert_eq!(
            route(command, &[]),
            vec![RoutedTarget::ScriptFile {
                language: SourceLanguage::Python,
                path: PathBuf::from("malicious.py"),
            }],
            "command: {command}"
        );
    }
}

#[test]
fn a_self_contained_fd_duplication_redirection_consumes_only_its_own_token() {
    // `2>&1` carries its own target in the same token (no separate token to
    // skip) — it must not cause the walk to over-skip and miss the real
    // script argument that immediately follows.
    for command in ["python3 2>&1 script.py", "python3 >&2 script.py"] {
        assert_eq!(
            route(command, &[]),
            vec![RoutedTarget::ScriptFile {
                language: SourceLanguage::Python,
                path: PathBuf::from("script.py"),
            }],
            "command: {command}"
        );
    }
}

#[test]
fn a_bash_combined_redirection_target_is_not_misread_as_the_script_file() {
    // `&>`/`&>>` (bash's combined stdout+stderr redirection) is a spaced
    // operator with a separate target token, same as `>`/`2>`/`>>` — it must
    // be skipped as a pair, not let its target shadow the real script file.
    for command in [
        "python3 &> out.txt malicious.py",
        "python3 &>> out.txt malicious.py",
    ] {
        assert_eq!(
            route(command, &[]),
            vec![RoutedTarget::ScriptFile {
                language: SourceLanguage::Python,
                path: PathBuf::from("malicious.py"),
            }],
            "command: {command}"
        );
    }
}

#[test]
fn quoted_heredoc_stdin_is_routed_as_inline_source() {
    let command = "bash <<'EOF'\nrm -rf /tmp/x\nEOF";
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Bash,
            source: "rm -rf /tmp/x".to_owned(),
        }]
    );
}

#[test]
fn expanding_heredoc_with_substitution_degrades_dynamically() {
    let command = "bash <<EOF\necho $HOME\nEOF";
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Dynamic {
            language: SourceLanguage::Bash,
            reason: DegradationReason::DynamicSource,
        }]
    );
}

#[test]
fn literal_here_string_is_routed_as_inline_source() {
    let targets = route(r#"python3 <<< "print(1)""#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(1)".to_owned(),
        }]
    );
}

#[test]
fn script_file_argument_before_a_heredoc_marker_wins_over_stdin() {
    // Real shell semantics: `script.py` runs; the heredoc is stdin, which
    // only matters if the script itself reads it. The router must not
    // substitute the heredoc body as a decoy source for the file that
    // actually executes.
    let command = "python3 script.py <<EOF\nprint(1)\nEOF";
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("script.py"),
        }]
    );
}

#[test]
fn script_file_argument_before_a_here_string_marker_wins_over_stdin() {
    let command = r#"python3 script.py <<< "print(1)""#;
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("script.py"),
        }]
    );
}

#[test]
fn heredoc_body_word_that_looks_like_a_glued_inline_flag_is_not_misread_as_one() {
    // The tokenizer has no heredoc-boundary awareness: a heredoc body whose
    // first word happens to start with the interpreter's inline flag
    // (`-c` for python3, glued to the rest of that body word) must not be
    // consumed by the inline-flag scan — it is heredoc *body*, not a further
    // command argument, and must resolve through the (quoted, exact) heredoc
    // stdin route instead.
    let command = "python3 <<'EOF'\n-crm -rf /\nEOF";
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "-crm -rf /".to_owned(),
        }]
    );
}

#[test]
fn here_string_body_word_that_looks_like_a_glued_inline_flag_is_not_misread_as_one() {
    let command = r#"node <<< "-ealert(1)""#;
    let targets = route(command, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::JavaScript,
            source: "-ealert(1)".to_owned(),
        }]
    );
}

#[test]
fn glued_flag_lookalike_is_not_misread_as_inline_body() {
    // `-e-x` must not be misread as `-e` (Node's inline flag) with body
    // `-x` — the glued-form guard in `inline_body` rejects any body that
    // itself starts with `-`. With no inline body, no heredoc/here-string,
    // and no plain file argument (`-e-x` starts with `-`), there is no
    // route.
    assert_eq!(route("node -e-x", &[]), Vec::new());
}

#[test]
fn printf_percent_s_literal_producer_piped_to_interpreter_is_routed() {
    let targets = route(r#"printf '%s' 'print(1)' | python3"#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Inline {
            language: SourceLanguage::Python,
            source: "print(1)".to_owned(),
        }]
    );
}

#[test]
fn other_pipeline_producer_degrades_dynamically() {
    let targets = route(r#"echo "$(whoami)" | python3"#, &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Dynamic {
            language: SourceLanguage::Python,
            reason: DegradationReason::DynamicSource,
        }]
    );
}

#[test]
fn pipeline_into_a_non_interpreter_yields_no_targets() {
    assert_eq!(route("echo hello | grep hello", &[]), Vec::new());
}

#[test]
fn verified_shebang_env_form_resolves_language() {
    assert_eq!(
        verified_shebang_language("#!/usr/bin/env python3"),
        Some(SourceLanguage::Python)
    );
}

#[test]
fn verified_shebang_direct_form_resolves_language() {
    assert_eq!(
        verified_shebang_language("#!/usr/bin/python3"),
        Some(SourceLanguage::Python)
    );
}

#[test]
fn verified_shebang_unknown_interpreter_is_not_verified() {
    assert_eq!(verified_shebang_language("#!/usr/bin/env perl"), None);
}

#[test]
fn missing_shebang_is_not_verified() {
    assert_eq!(verified_shebang_language("print(1)"), None);
}

#[tokio::test]
async fn resolve_reads_a_script_file_route() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.py");
    std::fs::write(&path, "print(1)\n").unwrap();

    let results = resolve(
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: path.clone(),
        }],
        1024,
    )
    .await;

    assert_eq!(
        results,
        vec![Ok(SourceTarget {
            language: SourceLanguage::Python,
            source: "print(1)\n".to_owned(),
        })]
    );
}

#[tokio::test]
async fn resolve_degrades_a_missing_script_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.py");

    let results = resolve(
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path,
        }],
        1024,
    )
    .await;

    assert_eq!(
        results,
        vec![Err(UnresolvedTarget {
            language: SourceLanguage::Python,
            reason: DegradationReason::UnsafeSource,
        })]
    );
}

#[test]
fn literal_cd_dashdash_prefix_rebases_a_relative_script_file() {
    let targets = route("cd -- /tmp/proj && python3 script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("/tmp/proj/script.py"),
        }]
    );
}

#[test]
fn cd_prefix_with_a_glob_metacharacter_degrades_a_relative_script_file() {
    // A shell may glob-expand `pro?ect` to a directory this literal-path
    // check has no way to know — treating it as literal and rebasing onto
    // it verbatim would be a wrong (not just missing) cwd.
    let targets = route("cd -- /tmp/pro?ect && python3 script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Dynamic {
            language: SourceLanguage::Python,
            reason: DegradationReason::DynamicSource,
        }]
    );
}

#[test]
fn literal_cd_prefix_leaves_an_absolute_script_file_unaffected() {
    let targets = route("cd -- /tmp/proj && python3 /abs/script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path: PathBuf::from("/abs/script.py"),
        }]
    );
}

#[test]
fn cd_without_dashdash_degrades_a_relative_script_file() {
    let targets = route("cd /tmp/proj && python3 script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Dynamic {
            language: SourceLanguage::Python,
            reason: DegradationReason::DynamicSource,
        }]
    );
}

#[test]
fn cd_with_command_substitution_degrades_a_relative_script_file() {
    let targets = route("cd -- $(mktemp -d) && python3 script.py", &[]);
    assert_eq!(
        targets,
        vec![RoutedTarget::Dynamic {
            language: SourceLanguage::Python,
            reason: DegradationReason::DynamicSource,
        }]
    );
}

#[test]
fn direct_exec_of_a_relative_path_is_routed_pending_shebang_verification() {
    assert_eq!(
        route("./script.py", &[]),
        vec![RoutedTarget::DirectExec {
            path: PathBuf::from("./script.py"),
        }]
    );
}

#[test]
fn bare_program_name_without_a_path_is_not_a_direct_exec_candidate() {
    // Would require `PATH` resolution, which routing never performs.
    assert_eq!(route("myscript", &[]), Vec::new());
}

#[test]
fn direct_exec_of_a_relative_path_is_routed_through_a_launcher_prefix() {
    // `effective_token_slices` already strips launcher prefixes for the
    // known-interpreter path (see `effective_program_resolves_through_stacked_launcher_prefixes`
    // above); direct-exec routing must see the same effective program, not
    // the raw first token, or a launched script silently yields no route.
    for command in [
        "sudo ./script.py",
        "timeout 5 ./script.py",
        "env FOO=bar ./script.py",
    ] {
        assert_eq!(
            route(command, &[]),
            vec![RoutedTarget::DirectExec {
                path: PathBuf::from("./script.py"),
            }],
            "command: {command}"
        );
    }
}

#[tokio::test]
async fn resolve_verifies_shebang_before_treating_direct_exec_as_a_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("script.py");
    std::fs::write(&path, "#!/usr/bin/env python3\nprint(1)\n").unwrap();

    let results = resolve(vec![RoutedTarget::DirectExec { path: path.clone() }], 1024).await;

    assert_eq!(
        results,
        vec![Ok(SourceTarget {
            language: SourceLanguage::Python,
            source: "#!/usr/bin/env python3\nprint(1)\n".to_owned(),
        })]
    );
}

#[tokio::test]
async fn resolve_drops_a_direct_exec_target_with_no_verified_shebang() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("binary");
    std::fs::write(&path, b"\x7fELF-not-really-but-no-shebang").unwrap();

    let results = resolve(vec![RoutedTarget::DirectExec { path }], 1024).await;

    assert_eq!(results, Vec::new());
}

#[tokio::test]
async fn resolve_degrades_an_oversized_script_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.py");
    std::fs::write(&path, "x".repeat(2048)).unwrap();

    let results = resolve(
        vec![RoutedTarget::ScriptFile {
            language: SourceLanguage::Python,
            path,
        }],
        1024,
    )
    .await;

    assert_eq!(
        results,
        vec![Err(UnresolvedTarget {
            language: SourceLanguage::Python,
            reason: DegradationReason::LimitExceeded,
        })]
    );
}
