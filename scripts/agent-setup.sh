#!/bin/sh
# aegis agent-setup — installs aegis hooks for AI agents.
# Idempotent: safe to run multiple times.
# Usage: sh scripts/agent-setup.sh [--claude-code] [--codex] [--all]
#        (default: auto-detect installed agents)

set -eu

SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd)"
HOOKS_DIR="${SCRIPT_DIR}/hooks"

INSTALL_CLAUDE_CODE=""
INSTALL_CODEX=""

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

need_cmd() {
    command -v "$1" >/dev/null 2>&1
}

need_file() {
    [ -f "$1" ] || fail "missing required hook source: $1"
}

require_shell_safe_path() {
    case "$1" in
        '')
            fail "unsafe hook command path (empty): $2"
            ;;
        *[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_./-]*)
            fail "unsafe hook command path (contains shell-unsafe characters): $2"
            ;;
    esac
}

make_atomic_tmp() {
    target="$1"
    target_dir="$(dirname "$target")"
    target_base="$(basename "$target")"
    mktemp "${target_dir}/.${target_base}.tmp.XXXXXX"
}

parse_args() {
    if [ $# -eq 0 ]; then
        [ -d "${HOME}/.claude" ] && INSTALL_CLAUDE_CODE=1
        [ -d "${HOME}/.codex" ] && INSTALL_CODEX=1
        return
    fi

    for arg in "$@"; do
        case "$arg" in
            --claude-code) INSTALL_CLAUDE_CODE=1 ;;
            --codex) INSTALL_CODEX=1 ;;
            --all)
                INSTALL_CLAUDE_CODE=1
                INSTALL_CODEX=1
                ;;
            *) fail "unknown option: $arg" ;;
        esac
    done
}

install_claude_code() {
    need_cmd jq || fail "jq is required for Claude Code setup"

    CLAUDE_HOOKS_DIR="${HOME}/.claude/hooks"
    CLAUDE_SETTINGS="${HOME}/.claude/settings.json"
    HOOK_DEST="${CLAUDE_HOOKS_DIR}/aegis-rewrite.sh"
    HOOK_SOURCE="${HOOKS_DIR}/claude-code.sh"

    mkdir -p "${CLAUDE_HOOKS_DIR}"
    need_file "${HOOK_SOURCE}"
    require_shell_safe_path "${HOOK_DEST}" "${HOOK_DEST}"
    install -m 0755 "${HOOK_SOURCE}" "${HOOK_DEST}"

    [ -f "${CLAUDE_SETTINGS}" ] || printf '{}\n' > "${CLAUDE_SETTINGS}"

    ALREADY="$(jq -e --arg cmd "${HOOK_DEST}" \
        '[.hooks.PreToolUse[]? | select(.matcher == "Bash") | .hooks[]? | select(.type == "command" and .command == $cmd)] | length > 0' \
        "${CLAUDE_SETTINGS}" 2>/dev/null || printf 'false')"

    if [ "${ALREADY}" = "true" ]; then
        printf 'Claude Code: hook already installed, skipping.\n'
        return
    fi

    PATCH_FILE="$(make_atomic_tmp "${CLAUDE_SETTINGS}")"
    UPDATED_FILE="$(make_atomic_tmp "${CLAUDE_SETTINGS}")"

    jq -n --arg cmd "${HOOK_DEST}" '{
        hooks: {
            PreToolUse: [
                {
                    matcher: "Bash",
                    hooks: [
                        {
                            type: "command",
                            command: $cmd
                        }
                    ]
                }
            ]
        }
    }' > "${PATCH_FILE}"

    jq -s '
        .[0] as $existing
        | .[1] as $patch
        | $existing * {
            hooks: (($existing.hooks // {}) * {
                PreToolUse: (($existing.hooks.PreToolUse // []) + $patch.hooks.PreToolUse)
            })
        }
    ' "${CLAUDE_SETTINGS}" "${PATCH_FILE}" > "${UPDATED_FILE}"

    mv "${UPDATED_FILE}" "${CLAUDE_SETTINGS}"
    rm -f "${PATCH_FILE}"
    printf 'Claude Code: hook installed → %s\n' "${HOOK_DEST}"
}

install_codex() {
    need_cmd jq || fail "jq is required for Codex setup"

    CODEX_HOOKS_DIR="${HOME}/.codex/hooks"
    CODEX_HOOKS_JSON="${HOME}/.codex/hooks.json"
    SESSION_DEST="${CODEX_HOOKS_DIR}/aegis-session-start.sh"
    PTU_DEST="${CODEX_HOOKS_DIR}/aegis-pre-tool-use.sh"
    SESSION_SOURCE="${HOOKS_DIR}/codex-session-start.sh"
    PTU_SOURCE="${HOOKS_DIR}/codex-pre-tool-use.sh"

    mkdir -p "${CODEX_HOOKS_DIR}"
    need_file "${SESSION_SOURCE}"
    need_file "${PTU_SOURCE}"
    require_shell_safe_path "${SESSION_DEST}" "${SESSION_DEST}"
    require_shell_safe_path "${PTU_DEST}" "${PTU_DEST}"
    install -m 0755 "${SESSION_SOURCE}" "${SESSION_DEST}"
    install -m 0755 "${PTU_SOURCE}" "${PTU_DEST}"

    SESSION_EXISTS="false"
    PTU_EXISTS="false"

    if [ -f "${CODEX_HOOKS_JSON}" ]; then
        SESSION_EXISTS="$(jq -e --arg cmd "${SESSION_DEST}" \
            '[.hooks.SessionStart[]? | select(.matcher == "startup|resume") | .hooks[]? | select(.type == "command" and .command == $cmd)] | length > 0' \
            "${CODEX_HOOKS_JSON}" 2>/dev/null || printf 'false')"
        PTU_EXISTS="$(jq -e --arg cmd "${PTU_DEST}" \
            '[.hooks.PreToolUse[]? | select(.matcher == "Bash") | .hooks[]? | select(.type == "command" and .command == $cmd)] | length > 0' \
            "${CODEX_HOOKS_JSON}" 2>/dev/null || printf 'false')"
    fi

    if [ "${SESSION_EXISTS}" = "true" ] && [ "${PTU_EXISTS}" = "true" ]; then
        printf 'Codex: hooks already installed, skipping.\n'
        return
    fi

    PATCH_FILE="$(make_atomic_tmp "${CODEX_HOOKS_JSON}")"

    jq -n \
        --arg ss "${SESSION_DEST}" \
        --arg ptu "${PTU_DEST}" \
        --arg session_exists "${SESSION_EXISTS}" \
        --arg ptu_exists "${PTU_EXISTS}" '
        {
            hooks: {
                SessionStart: (
                    if $session_exists == "true" then
                        []
                    else
                        [
                            {
                                matcher: "startup|resume",
                                hooks: [
                                    {
                                        type: "command",
                                        command: $ss
                                    }
                                ]
                            }
                        ]
                    end
                ),
                PreToolUse: (
                    if $ptu_exists == "true" then
                        []
                    else
                        [
                            {
                                matcher: "Bash",
                                hooks: [
                                    {
                                        type: "command",
                                        command: $ptu
                                    }
                                ]
                            }
                        ]
                    end
                )
            }
        }
    ' > "${PATCH_FILE}"

    if [ -f "${CODEX_HOOKS_JSON}" ]; then
        UPDATED_FILE="$(make_atomic_tmp "${CODEX_HOOKS_JSON}")"
        jq -s '
            .[0] as $existing
            | .[1] as $patch
            | $existing * {
                hooks: (($existing.hooks // {}) * {
                    SessionStart: (($existing.hooks.SessionStart // []) + $patch.hooks.SessionStart),
                    PreToolUse: (($existing.hooks.PreToolUse // []) + $patch.hooks.PreToolUse)
                })
            }
        ' "${CODEX_HOOKS_JSON}" "${PATCH_FILE}" > "${UPDATED_FILE}"
        mv "${UPDATED_FILE}" "${CODEX_HOOKS_JSON}"
        rm -f "${PATCH_FILE}"
    else
        mv "${PATCH_FILE}" "${CODEX_HOOKS_JSON}"
    fi

    printf 'Codex: hooks installed → %s\n' "${CODEX_HOOKS_DIR}"
}

main() {
    parse_args "$@"

    if [ -z "${INSTALL_CLAUDE_CODE}" ] && [ -z "${INSTALL_CODEX}" ]; then
        printf 'No agents detected (no ~/.claude or ~/.codex). Nothing installed.\n'
        printf 'Force with: --claude-code, --codex, or --all\n'
        exit 0
    fi

    [ -n "${INSTALL_CLAUDE_CODE}" ] && install_claude_code
    [ -n "${INSTALL_CODEX}" ] && install_codex

    printf '\nDone. Open a new agent session to activate.\n'
    printf 'To uninstall: sh scripts/agent-uninstall.sh\n'
}

main "$@"
