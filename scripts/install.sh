#!/bin/sh
set -eu

REPO="${AEGIS_REPO:-IliasAlmerekov/aegis}"
VERSION="${AEGIS_VERSION:-latest}"
BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
OS_OVERRIDE="${AEGIS_OS:-}"
ARCH_OVERRIDE="${AEGIS_ARCH:-}"
SHELL_RC_OVERRIDE="${AEGIS_SHELL_RC:-}"

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

validate_shell_path() {
    case "$1" in
        *[!A-Za-z0-9_./+-]*)
            fail "invalid real shell path: contains unsafe characters"
            ;;
    esac
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
    mv "${tmp_rc}" "${rc_file}"

    cat >> "${rc_file}" <<EOF
${BEGIN_MARKER}
export AEGIS_REAL_SHELL="${real_shell}"
export SHELL="${aegis_path}"
${END_MARKER}
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

    validate_shell_path "${real_shell}"

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

has_supported_agent_dirs() {
    if [ -d "${HOME}/.claude" ] || [ -d "${HOME}/.codex" ]; then
        return 0
    fi

    return 1
}

offer_agent_setup() {
    agent_setup_script=""
    agent_setup_output=""

    if agent_setup_script="$(resolve_local_agent_setup)"; then
        if ! has_supported_agent_dirs; then
            printf 'Agent hook setup skipped; no supported agent directories were detected.\n'
            return 0
        fi

        if agent_setup_output="$(/bin/sh "${agent_setup_script}" 2>&1)"; then
            if [ -n "${agent_setup_output}" ]; then
                printf '%s\n' "${agent_setup_output}"
            fi

            case "${agent_setup_output}" in
                *"No agents detected (no ~/.claude or ~/.codex). Nothing installed."*)
                    printf 'Agent hook setup skipped; no supported agent directories were detected.\n'
                    ;;
                *)
                    printf 'Agent hooks installed automatically.\n'
                    ;;
            esac
        else
            if [ -n "${agent_setup_output}" ]; then
                printf '%s\n' "${agent_setup_output}"
            fi
            printf 'Agent hook setup failed.\n'
            print_agent_setup_next_steps
        fi
    else
        print_agent_setup_next_steps
    fi
}

main() {
    print_banner
    if [ -n "${AEGIS_SETUP_MODE:-}" ] || [ -n "${AEGIS_SKIP_SHELL_SETUP:-}" ]; then
        fail "AEGIS_SETUP_MODE and AEGIS_SKIP_SHELL_SETUP are deprecated; the installer always performs global shell setup"
    fi

    os=""
    arch=""
    asset=""
    base_url=""
    download_url=""
    checksum_url=""
    binary_path=""
    checksum_path=""
    real_shell="$(detect_real_shell)"
    rc_file="$(resolve_rc_file "${real_shell}")"

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

    write_shell_setup "${rc_file}" "${real_shell}" "$(target_path)"
    print_post_install "${rc_file}"
    offer_agent_setup
    printf 'Aegis installed globally.\n'
    printf 'Use `aegis off` to disable temporarily.\n'
    printf 'Use `aegis on` to re-enable enforcement.\n'
}

main "$@"
