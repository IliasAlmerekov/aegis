#!/bin/sh
# aegis agent-setup — compatibility wrapper for `aegis install-hooks`.
# Idempotent: safe to run multiple times.
# Usage: sh scripts/agent-setup.sh [--claude-code] [--codex] [--all] [--local]
#        (default: auto-detect installed agents)

set -eu

BINDIR="${AEGIS_BINDIR:-/usr/local/bin}"
AEGIS_BIN_OVERRIDE="${AEGIS_BIN:-}"

fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

resolve_aegis_bin() {
    install_target="${BINDIR}/aegis"

    if [ -n "${AEGIS_BIN_OVERRIDE}" ]; then
        printf '%s\n' "${AEGIS_BIN_OVERRIDE}"
        return
    fi

    if [ -x "${install_target}" ]; then
        printf '%s\n' "${install_target}"
        return
    fi

    if command -v aegis >/dev/null 2>&1; then
        command -v aegis
        return
    fi

    fail "could not find aegis; install it first or set AEGIS_BIN to the aegis binary path"
}

main() {
    aegis_bin="$(resolve_aegis_bin)"
    exec "${aegis_bin}" install-hooks "$@"
}

main "$@"
