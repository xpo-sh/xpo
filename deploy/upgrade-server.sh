#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-latest}"
ARCH="x86_64-unknown-linux-gnu"
BINARY="/usr/local/bin/xpo-server"
REPO="xpo-sh/xpo"

if [ "$VERSION" = "latest" ]; then
    URL="https://github.com/${REPO}/releases/latest/download/xpo-server-${ARCH}.tar.gz"
else
    URL="https://github.com/${REPO}/releases/download/${VERSION}/xpo-server-${ARCH}.tar.gz"
fi

echo "=== xpo-server upgrade ==="
echo "Version: ${VERSION}"
echo "URL: ${URL}"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "Downloading..."
curl -fsSL "$URL" | tar xz -C "$TMPDIR"

echo "Backing up current binary..."
[ -f "$BINARY" ] && cp "$BINARY" "${BINARY}.bak"

echo "Stopping xpo-server..."
systemctl stop xpo-server

echo "Installing binary..."
cp "$TMPDIR/xpo-server" "$BINARY"
chmod 755 "$BINARY"

echo "Starting xpo-server..."
if ! systemctl start xpo-server; then
    echo "Start failed! Rolling back..."
    cp "${BINARY}.bak" "$BINARY"
    systemctl start xpo-server
    echo "Rolled back to previous version"
    exit 1
fi

echo "=== Deployed ${VERSION} ==="
systemctl status xpo-server --no-pager -l
