#!/usr/bin/env sh
# scripts/install.sh — Yggdrazil installer
set -e

REPO="rzorzal/yggdrazil"
BIN_NAME="ygg"
INSTALL_DIR="/usr/local/bin"

# Detect OS and arch
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
      aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="x86_64-apple-darwin" ;;
      arm64)   TARGET="aarch64-apple-darwin" ;;
      *) echo "Unsupported architecture: $ARCH" && exit 1 ;;
    esac
    EXT="tar.gz"
    ;;
  *)
    echo "Unsupported OS: $OS. Use the Windows installer from GitHub Releases."
    exit 1
    ;;
esac

# Get latest release tag from GitHub API
LATEST=$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Could not determine latest release. Check https://github.com/${REPO}/releases"
  exit 1
fi

FILENAME="${BIN_NAME}-${LATEST}-${TARGET}.${EXT}"
URL="https://github.com/${REPO}/releases/download/${LATEST}/${FILENAME}"

echo "Installing ygg ${LATEST} for ${TARGET}..."

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

curl -sSfL "$URL" -o "$TMP/$FILENAME"
tar xzf "$TMP/$FILENAME" -C "$TMP"

install -m 755 "$TMP/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"

echo "✓ ygg ${LATEST} installed to ${INSTALL_DIR}/${BIN_NAME}"
echo "  Run: ygg init"
