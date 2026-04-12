#!/bin/sh
set -eu

BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
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

detect_real_shell() {
    aegis_path="$(target_path)"

    if [ -n "${AEGIS_REAL_SHELL:-}" ]; then
        real_shell="${AEGIS_REAL_SHELL}"
    elif [ -n "${SHELL:-}" ] && [ "${SHELL}" != "${aegis_path}" ]; then
        real_shell="${SHELL}"
    else
        fail "cannot determine which shell rc file to clean up; set AEGIS_REAL_SHELL or AEGIS_SHELL_RC and rerun"
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
            fail "automatic shell cleanup supports bash and zsh; set AEGIS_SHELL_RC for ${shell_name}"
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

remove_shell_setup() {
    rc_file="$1"
    tmp_rc="${TMPDIR_AEGIS}/rc.tmp"

    mkdir -p "$(dirname "${rc_file}")"
    remove_managed_block "${rc_file}" "${tmp_rc}"
    cp "${tmp_rc}" "${rc_file}"
}

remove_binary() {
    install_target="$(target_path)"

    if [ ! -e "${install_target}" ]; then
        return
    fi

    if [ -w "${install_target}" ] || [ -w "${BINDIR}" ]; then
        rm -f "${install_target}"
        return
    fi

    if need_cmd sudo; then
        sudo rm -f "${install_target}"
        return
    fi

    fail "cannot remove ${install_target}; rerun as root or install sudo"
}

main() {
    TMPDIR_AEGIS="$(mktemp -d)"

    real_shell="$(detect_real_shell)"
    rc_file="$(resolve_rc_file "${real_shell}")"
    remove_shell_setup "${rc_file}"
    remove_binary

    printf 'Removed shell wrapper setup from %s\n' "${rc_file}"
    printf 'Removed %s\n' "$(target_path)"
}

main "$@"
