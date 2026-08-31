#!/usr/bin/env sh
set -e

REPO="nrynss/mooshik"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

detect_os() {
    OS="$(uname -s)"
    case "$OS" in
        Linux*)  echo "unknown-linux-gnu" ;;
        Darwin*) echo "apple-darwin" ;;
        *)       echo "unsupported" ;;
    esac
}

detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64) echo "x86_64" ;;
        arm64|aarch64) echo "aarch64" ;;
        *)            echo "unsupported" ;;
    esac
}

OS_TARGET="$(detect_os)"
ARCH_TARGET="$(detect_arch)"

if [ "$OS_TARGET" = "unsupported" ] || [ "$ARCH_TARGET" = "unsupported" ]; then
    echo "Error: Unsupported operating system or architecture: $(uname -s) $(uname -m)" >&2
    exit 1
fi

TARGET="${ARCH_TARGET}-${OS_TARGET}"

echo "Detecting latest Mooshik release..."
LATEST_TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"

if [ -z "$LATEST_TAG" ]; then
    echo "Error: Failed to find latest release tag." >&2
    exit 1
fi

VERSION="${LATEST_TAG#v}"
ARCHIVE_NAME="mooshik-${VERSION}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/${ARCHIVE_NAME}"
CHECKSUMS_URL="https://github.com/${REPO}/releases/download/${LATEST_TAG}/checksums.txt"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading Mooshik ${LATEST_TAG} for ${TARGET}..."
curl -fsSL "$DOWNLOAD_URL" -o "${TMP_DIR}/${ARCHIVE_NAME}"
curl -fsSL "$CHECKSUMS_URL" -o "${TMP_DIR}/checksums.txt"

echo "Verifying checksum..."
cd "$TMP_DIR"
if command -v sha256sum >/dev/null 2>&1; then
    grep "$ARCHIVE_NAME" checksums.txt | sha256sum -c -
elif command -v shasum >/dev/null 2>&1; then
    grep "$ARCHIVE_NAME" checksums.txt | shasum -a 256 -c -
else
    echo "Warning: Neither sha256sum nor shasum found. Skipping checksum verification." >&2
fi

tar -xzf "$ARCHIVE_NAME"

mkdir -p "$INSTALL_DIR"
mv mooshik "$INSTALL_DIR/mooshik"
chmod +x "$INSTALL_DIR/mooshik"

echo ""
echo "Mooshik installed successfully to ${INSTALL_DIR}/mooshik."
echo ""
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Note: ${INSTALL_DIR} is not in your PATH."
        echo "Add it to your shell configuration:"
        echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo ""
        ;;
esac

echo "Run 'mooshik init' to initialize your workspace."
echo "Documentation: https://nrynss.github.io/mooshik/"
