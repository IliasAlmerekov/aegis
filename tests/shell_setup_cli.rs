use std::fs;
use std::path::Path;
use std::process::Command;

fn aegis_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_aegis"))
}

fn run_setup_shell(args: &[&str], home: &Path, envs: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(aegis_bin());
    command
        .arg("setup-shell")
        .args(args)
        .env("HOME", home)
        .env_remove("AEGIS_REAL_SHELL")
        .env_remove("SHELL");

    for (key, value) in envs {
        command.env(key, value);
    }

    command.output().expect("run aegis setup-shell")
}

#[test]
fn setup_shell_writes_zsh_managed_block() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--shell",
            "/bin/zsh",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            "/usr/local/bin/aegis",
        ],
        temp.path(),
        &[],
    );

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rc = fs::read_to_string(&rc_file).expect("read rc file");
    assert!(rc.contains("# >>> aegis shell setup >>>"));
    assert!(rc.contains("export AEGIS_REAL_SHELL='/bin/zsh'"));
    assert!(rc.contains("export SHELL='/usr/local/bin/aegis'"));
}

#[test]
fn setup_shell_is_idempotent() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");

    for _ in 0..2 {
        let output = run_setup_shell(
            &[
                "--shell",
                "/bin/zsh",
                "--rc-file",
                rc_file.to_str().expect("utf8 rc path"),
                "--aegis-bin",
                "/usr/local/bin/aegis",
            ],
            temp.path(),
            &[],
        );
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let rc = fs::read_to_string(&rc_file).expect("read rc file");
    assert_eq!(rc.matches("# >>> aegis shell setup >>>").count(), 1);
}

#[test]
fn setup_shell_remove_deletes_only_managed_block() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");
    fs::write(&rc_file, "alias ll='ls -la'\n").expect("seed rc");

    let install = run_setup_shell(
        &[
            "--shell",
            "/bin/zsh",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            "/usr/local/bin/aegis",
        ],
        temp.path(),
        &[],
    );
    assert!(install.status.success());

    let remove = run_setup_shell(
        &[
            "--remove",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
        ],
        temp.path(),
        &[("SHELL", "/bin/zsh")],
    );
    assert!(
        remove.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&remove.stderr)
    );

    let rc = fs::read_to_string(&rc_file).expect("read rc file");
    assert_eq!(rc, "alias ll='ls -la'\n");
}

#[test]
fn setup_shell_rejects_unsupported_shell_without_rc_override() {
    let temp = tempfile::TempDir::new().expect("temp dir");

    let output = run_setup_shell(&["--shell", "/usr/local/bin/fish"], temp.path(), &[]);

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("supports bash and zsh"));
}

// If $SHELL already points at the Aegis binary itself, setup must NOT use it as
// the real shell — that would make Aegis exec itself recursively. With no
// explicit --shell and no AEGIS_REAL_SHELL, detect_real_shell must fail closed
// instead of producing a self-referential managed block. This guards a
// security-critical recursion path that has no other dedicated test.
#[test]
fn setup_shell_fails_closed_when_shell_points_at_aegis_binary() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();

    let output = run_setup_shell(
        &[
            "--rc-file",
            temp.path().join(".zshrc").to_str().expect("utf8"),
        ],
        temp.path(),
        &[("SHELL", aegis.to_str().expect("utf8 aegis path"))],
    );

    assert!(
        !output.status.success(),
        "setup must not recurse when $SHELL is the Aegis binary"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot determine the real shell"));
}

// The recursion guard must canonicalize paths: a $SHELL that is a symlink to
// the Aegis binary is lexically different but resolves to the same file. A
// purely lexical compare would let it through and write Aegis as the real
// shell — infinite recursion. `same_file` (which canonicalizes) must catch it.
#[cfg(unix)]
#[test]
fn setup_shell_fails_closed_when_shell_is_symlink_to_aegis_binary() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();
    // A symlink with a lexically different path that resolves to the Aegis
    // binary — the exact case a lexical-only guard misses.
    let symlink_path = temp.path().join("aegis-link");
    symlink(&aegis, &symlink_path).expect("create symlink to aegis binary");

    let output = run_setup_shell(
        &[
            "--rc-file",
            temp.path().join(".zshrc").to_str().expect("utf8"),
            "--aegis-bin",
            aegis.to_str().expect("utf8 aegis path"),
        ],
        temp.path(),
        &[("SHELL", symlink_path.to_str().expect("utf8 symlink path"))],
    );

    assert!(
        !output.status.success(),
        "setup must not recurse when $SHELL is a symlink to the Aegis binary"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("cannot determine the real shell"),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// `--aegis-bin` is interpolated into `export SHELL='...'`. A payload with
// quotes/semicolons must be made inert by POSIX single-quote escaping rather
// than rejected — the value is written verbatim inside single quotes, so the
// embedded `"; export EVIL=1; #` cannot start a new statement. This is the
// robust replacement for the old reject-on-unsafe-chars approach, which also
// blocked legitimate scoped npm paths.
#[test]
fn setup_shell_escapes_injection_payload_in_aegis_bin() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--shell",
            "/bin/zsh",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            "/tmp/aegis\"; export EVIL=1; #",
        ],
        temp.path(),
        &[("SHELL", "/bin/zsh")],
    );

    assert!(
        output.status.success(),
        "setup must safely escape injection payloads in --aegis-bin: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rc = fs::read_to_string(&rc_file).expect("read rc file");
    assert!(
        rc.contains("export SHELL='/tmp/aegis\"; export EVIL=1; #'"),
        "payload must be single-quoted verbatim, got: {rc}"
    );
    // The injected assignment must not appear as its own statement.
    assert!(
        !rc.contains("\nexport EVIL=1"),
        "injected statement must not escape the single-quoted value: {rc}"
    );
}

// The reported root cause: a scoped npm install path contains `@`, which the
// old strict validator rejected with `real shell path contains unsafe
// characters`. The scoped binary path must now be accepted and written
// verbatim inside single quotes.
#[test]
fn setup_shell_accepts_scoped_npm_aegis_binary_path() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");
    let scoped = temp
        .path()
        .join("node_modules/@iliasalmerekov/aegis/vendor/aegis");

    let output = run_setup_shell(
        &[
            "--shell",
            "/bin/zsh",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            scoped.to_str().expect("utf8 scoped path"),
        ],
        temp.path(),
        &[],
    );

    assert!(
        output.status.success(),
        "scoped npm path must be accepted: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rc = fs::read_to_string(&rc_file).expect("read rc file");
    assert!(
        rc.contains(&format!("export SHELL='{}'", scoped.display())),
        "scoped npm path must round-trip inside single quotes, got: {rc}"
    );
}

// When the aegis binary path is the invalid one, the error must name the aegis
// binary path, not the real shell path — the old shared validator always said
// "real shell path", which misdirected debugging of npm installs.
#[test]
fn setup_shell_reports_aegis_binary_path_when_aegis_bin_is_invalid() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--shell",
            "/bin/zsh",
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            "/tmp/aegis\u{7f}bad",
        ],
        temp.path(),
        &[],
    );

    assert!(
        !output.status.success(),
        "control characters in --aegis-bin must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("aegis binary path"),
        "error must name the aegis binary path, got: {stderr}"
    );
    assert!(
        !rc_file.exists(),
        "rc file must not be written when --aegis-bin validation fails"
    );
}

// The recursion invariant — the final real shell must never resolve to the
// Aegis binary — must hold for EVERY source of the real shell, not only $SHELL.
// AEGIS_REAL_SHELL "wins outright", so a value pointing at the Aegis binary
// (directly or via symlink) must be rejected before any rc file is written.
#[test]
fn setup_shell_rejects_aegis_real_shell_pointing_at_aegis_binary() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            aegis.to_str().expect("utf8 aegis path"),
        ],
        temp.path(),
        &[("AEGIS_REAL_SHELL", aegis.to_str().expect("utf8 aegis path"))],
    );

    assert!(
        !output.status.success(),
        "setup must not write Aegis as the real shell via AEGIS_REAL_SHELL"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("infinite recursion"),
        "expected recursion error, got: {stderr}"
    );
    assert!(
        !rc_file.exists(),
        "rc file must not be written when AEGIS_REAL_SHELL resolves to Aegis"
    );
}

#[cfg(unix)]
#[test]
fn setup_shell_rejects_aegis_real_shell_symlink_to_aegis_binary() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();
    let symlink_path = temp.path().join("real-shell-link");
    symlink(&aegis, &symlink_path).expect("create symlink to aegis binary");
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            aegis.to_str().expect("utf8 aegis path"),
        ],
        temp.path(),
        &[(
            "AEGIS_REAL_SHELL",
            symlink_path.to_str().expect("utf8 symlink path"),
        )],
    );

    assert!(
        !output.status.success(),
        "setup must not recurse when AEGIS_REAL_SHELL is a symlink to Aegis"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("infinite recursion"),
        "expected recursion error"
    );
    assert!(
        !rc_file.exists(),
        "rc file must not be written when AEGIS_REAL_SHELL is a symlink to Aegis"
    );
}

// The explicit `--shell` flag is taken directly with no recursion check. It
// must be subject to the same invariant as every other real-shell source.
#[test]
fn setup_shell_rejects_explicit_shell_pointing_at_aegis_binary() {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--shell",
            aegis.to_str().expect("utf8 aegis path"),
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            aegis.to_str().expect("utf8 aegis path"),
        ],
        temp.path(),
        &[],
    );

    assert!(
        !output.status.success(),
        "setup must not write Aegis as the real shell via --shell"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("infinite recursion"),
        "expected recursion error"
    );
    assert!(
        !rc_file.exists(),
        "rc file must not be written when --shell resolves to Aegis"
    );
}

#[cfg(unix)]
#[test]
fn setup_shell_rejects_explicit_shell_symlink_to_aegis_binary() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::TempDir::new().expect("temp dir");
    let aegis = aegis_bin();
    let symlink_path = temp.path().join("explicit-shell-link");
    symlink(&aegis, &symlink_path).expect("create symlink to aegis binary");
    let rc_file = temp.path().join(".zshrc");

    let output = run_setup_shell(
        &[
            "--shell",
            symlink_path.to_str().expect("utf8 symlink path"),
            "--rc-file",
            rc_file.to_str().expect("utf8 rc path"),
            "--aegis-bin",
            aegis.to_str().expect("utf8 aegis path"),
        ],
        temp.path(),
        &[],
    );

    assert!(
        !output.status.success(),
        "setup must not recurse when --shell is a symlink to Aegis"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("infinite recursion"),
        "expected recursion error"
    );
    assert!(
        !rc_file.exists(),
        "rc file must not be written when --shell is a symlink to Aegis"
    );
}
