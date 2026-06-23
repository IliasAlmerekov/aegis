use std::path::{Path, PathBuf};

const BEGIN_MARKER: &str = "# >>> aegis shell setup >>>";
const END_MARKER: &str = "# <<< aegis shell setup <<<";

pub(crate) fn run_setup_shell(args: &crate::SetupShellArgs) -> i32 {
    match run_setup_shell_inner(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(err) => {
            eprintln!("error: {err}");
            crate::EXIT_INTERNAL
        }
    }
}

fn run_setup_shell_inner(args: &crate::SetupShellArgs) -> Result<String, String> {
    let home = crate::install::home_dir().ok_or_else(|| "HOME is not set".to_string())?;
    let aegis_bin = match &args.aegis_bin {
        Some(path) => path.clone(),
        None => std::env::current_exe()
            .map_err(|err| format!("failed to resolve current Aegis binary: {err}"))?,
    };

    let real_shell = match &args.shell {
        Some(path) => path.clone(),
        None => detect_real_shell(&aegis_bin)?,
    };

    // Invariant: the real shell must never resolve to the Aegis binary itself,
    // regardless of which source supplied it (--shell, AEGIS_REAL_SHELL, or
    // $SHELL). Writing Aegis as AEGIS_REAL_SHELL would make it exec itself
    // recursively. `same_file` canonicalizes both paths so symlinks — and any
    // lexically-different path to the same file — are caught. This centralizes
    // the guard the $SHELL branch already applies inside detect_real_shell, so
    // AEGIS_REAL_SHELL (which "wins outright") and an explicit --shell cannot
    // bypass it. Applied before --remove too: a self-referential real shell is
    // never a sane state to proceed from.
    if crate::shell_compat::same_file(&real_shell, Some(&aegis_bin)) {
        return Err(
            "the resolved real shell is the Aegis binary itself, which would \
             cause infinite recursion; pass --shell /bin/zsh or set \
             AEGIS_REAL_SHELL to your real shell"
                .to_string(),
        );
    }

    validate_shell_path(&real_shell)?;

    let rc_file = resolve_rc_file(&home, &real_shell, args.rc_file.as_deref())?;

    if args.remove {
        remove_shell_setup_file(&rc_file)?;
        return Ok(format!(
            "Aegis shell proxy removed from {}.\nOpen a new terminal or run:\n  source {}",
            rc_file.display(),
            rc_file.display()
        ));
    }

    // `aegis_bin` is interpolated raw into `export SHELL="..."` in the managed
    // block, so it must pass the same strict path validation as the real shell.
    // Without this, `--aegis-bin '/tmp/aegis"; export EVIL=1; #'` would write
    // command-injection straight into the user's startup file. `--remove` does
    // not write `aegis_bin`, so it is intentionally not validated there.
    validate_shell_path(&aegis_bin)?;

    install_shell_setup_file(&rc_file, &real_shell, &aegis_bin)?;
    Ok(format!(
        "Aegis shell proxy enabled in {}.\nOpen a new terminal or run:\n  source {}\n\nWhat changed:\n  SHELL points to Aegis for tools that launch commands via $SHELL -c.\n  AEGIS_REAL_SHELL keeps your real shell ({}) for command execution.\n\nTo undo this change:\n  aegis setup-shell --remove",
        rc_file.display(),
        rc_file.display(),
        real_shell.display()
    ))
}

// Detect the real shell the user wants Aegis to delegate to. AEGIS_REAL_SHELL
// wins outright; otherwise $SHELL is used unless it already resolves to the
// Aegis binary itself (which would cause recursive wrapping). The comparison
// canonicalizes both paths via `shell_compat::same_file` so a $SHELL that is a
// symlink (or other lexically-different path) to the Aegis binary is still
// caught — a purely lexical compare would let it through and write Aegis as the
// real shell. Fails closed with an actionable message when neither is usable.
fn detect_real_shell(aegis_bin: &Path) -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os("AEGIS_REAL_SHELL")
        && !value.is_empty()
    {
        return Ok(PathBuf::from(value));
    }

    if let Some(value) = std::env::var_os("SHELL")
        && !value.is_empty()
    {
        let shell = PathBuf::from(value);
        if !crate::shell_compat::same_file(&shell, Some(aegis_bin)) {
            return Ok(shell);
        }
    }

    Err(
        "cannot determine the real shell; pass --shell /bin/zsh or set AEGIS_REAL_SHELL"
            .to_string(),
    )
}

fn managed_block(real_shell: &Path, aegis_bin: &Path) -> String {
    format!(
        "{BEGIN_MARKER}\nexport AEGIS_REAL_SHELL=\"{}\"\nexport SHELL=\"{}\"\n{END_MARKER}\n",
        real_shell.display(),
        aegis_bin.display()
    )
}

fn remove_managed_block(input: &str) -> String {
    let mut output = String::new();
    let mut skipping = false;

    for line in input.lines() {
        if line == BEGIN_MARKER {
            skipping = true;
            continue;
        }
        if line == END_MARKER {
            skipping = false;
            continue;
        }
        if !skipping {
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

fn validate_shell_path(path: &Path) -> Result<(), String> {
    let value = path.to_string_lossy();
    if value.is_empty() {
        return Err("real shell path cannot be empty".to_string());
    }
    if value
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '/' | '+' | '-')))
    {
        return Err("real shell path contains unsafe characters".to_string());
    }
    Ok(())
}

fn resolve_rc_file(
    home: &Path,
    real_shell: &Path,
    override_path: Option<&Path>,
) -> Result<PathBuf, String> {
    if let Some(path) = override_path {
        return Ok(path.to_path_buf());
    }

    let shell_name = real_shell
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("cannot determine shell name from {}", real_shell.display()))?;

    match shell_name {
        "bash" => Ok(home.join(".bashrc")),
        "zsh" => Ok(home.join(".zshrc")),
        other => Err(format!(
            "automatic shell setup supports bash and zsh; pass --rc-file for {other}"
        )),
    }
}

fn install_block_in_content(existing: &str, real_shell: &Path, aegis_bin: &Path) -> String {
    let mut cleaned = remove_managed_block(existing);
    if !cleaned.is_empty() && !cleaned.ends_with('\n') {
        cleaned.push('\n');
    }
    cleaned.push_str(&managed_block(real_shell, aegis_bin));
    cleaned
}

fn remove_block_from_content(existing: &str) -> String {
    remove_managed_block(existing)
}

fn read_text_if_exists(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!("failed to read {}: {err}", path.display())),
    }
}

fn write_text_atomically(path: &Path, content: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} does not have a parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;

    let temp_path = parent.join(format!(
        ".{}.aegis-{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("shellrc"),
        std::process::id()
    ));

    std::fs::write(&temp_path, content)
        .map_err(|err| format!("failed to write {}: {err}", temp_path.display()))?;
    std::fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to replace {}: {err}", path.display()))?;
    Ok(())
}

fn install_shell_setup_file(
    rc_file: &Path,
    real_shell: &Path,
    aegis_bin: &Path,
) -> Result<(), String> {
    let existing = read_text_if_exists(rc_file)?;
    let updated = install_block_in_content(&existing, real_shell, aegis_bin);
    write_text_atomically(rc_file, &updated)
}

fn remove_shell_setup_file(rc_file: &Path) -> Result<(), String> {
    let existing = read_text_if_exists(rc_file)?;
    let updated = remove_block_from_content(&existing);
    write_text_atomically(rc_file, &updated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_block_contains_real_shell_and_aegis_bin() {
        let block = managed_block(Path::new("/bin/zsh"), Path::new("/usr/local/bin/aegis"));

        assert_eq!(
            block,
            "# >>> aegis shell setup >>>\nexport AEGIS_REAL_SHELL=\"/bin/zsh\"\nexport SHELL=\"/usr/local/bin/aegis\"\n# <<< aegis shell setup <<<\n"
        );
    }

    #[test]
    fn remove_managed_block_keeps_user_content() {
        let input = "export PATH=\"$HOME/bin:$PATH\"\n# >>> aegis shell setup >>>\nexport AEGIS_REAL_SHELL=\"/bin/zsh\"\nexport SHELL=\"/usr/local/bin/aegis\"\n# <<< aegis shell setup <<<\nalias ll='ls -la'\n";

        let cleaned = remove_managed_block(input);

        assert_eq!(
            cleaned,
            "export PATH=\"$HOME/bin:$PATH\"\nalias ll='ls -la'\n"
        );
    }

    #[test]
    fn install_managed_block_is_idempotent() {
        let original = "export PATH=\"$HOME/bin:$PATH\"\n# >>> aegis shell setup >>>\nexport AEGIS_REAL_SHELL=\"/bin/bash\"\nexport SHELL=\"/old/aegis\"\n# <<< aegis shell setup <<<\n";
        let cleaned = remove_managed_block(original);
        let updated = format!(
            "{}{}",
            cleaned,
            managed_block(Path::new("/bin/zsh"), Path::new("/usr/local/bin/aegis"))
        );

        assert_eq!(updated.matches(BEGIN_MARKER).count(), 1);
        assert!(updated.contains("export AEGIS_REAL_SHELL=\"/bin/zsh\""));
        assert!(updated.contains("export SHELL=\"/usr/local/bin/aegis\""));
        assert!(!updated.contains("/old/aegis"));
    }

    #[test]
    fn validate_shell_path_rejects_newline_injection() {
        let err = validate_shell_path(Path::new("/bin/zsh\nexport EVIL=1")).unwrap_err();

        assert!(err.contains("unsafe characters"));
    }

    // The same validator must guard --aegis-bin, which is interpolated raw into
    // `export SHELL="..."`. Quotes, semicolons, spaces, and `#` would otherwise
    // allow command injection in the generated rc file.
    #[test]
    fn validate_shell_path_rejects_quote_semicolon_injection_payload() {
        let err = validate_shell_path(Path::new("/tmp/aegis\"; export EVIL=1; #")).unwrap_err();

        assert!(err.contains("unsafe characters"));
    }

    #[test]
    fn resolve_rc_file_uses_zshrc_for_zsh() {
        let rc = resolve_rc_file(Path::new("/Users/aiperi"), Path::new("/bin/zsh"), None).unwrap();

        assert_eq!(rc, PathBuf::from("/Users/aiperi/.zshrc"));
    }

    #[test]
    fn resolve_rc_file_uses_bashrc_for_bash() {
        let rc = resolve_rc_file(Path::new("/Users/aiperi"), Path::new("/bin/bash"), None).unwrap();

        assert_eq!(rc, PathBuf::from("/Users/aiperi/.bashrc"));
    }

    #[test]
    fn resolve_rc_file_accepts_explicit_override_for_other_shells() {
        let rc = resolve_rc_file(
            Path::new("/Users/aiperi"),
            Path::new("/usr/local/bin/fish"),
            Some(Path::new("/tmp/aegis-fish-config")),
        )
        .unwrap();

        assert_eq!(rc, PathBuf::from("/tmp/aegis-fish-config"));
    }

    #[test]
    fn resolve_rc_file_rejects_unsupported_shell_without_override() {
        let err = resolve_rc_file(
            Path::new("/Users/aiperi"),
            Path::new("/usr/local/bin/fish"),
            None,
        )
        .unwrap_err();

        assert!(err.contains("supports bash and zsh"));
    }

    #[test]
    fn install_block_appends_after_existing_content() {
        let updated = install_block_in_content(
            "alias gs='git status'\n",
            Path::new("/bin/zsh"),
            Path::new("/usr/local/bin/aegis"),
        );

        assert!(updated.starts_with("alias gs='git status'\n"));
        assert!(updated.contains(BEGIN_MARKER));
        assert!(updated.ends_with("# <<< aegis shell setup <<<\n"));
    }

    #[test]
    fn remove_block_from_content_returns_user_content_only() {
        let existing = install_block_in_content(
            "alias gs='git status'\n",
            Path::new("/bin/zsh"),
            Path::new("/usr/local/bin/aegis"),
        );

        let cleaned = remove_block_from_content(&existing);

        assert_eq!(cleaned, "alias gs='git status'\n");
    }
}
