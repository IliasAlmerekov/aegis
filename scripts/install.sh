#!/bin/sh
set -eu

REPO="${AEGIS_REPO:-IliasAlmerekov/aegis}"
VERSION="${AEGIS_VERSION:-latest}"
BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
OS_OVERRIDE="${AEGIS_OS:-}"
ARCH_OVERRIDE="${AEGIS_ARCH:-}"

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
    target_path="${BINDIR}/aegis"

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
        install -m 0755 "${source_path}" "${target_path}"
        return
    fi

    if need_cmd sudo; then
        sudo install -m 0755 "${source_path}" "${target_path}"
        return
    fi

    fail "cannot write to ${BINDIR}; rerun as root or install sudo"
}

print_post_install() {
    cat <<'EOF'

Post-install setup:

bash:
  echo 'export SHELL=$(which aegis)' >> ~/.bashrc
  source ~/.bashrc

zsh:
  echo 'export SHELL=$(which aegis)' >> ~/.zshrc
  source ~/.zshrc

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

    printf 'Installed aegis to %s/aegis\n' "${BINDIR}"

    if "${BINDIR}/aegis" --version >/dev/null 2>&1; then
        printf 'Installed version: %s\n' "$("${BINDIR}/aegis" --version)"
    fi

    print_post_install
}

main "$@"
