#!/bin/sh
set -eu

REPO="${AEGIS_REPO:-IliasAlmerekov/aegis}"
VERSION="${AEGIS_VERSION:-latest}"
BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
OS_OVERRIDE="${AEGIS_OS:-}"
ARCH_OVERRIDE="${AEGIS_ARCH:-}"
SHELL_RC_OVERRIDE="${AEGIS_SHELL_RC:-}"
SKIP_SHELL_SETUP="${AEGIS_SKIP_SHELL_SETUP:-0}"
SETUP_MODE="${AEGIS_SETUP_MODE:-}"

BEGIN_MARKER="# >>> aegis shell setup >>>"
END_MARKER="# <<< aegis shell setup <<<"

cleanup() {
    if [ -n "${TMPDIR_AEGIS:-}" ] && [ -d "${TMPDIR_AEGIS}" ]; then
        rm -rf "${TMPDIR_AEGIS}"
    fi
}

trap cleanup EXIT INT TERM

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1
}

target_path() {
    printf '%s/aegis\n' "${BINDIR}"
}

remove_managed_block() {
    input_path="$1"
    output_path="$2"

    if [ ! -f "${input_path}" ]; then
        : > "${output_path}"
        return
    fi

    awk -v begin="${BEGIN_MARKER}" -v end="${END_MARKER}" '
        $0 == begin { skip = 1; next }
        $0 == end { skip = 0; next }
        skip != 1 { print }
    ' "${input_path}" > "${output_path}"
}

write_shell_setup() {
    rc_file="$1"
    real_shell="$2"
    aegis_path="$3"
    tmp_rc="${TMPDIR_AEGIS}/rc.tmp"

    mkdir -p "$(dirname "${rc_file}")"
    remove_managed_block "${rc_file}" "${tmp_rc}"
    cp "${tmp_rc}" "${rc_file}"

    cat >> "${rc_file}" <<EOF
${BEGIN_MARKER}
export AEGIS_REAL_SHELL="${real_shell}"
export SHELL="${aegis_path}"
${END_MARKER}
EOF
}

has_prompt_tty() {
    if [ ! -r /dev/tty ] || [ ! -w /dev/tty ]; then
        return 1
    fi

    if ( : </dev/tty >/dev/tty ) >/dev/null 2>&1; then
        return 0
    fi

    return 1
}

prompt_setup_mode() {
    prompt_device=""
    read_device=""

    if [ -n "${SETUP_MODE}" ]; then
        printf '%s\n' "${SETUP_MODE}"
        return
    fi

    if has_prompt_tty; then
        prompt_device="/dev/tty"
        read_device="/dev/tty"
    elif [ -t 0 ] && [ -t 1 ]; then
        prompt_device="/dev/stdout"
        read_device="/dev/stdin"
    else
        printf '%s\n' 'No interactive terminal detected; defaulting to Global setup.' >&2
        printf '%s\n' \
            'To choose explicitly, rerun with AEGIS_SETUP_MODE=global|local|binary or download the script first and run it directly.' >&2
        printf 'global\n'
        return
    fi

    printf '\n' >"${prompt_device}"
    printf 'How would you like to set up Aegis?\n' >"${prompt_device}"
    printf '\n' >"${prompt_device}"
    printf '  [1] Global    — protect all shells (writes to ~/.bashrc or ~/.zshrc)\n' >"${prompt_device}"
    printf '  [2] Local     — protect this project only (starts a shielded shell now)\n' >"${prompt_device}"
    printf '  [3] Binary    — install the binary, skip shell setup\n' >"${prompt_device}"
    printf '\n' >"${prompt_device}"
    printf 'Choose [1/2/3]: ' >"${prompt_device}"
    read -r choice <"${read_device}"

    case "${choice}" in
        1|global|Global)
            printf 'global\n'
            ;;
        2|local|Local)
            printf 'local\n'
            ;;
        3|binary|Binary)
            printf 'binary\n'
            ;;
        *)
            printf 'global\n'
            ;;
    esac
}

setup_local_project() {
    real_shell="$1"
    aegis_path="$2"
    project_dir="$(pwd)"
    aegis_dir="${project_dir}/.aegis"

    mkdir -p "${aegis_dir}"

    cat > "${aegis_dir}/enter.sh" <<ENTER_EOF
#!/bin/sh
# Aegis — enter a protected shell for this project.
# Run this script to re-enter the aegis-shielded session.

AEGIS_BIN="${aegis_path}"
REAL_SHELL="${real_shell}"

if [ ! -x "\${AEGIS_BIN}" ]; then
    printf 'error: aegis binary not found at %s\\n' "\${AEGIS_BIN}" >&2
    printf 'Re-run the installer or set AEGIS_BIN.\\n' >&2
    exit 1
fi

export AEGIS_REAL_SHELL="\${REAL_SHELL}"
export SHELL="\${AEGIS_BIN}"

exec "\${REAL_SHELL}"
ENTER_EOF

    chmod 0755 "${aegis_dir}/enter.sh"

    if [ ! -f "${project_dir}/.aegis.toml" ]; then
        if "${aegis_path}" config init >/dev/null 2>&1; then
            :
        fi
    fi
}

enter_local_shell() {
    real_shell="$1"
    aegis_path="$2"

    export AEGIS_REAL_SHELL="${real_shell}"
    export SHELL="${aegis_path}"

    exec "${real_shell}"
}

print_local_post_install() {
    project_dir="$(pwd)"

    cat <<'EOF'

Aegis is now protecting this project.

You are inside an aegis-shielded shell. AI agents (Claude Code,
Codex, etc.) launched here will have their commands intercepted.

To re-enter this protected shell later, run:
EOF
    printf '  %s/.aegis/enter.sh\n' "${project_dir}"
    cat <<'EOF'

To stop protection, simply exit this shell (Ctrl-D or `exit`).

Rollback:
  curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh
EOF
}

print_banner() {
    cat <<'BANNER'

     _    _____ ____ ___ ____
    / \  | ____/ ___|_ _/ ___|
   / _ \ |  _|| |  _ | |\___ \
  / ___ \| |__| |_| || | ___) |
 /_/   \_\_____\____|___|____/

 Shield your terminal from AI agents

BANNER
}

detect_real_shell() {
    aegis_path="$(target_path)"

    if [ -n "${AEGIS_REAL_SHELL:-}" ]; then
        real_shell="${AEGIS_REAL_SHELL}"
    elif [ -n "${SHELL:-}" ]; then
        real_shell="${SHELL}"
    else
        fail "cannot determine the real shell; set AEGIS_REAL_SHELL or SHELL and rerun"
    fi

    if [ "${real_shell}" = "${aegis_path}" ]; then
        fail "refusing to wrap ${aegis_path} recursively; set AEGIS_REAL_SHELL to the real shell and rerun"
    fi

    printf '%s\n' "${real_shell}"
}

resolve_rc_file() {
    real_shell="$1"

    if [ -n "${SHELL_RC_OVERRIDE}" ]; then
        printf '%s\n' "${SHELL_RC_OVERRIDE}"
        return
    fi

    shell_name="$(basename "${real_shell}")"

    case "${shell_name}" in
        bash)
            printf '%s/.bashrc\n' "${HOME}"
            ;;
        zsh)
            printf '%s/.zshrc\n' "${HOME}"
            ;;
        *)
            fail "automatic shell setup supports bash and zsh; set AEGIS_SHELL_RC for ${shell_name}"
            ;;
    esac
}

detect_os() {
    raw_os=""

    if [ -n "${OS_OVERRIDE}" ]; then
        raw_os="${OS_OVERRIDE}"
    else
        raw_os="$(uname -s)"
    fi

    case "${raw_os}" in
        Linux | linux)
            printf 'linux\n'
            ;;
        Darwin | darwin | macOS | macos)
            printf 'macos\n'
            ;;
        *)
            fail "unsupported operating system: ${raw_os}"
            ;;
    esac
}

detect_arch() {
    raw_arch=""

    if [ -n "${ARCH_OVERRIDE}" ]; then
        raw_arch="${ARCH_OVERRIDE}"
    else
        raw_arch="$(uname -m)"
    fi

    case "${raw_arch}" in
        x86_64 | amd64)
            printf 'x86_64\n'
            ;;
        arm64 | aarch64)
            printf 'aarch64\n'
            ;;
        *)
            fail "unsupported architecture: ${raw_arch}"
            ;;
    esac
}

resolve_base_url() {
    if [ -n "${AEGIS_BASE_URL:-}" ]; then
        printf '%s\n' "${AEGIS_BASE_URL}"
        return
    fi

    if [ "${VERSION}" = "latest" ]; then
        printf 'https://github.com/%s/releases/latest/download\n' "${REPO}"
    else
        printf 'https://github.com/%s/releases/download/%s\n' "${REPO}" "${VERSION}"
    fi
}

download() {
    url="$1"
    output="$2"

    if need_cmd curl; then
        if curl --fail --location --silent --show-error "$url" --output "$output"; then
            return 0
        fi
        return 1
    fi

    if need_cmd wget; then
        if wget --quiet "$url" --output-document="$output"; then
            return 0
        fi
        return 1
    fi

    return 1
}

download_or_fail() {
    label="$1"
    url="$2"
    output="$3"

    if download "${url}" "${output}"; then
        return 0
    fi

    fail "${label} download failed"
}

select_checksum_tool() {
    if need_cmd sha256sum; then
        printf 'sha256sum\n'
        return
    fi

    if need_cmd shasum; then
        printf 'shasum\n'
        return
    fi

    fail "no supported checksum tool found"
}

read_expected_checksum() {
    checksum_path="$1"
    asset_name="$2"
    expected_checksum=""

    if [ ! -r "${checksum_path}" ]; then
        fail "checksum verification failed"
    fi

    if expected_checksum="$(awk -v asset="${asset_name}" '
        NF >= 2 {
            file = $2
            sub(/^\*/, "", file)
            if (file == asset) {
                print $1
                found = 1
                exit 0
            }
        }
        END {
            if (found != 1) {
                exit 1
            }
        }
    ' "${checksum_path}")"; then
        if [ -n "${expected_checksum}" ]; then
            printf '%s\n' "${expected_checksum}"
            return 0
        fi
    fi

    fail "checksum verification failed"
}

compute_actual_checksum() {
    checksum_tool="$1"
    binary_path="$2"
    checksum_output=""
    actual_checksum=""

    case "${checksum_tool}" in
        sha256sum)
            if checksum_output="$(sha256sum "${binary_path}")"; then
                actual_checksum="${checksum_output%% *}"
            else
                fail "checksum verification failed"
            fi
            ;;
        shasum)
            if checksum_output="$(shasum -a 256 "${binary_path}")"; then
                actual_checksum="${checksum_output%% *}"
            else
                fail "checksum verification failed"
            fi
            ;;
        *)
            fail "checksum verification failed"
            ;;
    esac

    if [ -n "${actual_checksum}" ]; then
        printf '%s\n' "${actual_checksum}"
        return 0
    fi

    fail "checksum verification failed"
}

verify_downloaded_binary() {
    binary_path="$1"
    checksum_path="$2"
    asset_name="$3"
    checksum_tool=""
    expected_checksum=""
    actual_checksum=""

    checksum_tool="$(select_checksum_tool)"
    expected_checksum="$(read_expected_checksum "${checksum_path}" "${asset_name}")"
    actual_checksum="$(compute_actual_checksum "${checksum_tool}" "${binary_path}")"

    if [ "${expected_checksum}" = "${actual_checksum}" ]; then
        return 0
    fi

    fail "checksum verification failed"
}

install_binary() {
    source_path="$1"
    install_target="$(target_path)"

    if [ ! -d "${BINDIR}" ]; then
        if [ -w "$(dirname "${BINDIR}")" ]; then
            mkdir -p "${BINDIR}"
        elif need_cmd sudo; then
            sudo mkdir -p "${BINDIR}"
        else
            fail "cannot create ${BINDIR}; rerun as root or install sudo"
        fi
    fi

    if [ -w "${BINDIR}" ]; then
        install -m 0755 "${source_path}" "${install_target}"
        return
    fi

    if need_cmd sudo; then
        sudo install -m 0755 "${source_path}" "${install_target}"
        return
    fi

    fail "cannot write to ${BINDIR}; rerun as root or install sudo"
}

print_post_install() {
    rc_file="$1"

    cat <<'EOF'

Post-install setup complete.

The installer added an Aegis-managed shell wrapper block to:
EOF
    printf '  %s\n' "${rc_file}"
    cat <<'EOF'

Open a new shell (or source the file above) to activate it.

Rollback:
  curl -fsSL https://raw.githubusercontent.com/IliasAlmerekov/aegis/main/scripts/uninstall.sh | sh

  Claude Code:
  Open Claude Code settings and set the shell path to the output of `which aegis`.
EOF
}

print_agent_setup_next_steps() {
    cat <<'EOF'

Agent hook setup is only available from a local checkout, because
scripts/agent-setup.sh depends on the sibling files in scripts/hooks/.

From a cloned repository, run:
  sh scripts/agent-setup.sh

If you only want to verify the installed binary path, run:
  command -v aegis
EOF
}

resolve_local_agent_setup() {
    script_dir=""

    case "$0" in
        */*)
            script_dir="$(CDPATH= cd "$(dirname "$0")" && pwd)"
            ;;
        *)
            return 1
            ;;
    esac

    if [ -f "${script_dir}/agent-setup.sh" ] \
        && [ -f "${script_dir}/hooks/claude-code.sh" ] \
        && [ -f "${script_dir}/hooks/codex-pre-tool-use.sh" ] \
        && [ -f "${script_dir}/hooks/codex-session-start.sh" ]; then
        printf '%s/agent-setup.sh\n' "${script_dir}"
        return 0
    fi

    return 1
}

offer_agent_setup() {
    agent_setup_script=""
    answer="n"

    if ! has_prompt_tty; then
        return 0
    fi

    printf '\nAgent hook setup — routes AI agent shell commands through aegis:\n'
    printf '  y) Install now for detected agents (Claude Code / Codex)\n'
    printf '  n) Skip\n'
    printf 'Install agent hooks? [y/N] '

    if ! read -r answer </dev/tty; then
        answer="n"
    fi

    case "${answer}" in
        y|Y|yes|YES)
            if agent_setup_script="$(resolve_local_agent_setup)"; then
                if ! /bin/sh "${agent_setup_script}"; then
                    printf 'Agent setup failed.\n'
                    print_agent_setup_next_steps
                fi
            else
                print_agent_setup_next_steps
            fi
            ;;
        *)
            print_agent_setup_next_steps
            ;;
    esac
}

main() {
    print_banner

    os=""
    arch=""
    asset=""
    base_url=""
    download_url=""
    checksum_url=""
    binary_path=""
    checksum_path=""
    mode=""

    os="$(detect_os)"
    arch="$(detect_arch)"
    asset="aegis-${os}-${arch}"
    base_url="$(resolve_base_url)"
    download_url="${base_url}/${asset}"
    checksum_url="${download_url}.sha256"

    TMPDIR_AEGIS="$(mktemp -d)"
    binary_path="${TMPDIR_AEGIS}/aegis"
    checksum_path="${TMPDIR_AEGIS}/aegis.sha256"

    printf 'Downloading %s\n' "${download_url}"
    download_or_fail "binary" "${download_url}" "${binary_path}"
    download_or_fail "checksum" "${checksum_url}" "${checksum_path}"
    verify_downloaded_binary "${binary_path}" "${checksum_path}" "${asset}"
    chmod 0755 "${binary_path}"
    install_binary "${binary_path}"

    printf 'Installed aegis to %s/aegis\n' "${BINDIR}"

    if "$(target_path)" --version >/dev/null 2>&1; then
        printf 'Installed version: %s\n' "$("$(target_path)" --version)"
    fi

    if [ "${SKIP_SHELL_SETUP}" = "1" ]; then
        printf 'Skipped shell setup because AEGIS_SKIP_SHELL_SETUP=1\n'
        return
    fi

    real_shell="$(detect_real_shell)"
    mode="$(prompt_setup_mode)"

    case "${mode}" in
        global)
            rc_file="$(resolve_rc_file "${real_shell}")"
            write_shell_setup "${rc_file}" "${real_shell}" "$(target_path)"
            print_post_install "${rc_file}"
            offer_agent_setup
            ;;
        local)
            setup_local_project "${real_shell}" "$(target_path)"
            print_local_post_install
            offer_agent_setup
            enter_local_shell "${real_shell}" "$(target_path)"
            ;;
        binary)
            printf 'Binary installed. Shell setup skipped.\n'
            printf 'Set SHELL=%s and AEGIS_REAL_SHELL=%s manually to activate.\n' \
                "$(target_path)" "${real_shell}"
            offer_agent_setup
            ;;
    esac
}

main "$@"
