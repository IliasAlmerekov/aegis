#!/usr/bin/env sh

aegis_truthy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    1|true|yes) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_falsy() {
  case "$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')" in
    0|false|no) return 0 ;;
    *) return 1 ;;
  esac
}

aegis_ci_active() {
  if [ -n "${AEGIS_CI:-}" ]; then
    aegis_falsy "${AEGIS_CI}" && return 1
    aegis_truthy "${AEGIS_CI}" && return 0
  fi

  for key in CI GITHUB_ACTIONS GITLAB_CI CIRCLECI BUILDKITE TRAVIS TF_BUILD; do
    value="$(printenv "$key" 2>/dev/null || true)"
    if [ -n "${value}" ] && aegis_truthy "${value}"; then
      return 0
    fi
  done

  [ -n "${JENKINS_URL:-}" ]
}

aegis_disabled_locally() {
  [ -f "${HOME}/.aegis/disabled" ]
}

aegis_enforcement_enabled() {
  if aegis_ci_active; then
    return 0
  fi

  if aegis_disabled_locally; then
    return 1
  fi

  return 0
}
