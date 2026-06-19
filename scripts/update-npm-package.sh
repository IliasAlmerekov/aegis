#!/bin/sh
set -eu

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
out="${AEGIS_NPM_CHECKSUMS:-packaging/npm/checksums.json}"
package_json="${AEGIS_NPM_PACKAGE_JSON:-packaging/npm/package.json}"
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
{
  "release": "${tag}",
  "repo": "${repo}",
  "assets": {
    "aegis-linux-x86_64": "${linux_x86_64}",
    "aegis-linux-aarch64": "${linux_aarch64}",
    "aegis-macos-x86_64": "${macos_x86_64}",
    "aegis-macos-aarch64": "${macos_aarch64}"
  }
}
EOF

if [ -f "$package_json" ]; then
  tmp_package="${tmp_dir}/package.json"
  awk -v version="$version" '
    /^  "version": / { print "  \"version\": \"" version "\","; next }
    { print }
  ' "$package_json" > "$tmp_package"
  mv "$tmp_package" "$package_json"
fi

printf '%s updated\n' "$out"