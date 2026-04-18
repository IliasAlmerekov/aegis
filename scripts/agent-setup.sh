#!/bin/sh
set -eu

CLAUDE_HOME="${HOME}/.claude"
CODEX_HOME="${HOME}/.codex"
SCRIPT_DIR="$(CDPATH= cd "$(dirname "$0")" && pwd)"
CLAUDE_HOOK_SRC="${SCRIPT_DIR}/hooks/claude-code.sh"
CODEX_SESSION_SRC="${SCRIPT_DIR}/hooks/codex-session-start.sh"
CODEX_PRE_TOOL_SRC="${SCRIPT_DIR}/hooks/codex-pre-tool-use.sh"
CLAUDE_HOOK_DEST="${CLAUDE_HOME}/hooks/aegis-rewrite.sh"
CLAUDE_SETTINGS="${CLAUDE_HOME}/settings.json"
CODEX_HOOK_DIR="${CODEX_HOME}/hooks"
CODEX_SESSION_DEST="${CODEX_HOOK_DIR}/aegis-session-start.sh"
CODEX_PRE_TOOL_DEST="${CODEX_HOOK_DIR}/aegis-pre-tool-use.sh"
CODEX_HOOKS_JSON="${CODEX_HOME}/hooks.json"

TMPDIR_AEGIS=""
RUN_CLAUDE=0
RUN_CODEX=0

cleanup() {
    if [ -n "${TMPDIR_AEGIS}" ] && [ -d "${TMPDIR_AEGIS}" ]; then
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

usage() {
    cat <<'EOF'
Usage: sh scripts/agent-setup.sh [--claude-code] [--codex] [--all]

If no agent flags are provided, the installer auto-detects ~/.claude and ~/.codex.
EOF
}

parse_args() {
    if [ "$#" -eq 0 ]; then
        return
    fi

    while [ "$#" -gt 0 ]; do
        case "$1" in
            --claude-code)
                RUN_CLAUDE=1
                ;;
            --codex)
                RUN_CODEX=1
                ;;
            --all)
                RUN_CLAUDE=1
                RUN_CODEX=1
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                fail "unknown option: $1"
                ;;
        esac
        shift
    done
}

ensure_tmpdir() {
    if [ -z "${TMPDIR_AEGIS}" ]; then
        TMPDIR_AEGIS="$(mktemp -d)"
    fi
}

copy_hook() {
    src="$1"
    dest="$2"
    dest_dir="$(dirname "$dest")"

    [ -f "$src" ] || fail "missing source hook: $src"

    mkdir -p "$dest_dir"
    cp "$src" "$dest"
    chmod 0755 "$dest"
}

claude_hook_installed() {
    if [ ! -f "${CLAUDE_SETTINGS}" ]; then
        return 1
    fi

    jq -e --arg cmd "${CLAUDE_HOOK_DEST}" '
        (.hooks.PreToolUse // [])
        | any(.matcher == "Bash" and any(.hooks[]?; .command == $cmd))
    ' "${CLAUDE_SETTINGS}" >/dev/null 2>&1
}

install_claude_code() {
    need_cmd jq || fail "jq is required for Claude Code installation"
    ensure_tmpdir

    mkdir -p "${CLAUDE_HOME}/hooks"
    copy_hook "${CLAUDE_HOOK_SRC}" "${CLAUDE_HOOK_DEST}"

    if [ ! -f "${CLAUDE_SETTINGS}" ]; then
        printf '{}\n' > "${CLAUDE_SETTINGS}"
    fi

    if claude_hook_installed; then
        printf '[aegis] Claude Code hook already installed; skipping.\n'
        return 0
    fi

    patched_settings="${TMPDIR_AEGIS}/claude-settings.json"
    jq --arg cmd "${CLAUDE_HOOK_DEST}" '
        .hooks = (.hooks // {})
        | .hooks.PreToolUse = ((.hooks.PreToolUse // []) + [
            {
                "matcher": "Bash",
                "hooks": [
                    {
                        "type": "command",
                        "command": $cmd
                    }
                ]
            }
        ])
    ' "${CLAUDE_SETTINGS}" > "${patched_settings}"
    mv "${patched_settings}" "${CLAUDE_SETTINGS}"

    printf '[aegis] Claude Code hook installed.\n'
}

codex_session_hook_installed() {
    jq -e --arg cmd "${CODEX_SESSION_DEST}" '
        (.hooks.SessionStart // [])
        | any(.[]?; any(.hooks[]?; .command == $cmd))
    ' "${CODEX_HOOKS_JSON}" >/dev/null 2>&1
}

codex_pre_tool_hook_installed() {
    jq -e --arg cmd "${CODEX_PRE_TOOL_DEST}" '
        (.hooks.PreToolUse // [])
        | any(.[]?; any(.hooks[]?; .command == $cmd))
    ' "${CODEX_HOOKS_JSON}" >/dev/null 2>&1
}

write_codex_desired_json() {
    desired_path="$1"

    jq -n \
        --arg session "${CODEX_SESSION_DEST}" \
        --arg pre_tool "${CODEX_PRE_TOOL_DEST}" '
        {
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "startup|resume",
                        "hooks": [
                            {
                                "type": "command",
                                "command": $session
                            }
                        ]
                    }
                ],
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            {
                                "type": "command",
                                "command": $pre_tool
                            }
                        ]
                    }
                ]
            }
        }
    ' > "${desired_path}"
}

merge_codex_hooks() {
    existing_path="$1"
    desired_path="$2"
    merged_path="$3"

    jq -s --arg session "${CODEX_SESSION_DEST}" --arg pre_tool "${CODEX_PRE_TOOL_DEST}" '
        def entry_command($entry):
            $entry.hooks[0].command;

        def contains_entry($entries; $entry):
            any(
                $entries[]?;
                .matcher == $entry.matcher
                and any(.hooks[]?; .command == entry_command($entry))
            );

        def merge_entries($existing; $incoming):
            reduce $incoming[] as $entry ($existing;
                if contains_entry(.; $entry) then .
                else . + [$entry]
                end
            );

        .[0] as $current
        | .[1] as $desired
        | $current
        | .hooks = (.hooks // {})
        | .hooks.SessionStart = merge_entries((.hooks.SessionStart // []); $desired.hooks.SessionStart)
        | .hooks.PreToolUse = merge_entries((.hooks.PreToolUse // []); $desired.hooks.PreToolUse)
    ' "${existing_path}" "${desired_path}" > "${merged_path}"
}

install_codex() {
    need_cmd jq || fail "jq is required for Codex installation"
    ensure_tmpdir

    mkdir -p "${CODEX_HOOK_DIR}"
    copy_hook "${CODEX_SESSION_SRC}" "${CODEX_SESSION_DEST}"
    copy_hook "${CODEX_PRE_TOOL_SRC}" "${CODEX_PRE_TOOL_DEST}"

    desired_json="${TMPDIR_AEGIS}/codex-hooks.desired.json"
    write_codex_desired_json "${desired_json}"

    if [ ! -f "${CODEX_HOOKS_JSON}" ]; then
        mv "${desired_json}" "${CODEX_HOOKS_JSON}"
        printf '[aegis] Codex hooks installed.\n'
        return 0
    fi

    if codex_session_hook_installed && codex_pre_tool_hook_installed; then
        printf '[aegis] Codex hooks already installed; skipping.\n'
        return 0
    fi

    merged_json="${TMPDIR_AEGIS}/codex-hooks.merged.json"
    merge_codex_hooks "${CODEX_HOOKS_JSON}" "${desired_json}" "${merged_json}"
    mv "${merged_json}" "${CODEX_HOOKS_JSON}"

    printf '[aegis] Codex hooks installed.\n'
}

detect_agents() {
    if [ -d "${CLAUDE_HOME}" ]; then
        RUN_CLAUDE=1
    fi

    if [ -d "${CODEX_HOME}" ]; then
        RUN_CODEX=1
    fi
}

main() {
    parse_args "$@"

    if [ "${RUN_CLAUDE}" -eq 0 ] && [ "${RUN_CODEX}" -eq 0 ]; then
        detect_agents
    fi

    if [ "${RUN_CLAUDE}" -eq 0 ] && [ "${RUN_CODEX}" -eq 0 ]; then
        printf 'No agents detected in ~/.claude or ~/.codex. Force with --claude-code, --codex, or --all.\n'
        exit 0
    fi

    if [ "${RUN_CLAUDE}" -eq 1 ]; then
        install_claude_code
    fi

    if [ "${RUN_CODEX}" -eq 1 ]; then
        install_codex
    fi

    printf 'Done. Open a new agent session to activate.\n'
    printf 'To uninstall: sh scripts/agent-uninstall.sh\n'
}

main "$@"
