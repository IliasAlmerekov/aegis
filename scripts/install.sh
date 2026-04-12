#!/bin/sh
set -eu

REPO="${AEGIS_REPO:-IliasAlmerekov/aegis}"
VERSION="${AEGIS_VERSION:-latest}"
BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
OS_OVERRIDE="${AEGIS_OS:-}"
ARCH_OVERRIDE="${AEGIS_ARCH:-}"
SHELL_RC_OVERRIDE="${AEGIS_SHELL_RC:-}"
SKIP_SHELL_SETUP="${AEGIS_SKIP_SHELL_SETUP:-0}"

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
        curl --fail --location --silent --show-error "$url" --output "$output"
        return
    fi

    if need_cmd wget; then
        wget --quiet "$url" --output-document="$output"
        return
    fi

    fail "curl or wget is required to download ${url}"
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

main() {
    os=""
    arch=""
    asset=""
    base_url=""
    download_url=""
    binary_path=""
    rc_file=""

    os="$(detect_os)"
    arch="$(detect_arch)"
    asset="aegis-${os}-${arch}"
    base_url="$(resolve_base_url)"
    download_url="${base_url}/${asset}"

    TMPDIR_AEGIS="$(mktemp -d)"
    binary_path="${TMPDIR_AEGIS}/aegis"

    printf 'Downloading %s\n' "${download_url}"
    download "${download_url}" "${binary_path}"
    chmod 0755 "${binary_path}"
    install_binary "${binary_path}"

    if [ "${SKIP_SHELL_SETUP}" != "1" ]; then
        real_shell="$(detect_real_shell)"
        rc_file="$(resolve_rc_file "${real_shell}")"
        write_shell_setup "${rc_file}" "${real_shell}" "$(target_path)"
    fi

    printf 'Installed aegis to %s/aegis\n' "${BINDIR}"

    if "$(target_path)" --version >/dev/null 2>&1; then
        printf 'Installed version: %s\n' "$("$(target_path)" --version)"
    fi

    if [ -n "${rc_file}" ]; then
        print_post_install "${rc_file}"
    else
        printf 'Skipped shell wrapper setup because AEGIS_SKIP_SHELL_SETUP=1\n'
    fi
}

main "$@"
