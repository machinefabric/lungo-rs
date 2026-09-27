#!/bin/sh
# Installs the `lungo` command from a lungo release, verifying its SHA-256 digest against the
# release's SHA256SUMS.
#
#   curl -sSfL https://github.com/machinefabric/lungo/releases/latest/download/install.sh | sh
#
# LUNGO_VERSION selects a release (default: the latest); LUNGO_INSTALL_DIR the directory the
# command is installed in (default: ~/.local/bin).
set -eu

repo="machinefabric/lungo"
dir="${LUNGO_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) target=x86_64-unknown-linux-musl ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl ;;
  Darwin-x86_64) target=x86_64-apple-darwin ;;
  Darwin-arm64) target=aarch64-apple-darwin ;;
  *) echo "lungo has no release for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

if [ -n "${LUNGO_VERSION:-}" ]; then
  version="$LUNGO_VERSION"
else
  version=$(curl -sSfL -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest" | sed 's|.*/tag/v||')
fi
base="https://github.com/$repo/releases/download/v$version"
archive="lungo-$version-$target.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -sSfL "$base/$archive" -o "$tmp/$archive"
curl -sSfL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"
expected=$(awk -v f="$archive" '$2 == f { print $1 }' "$tmp/SHA256SUMS")
if [ -z "$expected" ]; then
  echo "SHA256SUMS of lungo $version lists no $archive" >&2
  exit 1
fi
if command -v sha256sum > /dev/null; then
  actual=$(sha256sum "$tmp/$archive" | awk '{ print $1 }')
else
  actual=$(shasum -a 256 "$tmp/$archive" | awk '{ print $1 }')
fi
if [ "$actual" != "$expected" ]; then
  echo "$archive has SHA-256 $actual, but the release lists $expected; nothing was installed" >&2
  exit 1
fi
tar -xzf "$tmp/$archive" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/lungo-$version-$target/lungo" "$dir/lungo"
echo "installed lungo $version in $dir"
case ":$PATH:" in *":$dir:"*) ;; *) echo "add $dir to PATH to run lungo" ;; esac
