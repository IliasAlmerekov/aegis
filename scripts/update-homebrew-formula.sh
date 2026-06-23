#!/bin/sh
set -eu

# Regenerates packaging/homebrew/Formula/aegis.rb for a release tag by
# downloading the four <asset>.sha256 sidecars published alongside the
# GitHub Release. Fails closed if any sidecar is missing or its checksum
# is not exactly 64 hex characters. The release assets are raw single-file
# binaries (not archives), so every url is emitted with `using: :nounzip`
# to stop Homebrew from trying to decompress them.

usage() {
  printf 'Usage: %s vX.Y.Z\n' "$0" >&2
}

if [ "$#" -ne 1 ]; then
  usage
  exit 2
fi

tag="$1"
case "$tag" in
  v*) ;;
  *)
    printf 'release tag must start with v: %s\n' "$tag" >&2
    exit 2
    ;;
esac

version="${tag#v}"
repo="${AEGIS_RELEASE_REPO:-IliasAlmerekov/aegis}"
base_url="https://github.com/${repo}/releases/download/${tag}"
out="${AEGIS_HOMEBREW_FORMULA:-packaging/homebrew/Formula/aegis.rb}"
tmp_dir="$(mktemp -d)"

cleanup() {
  rm -rf "$tmp_dir"
}
trap cleanup EXIT INT TERM

fetch_sha() {
  sidecar="$1"
  asset="${sidecar%.sha256}"
  checksum_file="${tmp_dir}/${sidecar}"
  url="${base_url}/${sidecar}"

  curl -fsSL "$url" -o "$checksum_file"
  checksum="$(awk '{print $1}' "$checksum_file")"
  if ! printf '%s\n' "$checksum" | grep -Eq '^[[:xdigit:]]{64}$'; then
    printf 'invalid sha256 for %s from %s\n' "$asset" "$url" >&2
    exit 1
  fi
  printf '%s' "$checksum"
}

linux_x86_64="$(fetch_sha aegis-linux-x86_64.sha256)"
linux_aarch64="$(fetch_sha aegis-linux-aarch64.sha256)"
macos_x86_64="$(fetch_sha aegis-macos-x86_64.sha256)"
macos_aarch64="$(fetch_sha aegis-macos-aarch64.sha256)"

mkdir -p "$(dirname "$out")"

cat > "$out" <<EOF
class Aegis < Formula
  desc "Heuristic shell guardrail for AI agent command execution"
  homepage "https://github.com/IliasAlmerekov/aegis"
  version "${version}"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "${base_url}/aegis-macos-aarch64", using: :nounzip
      sha256 "${macos_aarch64}"
    else
      url "${base_url}/aegis-macos-x86_64", using: :nounzip
      sha256 "${macos_x86_64}"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "${base_url}/aegis-linux-aarch64", using: :nounzip
      sha256 "${linux_aarch64}"
    else
      url "${base_url}/aegis-linux-x86_64", using: :nounzip
      sha256 "${linux_x86_64}"
    end
  end

  def install
    bin.install Dir["aegis-*"].first => "aegis"
  end

  def caveats
    <<~EOS
      Homebrew installs the aegis binary only.

      To install supported Claude Code and Codex hooks after installation:
        aegis install-hooks --all

      To enable shell-proxy mode for tools that launch commands through \$SHELL -c:
        aegis setup-shell

      To undo shell-proxy setup:
        aegis setup-shell --remove

      Native Windows shells are not supported; use Aegis from WSL2 on Windows.
    EOS
  end

  test do
    assert_match "brew-test", shell_output("#{bin}/aegis -c 'echo brew-test'")
  end
end
EOF

printf '%s updated\n' "$out"