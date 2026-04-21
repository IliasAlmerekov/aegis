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

    if [ ! -f "${rc_file}" ]; then
        return
    fi

    remove_managed_block "${rc_file}" "${tmp_rc}"
    cp "${tmp_rc}" "${rc_file}"
}

remove_binary() {
    install_target="$(target_path)"
    install_dir="$(dirname "${install_target}")"

    if [ ! -e "${install_target}" ]; then
        return
    fi

    if [ -w "${install_dir}" ]; then
        rm -f "${install_target}"
        return
    fi

    if need_cmd sudo; then
        sudo rm -f "${install_target}"
        return
    fi

    fail "cannot remove ${install_target}; rerun as root or install sudo"
}

remove_hook_payload() {
    hook_path="$1"
    hook_dir="$(dirname "${hook_path}")"

    if [ -e "${hook_path}" ] && ! [ -w "${hook_dir}" ]; then
        if need_cmd sudo; then
            sudo rm -f "${hook_path}"
            return
        fi

        fail "cannot remove ${hook_path}; rerun as root or install sudo"
    fi

    rm -f "${hook_path}"
}

prune_hook_registration() {
    json_file="$1"
    section="$2"
    command_path="$3"

    [ -f "${json_file}" ] || return 0

    jq --arg section "${section}" --arg cmd "${command_path}" '
        if .hooks[$section]? then
            .hooks[$section] = [
                .hooks[$section][]?
                | .hooks = [
                    .hooks[]?
                    | select(.type != "command" or .command != $cmd)
                  ]
                | select((.hooks | length) > 0)
            ]
        else
            .
        end
    ' "${json_file}" > "${TMPDIR_AEGIS}/hook-prune.tmp"

    mv "${TMPDIR_AEGIS}/hook-prune.tmp" "${json_file}"
}

main() {
    TMPDIR_AEGIS="$(mktemp -d)"

    if [ -f "${HOME}/.claude/settings.json" ] || [ -f "${HOME}/.codex/hooks.json" ]; then
        need_cmd jq || fail "jq is required to prune agent hook registrations"
    fi

    if [ -n "${SHELL_RC_OVERRIDE}" ]; then
        rc_file="$(resolve_rc_file "")"
    else
        real_shell="$(detect_real_shell)"
        rc_file="$(resolve_rc_file "${real_shell}")"
    fi
    remove_shell_setup "${rc_file}"
    remove_binary
    remove_hook_payload "${HOME}/.claude/hooks/aegis-rewrite.sh"
    remove_hook_payload "${HOME}/.codex/hooks/aegis-session-start.sh"
    remove_hook_payload "${HOME}/.codex/hooks/aegis-pre-tool-use.sh"
    remove_hook_payload "${HOME}/.aegis/lib/toggle-state.sh"
    prune_hook_registration "${HOME}/.claude/settings.json" "PreToolUse" "${HOME}/.claude/hooks/aegis-rewrite.sh"
    prune_hook_registration "${HOME}/.codex/hooks.json" "SessionStart" "${HOME}/.codex/hooks/aegis-session-start.sh"
    prune_hook_registration "${HOME}/.codex/hooks.json" "PreToolUse" "${HOME}/.codex/hooks/aegis-pre-tool-use.sh"

    printf 'Removed shell wrapper setup from %s\n' "${rc_file}"
    printf 'Removed %s\n' "$(target_path)"
}

main "$@"
