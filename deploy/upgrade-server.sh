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

echo "Stopping xpo-server..."
systemctl stop xpo-server

echo "Installing binary..."
cp "$TMPDIR/xpo-server" "$BINARY"
chmod 755 "$BINARY"

echo "Starting xpo-server..."
systemctl start xpo-server

echo "=== Done ==="
"$BINARY" --version 2>/dev/null || echo "Deployed successfully"
systemctl status xpo-server --no-pager -l
