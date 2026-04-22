use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::fs::symlink;

fn script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

fn find_command_on_path(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .find(|candidate| candidate.exists())
        })
        .unwrap_or_else(|| panic!("failed to find {name} on PATH"))
}

#[cfg(unix)]
fn write_command_shim(path: &Path, target: &Path) {
    let _ = fs::remove_file(path);
    symlink(target, path).unwrap();
}

#[cfg(not(unix))]
fn write_command_shim(_path: &Path, _target: &Path) {
    panic!("installer_flow tests require Unix symlink support");
}

fn write_host_command_shims(dir: &Path, commands: &[&str]) {
    fs::create_dir_all(dir).unwrap();

    for command in commands {
        let target = find_command_on_path(command);
        write_command_shim(&dir.join(command), &target);
    }
}

fn write_fake_release_binary(path: &Path) {
    write_executable(path, "#!/bin/sh\necho 'aegis 1.0.0'\n");
}

fn aegis_test_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_aegis")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("CARGO_BIN_EXE_aegis is not set for installer flow tests"))
}

fn copy_release_binary(source: &Path, target: &Path) {
    fs::copy(source, target).unwrap();

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(target).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(target, permissions).unwrap();
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn write_release_checksum(path: &Path, asset_name: &str, digest: &str) {
    fs::write(path, format!("{digest}  {asset_name}\n")).unwrap();
}

fn write_curl_stub(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
set -eu

output=""
url=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            output="$2"
            shift 2
            ;;
        --*)
            shift
            ;;
        *)
            url="$1"
            shift
            ;;
    esac
done

case "${url}" in
    *.sha256)
        if [ "${TEST_CHECKSUM_MODE:-present}" = "missing" ]; then
            printf 'checksum asset missing\n' >&2
            exit 22
        fi
        cp "${TEST_CHECKSUM_ASSET}" "${output}"
        ;;
    *)
        cp "${TEST_BINARY_ASSET:-${TEST_ASSET:-}}" "${output}"
        ;;
esac
"#,
    );
}

fn write_sha256sum_stub(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
set -eu

asset_name="$(basename "${TEST_BINARY_ASSET:-${TEST_ASSET:-}}")"

verify_checksum_file() {
    checksum_file="$1"
    awk -v expected="${TEST_BINARY_DIGEST}" -v asset="${asset_name}" '
        NF >= 2 {
            file = $2
            sub(/^\*/, "", file)
            if ($1 == expected && file == asset) {
                found = 1
                exit 0
            }
        }
        END {
            if (found != 1) {
                exit 1
            }
        }
    ' "${checksum_file}"
}

mode="print"
checksum_file=""
file=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        -c|--check)
            mode="check"
            shift
            ;;
        --status|--quiet|--warn|--zero)
            shift
            ;;
        --)
            shift
            while [ "$#" -gt 0 ]; do
                if [ -z "${file}" ]; then
                    file="$1"
                fi
                shift
            done
            break
            ;;
        -*)
            shift
            ;;
        *)
            if [ -z "${file}" ]; then
                file="$1"
            fi
            shift
            ;;
    esac
done

if [ "${mode}" = "check" ]; then
    checksum_file="${file}"
    [ -n "${checksum_file}" ] || exit 64
    verify_checksum_file "${checksum_file}"
    exit 0
fi

[ -n "${file}" ] || exit 64
printf '%s  %s\n' "${TEST_BINARY_DIGEST}" "${file}"
"#,
    );
}

fn write_shasum_stub(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
set -eu

asset_name="$(basename "${TEST_BINARY_ASSET:-${TEST_ASSET:-}}")"

verify_checksum_file() {
    checksum_file="$1"
    awk -v expected="${TEST_BINARY_DIGEST}" -v asset="${asset_name}" '
        NF >= 2 {
            file = $2
            sub(/^\*/, "", file)
            if ($1 == expected && file == asset) {
                found = 1
                exit 0
            }
        }
        END {
            if (found != 1) {
                exit 1
            }
        }
    ' "${checksum_file}"
}

mode=""
checksum_file=""
file=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -a)
            [ "${2:-}" = "256" ] || exit 64
            shift 2
            ;;
        -c|--check)
            mode="check"
            shift
            ;;
        --status|--quiet|--warn|--zero)
            shift
            ;;
        --)
            shift
            while [ "$#" -gt 0 ]; do
                if [ -z "${file}" ]; then
                    file="$1"
                fi
                shift
            done
            break
            ;;
        -*)
            shift
            ;;
        *)
            if [ -z "${file}" ]; then
                file="$1"
            fi
            shift
            ;;
    esac
done

if [ "${mode}" = "check" ]; then
    checksum_file="${file}"
    [ -n "${checksum_file}" ] || exit 64
    verify_checksum_file "${checksum_file}"
    exit 0
fi

[ -n "${file}" ] || exit 64
printf '%s  %s\n' "${TEST_BINARY_DIGEST}" "${file}"
"#,
    );
}

fn prepare_checksum_ready_release(
    temp: &TempDir,
    stub_dir: &Path,
) -> (PathBuf, PathBuf, String, String) {
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");

    fs::create_dir_all(stub_dir).unwrap();
    write_fake_release_binary(&binary_asset);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&stub_dir.join("curl"));
    write_sha256sum_stub(&stub_dir.join("sha256sum"));
    write_shasum_stub(&stub_dir.join("shasum"));

    let path_value = installer_path(temp, stub_dir);

    (binary_asset, checksum_asset, binary_digest, path_value)
}

fn prepare_real_binary_release(
    temp: &TempDir,
    stub_dir: &Path,
) -> (PathBuf, PathBuf, String, String) {
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");

    fs::create_dir_all(stub_dir).unwrap();
    copy_release_binary(&aegis_test_binary(), &binary_asset);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&stub_dir.join("curl"));
    write_sha256sum_stub(&stub_dir.join("sha256sum"));
    write_shasum_stub(&stub_dir.join("shasum"));

    let path_value = installer_path(temp, stub_dir);

    (binary_asset, checksum_asset, binary_digest, path_value)
}

fn installer_path(temp: &TempDir, stub_dir: &Path) -> String {
    let host_dir = temp.path().join("host-bin");
    write_host_command_shims(
        &host_dir,
        &[
            "mktemp", "dirname", "cp", "mkdir", "uname", "basename", "awk", "install", "chmod",
            "rm", "mv", "cat", "cut", "grep", "sed", "jq",
        ],
    );

    format!("{}:{}", stub_dir.display(), host_dir.display())
}

fn write_failing_aegis_on_path(path: &Path, log_path: &Path) {
    write_executable(
        path,
        &format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> '{}'\nprintf 'unexpected PATH aegis invocation\\n' >&2\nexit 99\n",
            log_path.display()
        ),
    );
}

fn run_script_at(script_path: &Path, envs: &[(&str, &str)]) -> Output {
    let sandbox_home = TempDir::new().unwrap();
    let mut command = Command::new("/bin/sh");
    command.arg(script_path);
    command.env_remove("AEGIS_REAL_SHELL");
    command.env_remove("AEGIS_SHELL_RC");
    command.env("HOME", sandbox_home.path());

    for (key, value) in envs {
        command.env(key, value);
    }

    command.output().unwrap()
}

fn run_script(script_name: &str, envs: &[(&str, &str)]) -> Output {
    let temp = TempDir::new().unwrap();
    let script_copy = temp.path().join(script_name);
    fs::copy(script_path(script_name), &script_copy).unwrap();
    run_script_at(&script_copy, envs)
}

fn run_piped_script_with_tty(script_name: &str, envs: &[(&str, &str)], input: &str) -> Output {
    let sandbox_home = TempDir::new().unwrap();
    let script_cmd = find_command_on_path("script");
    let installer_script = script_path(script_name);
    let mut command = Command::new(script_cmd);

    command
        .arg("-qec")
        .arg("cat \"$AEGIS_INSTALLER_SCRIPT\" | /bin/sh")
        .arg("/dev/null")
        .env("AEGIS_INSTALLER_SCRIPT", &installer_script)
        .env_remove("AEGIS_REAL_SHELL")
        .env_remove("AEGIS_SHELL_RC")
        .env("HOME", sandbox_home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in envs {
        command.env(key, value);
    }

    let mut child = command.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    child.wait_with_output().unwrap()
}

fn managed_block(real_shell: &Path, aegis_path: &Path) -> String {
    format!(
        "# >>> aegis shell setup >>>\nexport AEGIS_REAL_SHELL=\"{}\"\nexport SHELL=\"{}\"\n# <<< aegis shell setup <<<\n",
        real_shell.display(),
        aegis_path.display()
    )
}

#[test]
fn install_script_configures_shell_wrapper_block_once() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let aegis_path = bindir.join("aegis");
    assert!(
        aegis_path.exists(),
        "installer must place binary into bindir"
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    let expected_block = managed_block(Path::new("/bin/bash"), &aegis_path);
    assert!(
        rc_contents.contains(&expected_block),
        "install must append managed shell-wrapper block; rc contents:\n{rc_contents}"
    );
    assert_eq!(
        rc_contents.matches("# >>> aegis shell setup >>>").count(),
        1
    );

    let second_output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        second_output.status.success(),
        "second install must also succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );

    let rc_contents_after_second_run = fs::read_to_string(&rc_file).unwrap();
    assert_eq!(
        rc_contents_after_second_run, rc_contents,
        "installer must be deterministic and not duplicate the managed block"
    );
}

#[test]
fn install_script_prefers_aegis_real_shell_when_shell_already_points_to_wrapper() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".zshrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let wrapper_shell = bindir.join("aegis");
    let wrapper_shell_str = wrapper_shell.display().to_string();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", &wrapper_shell_str),
            ("AEGIS_REAL_SHELL", "/bin/zsh"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "install must succeed from an already-wrapped shell when AEGIS_REAL_SHELL is set"
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    let expected_block = managed_block(Path::new("/bin/zsh"), &wrapper_shell);
    assert!(
        rc_contents.contains(&expected_block),
        "managed block must preserve the real shell to avoid recursion; rc contents:\n{rc_contents}"
    );
}

#[test]
fn install_script_rejects_unsafe_real_shell_value_before_rc_mutation() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash\nexport EVIL=1"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        !output.status.success(),
        "install must reject unsafe real shell values: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid real shell path"),
        "installer must explain why the shell path was rejected"
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    assert_eq!(
        rc_contents, "export FOO=bar\n",
        "unsafe real shell values must not mutate the rc file"
    );
    assert!(
        !bindir.join("aegis").exists(),
        "unsafe real shell values must abort before installing the binary"
    );
}

#[test]
fn install_script_rejects_checksum_mismatch_before_touching_bindir() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let sha256sum_stub = stub_dir.join("sha256sum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    write_release_checksum(
        &checksum_asset,
        "aegis-linux-x86_64",
        "0000000000000000000000000000000000000000000000000000000000000000",
    );
    write_curl_stub(&curl_stub);
    write_sha256sum_stub(&sha256sum_stub);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "checksum mismatch must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum verification failed"));
}

#[test]
fn install_script_rejects_missing_checksum_before_touching_bindir() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let sha256sum_stub = stub_dir.join("sha256sum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    write_release_checksum(
        &checksum_asset,
        "aegis-linux-x86_64",
        "1111111111111111111111111111111111111111111111111111111111111111",
    );
    write_curl_stub(&curl_stub);
    write_sha256sum_stub(&sha256sum_stub);

    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_CHECKSUM_MODE", "missing"),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "missing checksum must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("checksum download failed"));
}

#[test]
fn install_script_fails_when_no_supported_checksum_tool_exists() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&curl_stub);

    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(!output.status.success());
    assert!(
        !bindir.join("aegis").exists(),
        "missing checksum tools must leave final bindir untouched"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("no supported checksum tool found"));
}

#[test]
fn install_script_falls_back_to_shasum_when_sha256sum_is_missing() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let binary_asset = temp.path().join("aegis-linux-x86_64");
    let checksum_asset = temp.path().join("aegis-linux-x86_64.sha256");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let shasum_stub = stub_dir.join("shasum");

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&binary_asset);
    let binary_digest = sha256_hex(&fs::read(&binary_asset).unwrap());
    write_release_checksum(&checksum_asset, "aegis-linux-x86_64", &binary_digest);
    write_curl_stub(&curl_stub);
    write_shasum_stub(&shasum_stub);

    let path_value = installer_path(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "fallback to shasum must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(bindir.join("aegis").exists());
}

#[test]
fn install_script_rejects_deprecated_setup_controls_before_mutation() {
    for (env_key, env_value) in [
        ("AEGIS_SETUP_MODE", "binary"),
        ("AEGIS_SKIP_SHELL_SETUP", "1"),
    ] {
        let temp = TempDir::new().unwrap();
        let bindir = temp.path().join("bin");
        let rc_file = temp.path().join(".bashrc");
        let stub_dir = temp.path().join("stub-bin");

        fs::write(&rc_file, "export FOO=bar\n").unwrap();
        let (binary_asset, checksum_asset, binary_digest, path_value) =
            prepare_checksum_ready_release(&temp, &stub_dir);
        let bindir_str = bindir.display().to_string();
        let rc_file_str = rc_file.display().to_string();
        let binary_asset_str = binary_asset.display().to_string();
        let checksum_asset_str = checksum_asset.display().to_string();

        let output = run_script(
            "install.sh",
            &[
                ("AEGIS_BINDIR", &bindir_str),
                ("AEGIS_SHELL_RC", &rc_file_str),
                ("AEGIS_OS", "linux"),
                ("AEGIS_ARCH", "x86_64"),
                (env_key, env_value),
                ("PATH", &path_value),
                ("SHELL", "/bin/bash"),
                ("TEST_BINARY_ASSET", &binary_asset_str),
                ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
                ("TEST_BINARY_DIGEST", &binary_digest),
            ],
        );

        assert!(
            !output.status.success(),
            "installer must reject deprecated control {env_key}; stdout=\n{}\nstderr=\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(env_key),
            "error should name the deprecated control {env_key}; stderr=\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !bindir.join("aegis").exists(),
            "deprecated controls must be rejected before touching bindir"
        );
        let rc_contents = fs::read_to_string(&rc_file).unwrap();
        assert_eq!(
            rc_contents, "export FOO=bar\n",
            "deprecated controls must be rejected before mutating rc files"
        );
    }
}

#[test]
fn install_script_rejects_unsupported_shell_before_mutation() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/fish"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        !output.status.success(),
        "unsupported shells must fail before any installation occurs: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("automatic shell setup supports bash and zsh"),
        "unsupported shell error should explain the bash/zsh limitation; stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !bindir.join("aegis").exists(),
        "unsupported shell must fail before downloading or installing the binary"
    );
    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    assert_eq!(
        rc_contents, "export FOO=bar\n",
        "unsupported shell must fail before mutating rc files"
    );
}

#[test]
fn install_script_global_setup_writes_shell_setup() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "global mode must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    let aegis_path = bindir.join("aegis");
    let expected_block = managed_block(Path::new("/bin/bash"), &aegis_path);
    assert!(
        rc_contents.contains(&expected_block),
        "global setup must write managed block; rc contents:\n{rc_contents}"
    );
}

#[test]
fn install_script_skips_agent_setup_honestly_without_detected_agents() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let rogue_log = temp.path().join("rogue-aegis.log");
    let home = temp.path().join("home");

    fs::create_dir_all(&home).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);
    write_failing_aegis_on_path(&stub_dir.join("aegis"), &rogue_log);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must succeed even without detectable agents: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected_follow_up = format!(
        "If you install Claude Code or Codex later, run:\n  {}/aegis install-hooks --all",
        bindir.display()
    );
    assert!(
        stdout.contains("Agent hook setup skipped; no supported agent directories were detected."),
        "installer should explain that no hooks were installed when no agent dirs are present; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains(&expected_follow_up),
        "installer should point users at the installed binary for follow-up hook setup; stdout=\n{stdout}"
    );
    assert!(
        !stdout.contains("Agent hook setup completed automatically."),
        "installer must not claim success when no agent dirs were detected; stdout=\n{stdout}"
    );
    assert!(
        bindir.join("aegis").exists(),
        "binary must still be installed"
    );
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "no agent dirs should mean no Claude settings are created"
    );
    assert!(
        !home.join(".codex").join("hooks.json").exists(),
        "no agent dirs should mean no Codex hook files are created"
    );
    assert!(
        !rogue_log.exists(),
        "installer must invoke the installed binary directly instead of PATH aegis"
    );
}

#[test]
fn install_script_auto_installs_codex_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let rogue_log = temp.path().join("rogue-aegis.log");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);
    write_failing_aegis_on_path(&stub_dir.join("aegis"), &rogue_log);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install Codex hooks when Codex is detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "installer should report successful automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Codex: hooks installed")
            || stdout.contains("Codex: hooks already present, skipping"),
        "installer should show Codex hook setup output from the installed binary; stdout=\n{stdout}"
    );
    let codex_hooks = home.join(".codex").join("hooks.json");
    assert!(codex_hooks.exists(), "Codex hooks.json should be created");
    assert!(
        home.join(".codex")
            .join("hooks")
            .join("aegis-pre-tool-use.sh")
            .exists(),
        "Codex pre-tool-use hook should be installed"
    );
    assert!(
        home.join(".codex")
            .join("hooks")
            .join("aegis-session-start.sh")
            .exists(),
        "Codex session-start hook should be installed"
    );
    assert!(
        !home.join(".claude").join("settings.json").exists(),
        "Codex-only install should not seed Claude settings"
    );
    assert!(
        !rogue_log.exists(),
        "installer must invoke the installed binary directly instead of PATH aegis"
    );
}

#[test]
fn install_script_auto_installs_claude_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install Claude hooks when Claude is detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "installer should report successful automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Claude Code: hook installed")
            || stdout.contains("Claude Code: hook already present, skipping"),
        "installer should show Claude hook setup output from the installed binary; stdout=\n{stdout}"
    );
    let claude_settings = fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    assert!(
        claude_settings.contains("\"command\": \"aegis hook\""),
        "Claude settings should point at the aegis hook subcommand; settings.json=\n{claude_settings}"
    );
    assert!(
        !home.join(".codex").join("hooks.json").exists(),
        "Claude-only install should not seed Codex hooks"
    );
}

#[test]
fn install_script_auto_installs_both_agent_hooks_via_installed_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("HOME", &home_str),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );

    assert!(
        output.status.success(),
        "release install must auto-install both hook sets when both agents are detected: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Agent hook setup completed automatically."));
    assert!(
        stdout.contains("Claude Code: hook installed")
            || stdout.contains("Claude Code: hook already present, skipping")
    );
    assert!(
        stdout.contains("Codex: hooks installed")
            || stdout.contains("Codex: hooks already present, skipping")
    );
    assert!(home.join(".claude").join("settings.json").exists());
    assert!(home.join(".codex").join("hooks.json").exists());
}

#[test]
fn install_script_repeated_install_keeps_agent_hook_setup_idempotent() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");
    let home = temp.path().join("home");

    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(home.join(".codex")).unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_real_binary_release(&temp, &stub_dir);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let home_str = home.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();
    let envs = [
        ("AEGIS_BINDIR", bindir_str.as_str()),
        ("AEGIS_SHELL_RC", rc_file_str.as_str()),
        ("AEGIS_OS", "linux"),
        ("AEGIS_ARCH", "x86_64"),
        ("HOME", home_str.as_str()),
        ("PATH", path_value.as_str()),
        ("SHELL", "/bin/bash"),
        ("AEGIS_REAL_SHELL", "/bin/bash"),
        ("TEST_BINARY_ASSET", binary_asset_str.as_str()),
        ("TEST_CHECKSUM_ASSET", checksum_asset_str.as_str()),
        ("TEST_BINARY_DIGEST", binary_digest.as_str()),
    ];

    let first_output = run_script("install.sh", &envs);
    assert!(
        first_output.status.success(),
        "first release install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&first_output.stdout),
        String::from_utf8_lossy(&first_output.stderr)
    );

    let claude_before = fs::read_to_string(home.join(".claude").join("settings.json")).unwrap();
    let codex_before = fs::read_to_string(home.join(".codex").join("hooks.json")).unwrap();

    let second_output = run_script("install.sh", &envs);
    assert!(
        second_output.status.success(),
        "second release install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&second_output.stdout),
        String::from_utf8_lossy(&second_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&second_output.stdout);
    assert!(
        stdout.contains("Agent hook setup completed automatically."),
        "successful re-install should still report automatic hook setup; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Claude Code: hook already present, skipping"),
        "re-install should report Claude idempotence; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Codex: hooks already present, skipping"),
        "re-install should report Codex idempotence; stdout=\n{stdout}"
    );
    assert_eq!(
        fs::read_to_string(home.join(".claude").join("settings.json")).unwrap(),
        claude_before,
        "re-install should not rewrite Claude settings when already configured"
    );
    assert_eq!(
        fs::read_to_string(home.join(".codex").join("hooks.json")).unwrap(),
        codex_before,
        "re-install should not rewrite Codex hooks when already configured"
    );
}

#[test]
fn install_script_global_first_flow_in_tty_session() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let output = run_piped_script_with_tty(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("AEGIS_REAL_SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
        "",
    );

    assert!(
        output.status.success(),
        "piped install must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("How would you like to set up Aegis?"),
        "global-first installer must not prompt for Local/Global/Binary; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Aegis installed globally."),
        "installer should confirm the global default; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Use `aegis off` to disable temporarily."),
        "installer should advertise the new toggle flow; stdout=\n{stdout}"
    );
    assert!(
        stdout.contains("Agent hook setup completed automatically.")
            || stdout.contains(
                "Agent hook setup skipped; no supported agent directories were detected."
            )
            || stdout.contains("Agent hook setup failed."),
        "installer must print an honest hook-setup outcome; stdout=\n{stdout}"
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    let aegis_path = bindir.join("aegis");
    let expected_block = managed_block(Path::new("/bin/bash"), &aegis_path);
    assert!(
        rc_contents.contains(&expected_block),
        "global-first install must write the managed shell wrapper block; rc contents:\n{rc_contents}"
    );
}

#[test]
fn uninstall_script_removes_managed_block_and_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let stub_dir = temp.path().join("stub-bin");

    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    let (binary_asset, checksum_asset, binary_digest, path_value) =
        prepare_checksum_ready_release(&temp, &stub_dir);
    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let binary_asset_str = binary_asset.display().to_string();
    let checksum_asset_str = checksum_asset.display().to_string();

    let install_output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_BINARY_ASSET", &binary_asset_str),
            ("TEST_CHECKSUM_ASSET", &checksum_asset_str),
            ("TEST_BINARY_DIGEST", &binary_digest),
        ],
    );
    assert!(install_output.status.success());

    let uninstall_output = run_script(
        "uninstall.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("SHELL", "/bin/bash"),
        ],
    );

    assert!(
        uninstall_output.status.success(),
        "uninstall must succeed: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&uninstall_output.stdout),
        String::from_utf8_lossy(&uninstall_output.stderr)
    );

    let rc_contents = fs::read_to_string(&rc_file).unwrap();
    assert_eq!(
        rc_contents, "export FOO=bar\n",
        "uninstall must restore the rc file by removing only the managed block"
    );
    assert!(
        !bindir.join("aegis").exists(),
        "uninstall must remove the installed binary"
    );
}

#[test]
fn uninstall_script_does_not_create_missing_rc_file() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");

    fs::create_dir_all(&bindir).unwrap();
    write_fake_release_binary(&bindir.join("aegis"));
    assert!(
        !rc_file.exists(),
        "test setup must start without an rc file"
    );

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();

    let uninstall_output = run_script(
        "uninstall.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("SHELL", "/bin/bash"),
        ],
    );

    assert!(
        uninstall_output.status.success(),
        "uninstall must succeed even when the rc file is absent: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&uninstall_output.stdout),
        String::from_utf8_lossy(&uninstall_output.stderr)
    );
    assert!(
        !rc_file.exists(),
        "uninstall must not create a missing rc file"
    );
    assert!(
        !bindir.join("aegis").exists(),
        "uninstall must still remove the installed binary when the rc file is absent"
    );
}

#[test]
fn uninstall_script_honors_explicit_rc_override_without_shell_detection() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let aegis_path = bindir.join("aegis");

    fs::create_dir_all(&bindir).unwrap();
    write_fake_release_binary(&aegis_path);
    fs::write(
        &rc_file,
        format!(
            "export FOO=bar\n{}",
            managed_block(Path::new("/bin/bash"), &aegis_path)
        ),
    )
    .unwrap();

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let aegis_path_str = aegis_path.display().to_string();

    let uninstall_output = run_script(
        "uninstall.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("SHELL", &aegis_path_str),
        ],
    );

    assert!(
        uninstall_output.status.success(),
        "uninstall must honor explicit rc override even when SHELL points at aegis: stdout=\n{}\nstderr=\n{}",
        String::from_utf8_lossy(&uninstall_output.stdout),
        String::from_utf8_lossy(&uninstall_output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&rc_file).unwrap(),
        "export FOO=bar\n",
        "uninstall must clean the explicit rc file override"
    );
    assert!(
        !aegis_path.exists(),
        "uninstall must still remove the installed binary when using an explicit rc override"
    );
}
