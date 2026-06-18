//! Installer-flow shared helpers extracted verbatim from the original
//! `tests/installer_flow.rs` so the split installer test crates
//! (`installer_checksum`, `installer_platform`, `installer_tty`,
//! `installer_live_release`) can share the same script runners, stub writers,
//! and release-fixture preparation without duplicating logic.
//!
//! These helpers are test-only and intentionally panic on internal errors
//! (`.unwrap()` is acceptable here). `write_executable` is reused from
//! `super` (the parent `support` module) to avoid a name collision.
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::symlink;

pub fn script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("scripts")
        .join(name)
}

pub fn find_command_on_path(name: &str) -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join(name))
                .find(|candidate| candidate.exists())
        })
        .unwrap_or_else(|| panic!("failed to find {name} on PATH"))
}

#[cfg(unix)]
pub fn write_command_shim(path: &Path, target: &Path) {
    let _ = fs::remove_file(path);
    symlink(target, path).unwrap();
}

#[cfg(not(unix))]
pub fn write_command_shim(_path: &Path, _target: &Path) {
    panic!("installer_flow tests require Unix symlink support");
}

pub fn write_host_command_shims(dir: &Path, commands: &[&str]) {
    fs::create_dir_all(dir).unwrap();

    for command in commands {
        let target = find_command_on_path(command);
        write_command_shim(&dir.join(command), &target);
    }
}

pub fn write_fake_release_binary(path: &Path) {
    super::write_executable(path, "#!/bin/sh\necho 'aegis 1.0.0'\n");
}

pub fn aegis_test_binary() -> PathBuf {
    std::env::var_os("CARGO_BIN_EXE_aegis")
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("CARGO_BIN_EXE_aegis is not set for installer flow tests"))
}

pub fn copy_release_binary(source: &Path, target: &Path) {
    fs::copy(source, target).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(target).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(target, permissions).unwrap();
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

pub fn host_asset_name() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("aegis-linux-x86_64"),
        ("linux", "aarch64") => Some("aegis-linux-aarch64"),
        ("macos", "x86_64") => Some("aegis-macos-x86_64"),
        ("macos", "aarch64") => Some("aegis-macos-aarch64"),
        _ => None,
    }
}

pub fn write_release_checksum(path: &Path, asset_name: &str, digest: &str) {
    fs::write(path, format!("{digest}  {asset_name}\n")).unwrap();
}

pub fn write_curl_stub(path: &Path) {
    super::write_executable(
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

pub fn write_sha256sum_stub(path: &Path) {
    super::write_executable(
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

pub fn write_shasum_stub(path: &Path) {
    super::write_executable(
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

pub fn prepare_checksum_ready_release(
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

pub fn prepare_real_binary_release(
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

pub fn installer_path(temp: &TempDir, stub_dir: &Path) -> String {
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

pub fn write_failing_aegis_on_path(path: &Path, log_path: &Path) {
    super::write_executable(
        path,
        &format!(
            "#!/bin/sh\nset -eu\nprintf '%s\\n' \"$*\" >> '{}'\nprintf 'unexpected PATH aegis invocation\\n' >&2\nexit 99\n",
            log_path.display()
        ),
    );
}

pub fn run_script_at(script_path: &Path, envs: &[(&str, &str)]) -> Output {
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

pub fn run_script(script_name: &str, envs: &[(&str, &str)]) -> Output {
    let temp = TempDir::new().unwrap();
    let script_copy = temp.path().join(script_name);
    fs::copy(script_path(script_name), &script_copy).unwrap();
    run_script_at(&script_copy, envs)
}

#[derive(Clone, Copy)]
pub enum ScriptFlavor {
    Bsd,
    UtilLinux,
}

pub fn script_tty_args(flavor: ScriptFlavor) -> Vec<&'static str> {
    match flavor {
        ScriptFlavor::Bsd => vec![
            "-q",
            "/dev/null",
            "/bin/sh",
            "-c",
            "cat \"$AEGIS_INSTALLER_SCRIPT\" | /bin/sh",
        ],
        ScriptFlavor::UtilLinux => vec![
            "-qec",
            "cat \"$AEGIS_INSTALLER_SCRIPT\" | /bin/sh",
            "/dev/null",
        ],
    }
}

pub fn run_piped_script_with_tty(script_name: &str, envs: &[(&str, &str)], input: &str) -> Output {
    let sandbox_home = TempDir::new().unwrap();
    let script_cmd = find_command_on_path("script");
    let installer_script = script_path(script_name);
    let mut command = Command::new(script_cmd);
    let script_flavor = if cfg!(target_os = "macos") {
        ScriptFlavor::Bsd
    } else {
        ScriptFlavor::UtilLinux
    };

    command
        .args(script_tty_args(script_flavor))
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

pub fn managed_block(real_shell: &Path, aegis_path: &Path) -> String {
    format!(
        "# >>> aegis shell setup >>>\nexport AEGIS_REAL_SHELL=\"{}\"\nexport SHELL=\"{}\"\n# <<< aegis shell setup <<<\n",
        real_shell.display(),
        aegis_path.display()
    )
}
