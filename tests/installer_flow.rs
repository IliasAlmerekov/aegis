use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

fn write_fake_release_binary(path: &Path) {
    write_executable(path, "#!/bin/sh\necho 'aegis 1.0.0'\n");
}

fn write_curl_stub(path: &Path) {
    write_executable(
        path,
        r#"#!/bin/sh
set -eu

output=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --output)
            output="$2"
            shift 2
            ;;
        *)
            shift
            ;;
    esac
done

cp "${TEST_ASSET}" "${output}"
"#,
    );
}

fn run_script(script_name: &str, envs: &[(&str, &str)]) -> Output {
    let mut command = Command::new("/bin/sh");
    command.arg(script_path(script_name));

    for (key, value) in envs {
        command.env(key, value);
    }

    command.output().unwrap()
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
    let asset = temp.path().join("aegis-linux-x86_64");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let original_path = std::env::var("PATH").unwrap();

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&asset);
    write_curl_stub(&curl_stub);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let asset_str = asset.display().to_string();
    let path_value = format!("{}:{}", stub_dir.display(), original_path);

    let output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_ASSET", &asset_str),
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
            ("TEST_ASSET", &asset_str),
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
    let asset = temp.path().join("aegis-linux-x86_64");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let original_path = std::env::var("PATH").unwrap();

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "").unwrap();
    write_fake_release_binary(&asset);
    write_curl_stub(&curl_stub);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let asset_str = asset.display().to_string();
    let path_value = format!("{}:{}", stub_dir.display(), original_path);
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
            ("TEST_ASSET", &asset_str),
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
fn uninstall_script_removes_managed_block_and_binary() {
    let temp = TempDir::new().unwrap();
    let bindir = temp.path().join("bin");
    let rc_file = temp.path().join(".bashrc");
    let asset = temp.path().join("aegis-linux-x86_64");
    let stub_dir = temp.path().join("stub-bin");
    let curl_stub = stub_dir.join("curl");
    let original_path = std::env::var("PATH").unwrap();

    fs::create_dir_all(&stub_dir).unwrap();
    fs::write(&rc_file, "export FOO=bar\n").unwrap();
    write_fake_release_binary(&asset);
    write_curl_stub(&curl_stub);

    let bindir_str = bindir.display().to_string();
    let rc_file_str = rc_file.display().to_string();
    let asset_str = asset.display().to_string();
    let path_value = format!("{}:{}", stub_dir.display(), original_path);

    let install_output = run_script(
        "install.sh",
        &[
            ("AEGIS_BINDIR", &bindir_str),
            ("AEGIS_SHELL_RC", &rc_file_str),
            ("AEGIS_OS", "linux"),
            ("AEGIS_ARCH", "x86_64"),
            ("PATH", &path_value),
            ("SHELL", "/bin/bash"),
            ("TEST_ASSET", &asset_str),
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
