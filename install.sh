#!/bin/sh
# Install the kap binary from a GitHub release. No Rust toolchain needed.
#
#   curl -fsSL https://raw.githubusercontent.com/Manifestro/kap/main/install.sh | sh
#
# KAP_VERSION picks a release (default: the latest), KAP_INSTALL_DIR where the binary goes
# (default: ~/.local/bin).
set -eu

REPO="Manifestro/kap"
VERSION="${KAP_VERSION:-latest}"
INSTALL_DIR="${KAP_INSTALL_DIR:-$HOME/.local/bin}"

fail() {
  printf 'kap install: %s\n' "$1" >&2
  exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

os=$(uname -s)
arch=$(uname -m)
case "$os/$arch" in
  Darwin/arm64 | Darwin/aarch64) target="aarch64-apple-darwin" ;;
  Darwin/x86_64) target="x86_64-apple-darwin" ;;
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-gnu" ;;
  *) fail "no prebuilt binary for $os/$arch; build from source with \`cargo install kap-cli\`" ;;
esac

case "$VERSION" in
  latest) base="https://github.com/$REPO/releases/latest/download" ;;
  v*) base="https://github.com/$REPO/releases/download/$VERSION" ;;
  *) base="https://github.com/$REPO/releases/download/v$VERSION" ;;
esac
archive="kap-$target.tar.gz"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'Downloading kap (%s, %s)...\n' "$VERSION" "$target"
curl -fsSL "$base/$archive" -o "$tmp/$archive" || fail "could not download $base/$archive"
curl -fsSL "$base/$archive.sha256" -o "$tmp/$archive.sha256" || fail "could not download the checksum"

expected=$(cut -d ' ' -f 1 < "$tmp/$archive.sha256")
if command -v shasum >/dev/null 2>&1; then
  actual=$(shasum -a 256 "$tmp/$archive" | cut -d ' ' -f 1)
else
  actual=$(sha256sum "$tmp/$archive" | cut -d ' ' -f 1)
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch: the download is not the released archive"

mkdir -p "$tmp/x" "$INSTALL_DIR"
tar -xzf "$tmp/$archive" -C "$tmp/x"
[ -f "$tmp/x/kap" ] || fail "the archive holds no kap binary"
# Replace through a temporary name, so a running hook never sees a half-written binary.
cp "$tmp/x/kap" "$INSTALL_DIR/.kap.new"
chmod 755 "$INSTALL_DIR/.kap.new"
mv -f "$INSTALL_DIR/.kap.new" "$INSTALL_DIR/kap"

printf 'Installed %s to %s/kap\n' "$("$INSTALL_DIR/kap" --version)" "$INSTALL_DIR"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) printf 'Add %s to your PATH to run it as `kap`.\n' "$INSTALL_DIR" ;;
esac
printf 'Next: `kap install` records Claude Code sessions; `kap init` in a repository starts using it.\n'
