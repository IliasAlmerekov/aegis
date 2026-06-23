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
    assert!(rc.contains("export AEGIS_REAL_SHELL=\"/bin/zsh\""));
    assert!(rc.contains("export SHELL=\"/usr/local/bin/aegis\""));
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

// `--aegis-bin` is interpolated raw into `export SHELL="..."`. A payload with
// quotes/semicolons must be rejected before any rc file is written, otherwise
// the generated startup file contains injected shell commands.
#[test]
fn setup_shell_rejects_shell_injection_via_aegis_bin() {
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
        !output.status.success(),
        "setup must reject shell-injection payloads in --aegis-bin"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsafe characters"),
        "expected unsafe-characters error, got: {stderr}"
    );
    // The rc file must not be created when validation fails.
    assert!(
        !rc_file.exists(),
        "rc file must not be written when --aegis-bin validation fails"
    );
}
