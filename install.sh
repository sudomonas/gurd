#!/bin/sh
# Installer for gurd on Linux and macOS
# Usage:
#   curl -proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/sudomonas/gurd/main/install.sh | sh
# Or with options:
#   curl -proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/sudomonas/gurd/main/install.sh | sh -s -- --to /usr/local/bin

set -eu

REPO="sudomonas/gurd"
BIN_NAME="gurd"

help() {
    cat <<EOF
gurd installer

Usage:
    install.sh [options]

Options:
    --to <dir>       Directory to install the binary into (default: ~/.local/bin)
    --version <ver>  Version to install (default: latest, e.g. v0.2.0)
    -h, --help       Show this help message
EOF
}

INSTALL_DIR="${HOME}/.local/bin"
VERSION=""

while [ $# -gt 0 ]; do
    case "$1" in
        --to)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --version)
            VERSION="$2"
            shift 2
            ;;
        -h|--help)
            help
            exit 0
            ;;
        *)
            echo "Unknown option: $1" >&2
            help >&2
            exit 1
            ;;
    esac
done

# Detect OS
OS="$(uname -s)"
case "$OS" in
    Linux)
        OS_TYPE="linux"
        ;;
    Darwin)
        OS_TYPE="apple-darwin"
        ;;
    *)
        echo "Error: Unsupported operating system: $OS" >&2
        echo "For Windows, use: irm https://raw.githubusercontent.com/sudomonas/gurd/main/install.ps1 | iex" >&2
        exit 1
        ;;
esac

# Detect Architecture
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64|amd64)
        ARCH_TYPE="x86_64"
        ;;
    aarch64|arm64)
        ARCH_TYPE="aarch64"
        ;;
    *)
        echo "Error: Unsupported architecture: $ARCH" >&2
        exit 1
        ;;
esac

# Determine target triple
if [ "$OS_TYPE" = "linux" ]; then
    # Use static musl build for universal Linux compatibility across any distro
    TARGET="${ARCH_TYPE}-unknown-linux-musl"
elif [ "$OS_TYPE" = "apple-darwin" ]; then
    TARGET="${ARCH_TYPE}-apple-darwin"
fi

# Determine version
if [ -z "$VERSION" ]; then
    echo "Finding latest release of ${REPO}..."
    if command -v curl >/dev/null 2>&1; then
        LATEST_URL=$(curl -sSL -I -o /dev/null -w '%{url_effective}' "https://github.com/${REPO}/releases/latest" 2>/dev/null || true)
        VERSION=$(basename "$LATEST_URL")
    fi
    if [ -z "$VERSION" ] || [ "$VERSION" = "latest" ]; then
        # Fallback to GitHub API
        VERSION=$(curl -sSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null | grep '"tag_name":' | head -n 1 | cut -d '"' -f 4 || true)
    fi
    if [ -z "$VERSION" ]; then
        echo "Error: Could not determine latest version. Specify one using --version vX.Y.Z" >&2
        exit 1
    fi
fi

# Ensure version has 'v' prefix
case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
esac

ARCHIVE="${BIN_NAME}-${TARGET}.tar.gz"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARCHIVE}"
CHECKSUM_URL="${DOWNLOAD_URL}.sha256"

TMP_DIR="$(mktemp -d 2>/dev/null || mktemp -d -t 'gurd-install')"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

echo "Downloading ${BIN_NAME} ${VERSION} for ${TARGET}..."
if command -v curl >/dev/null 2>&1; then
    curl -fL -o "${TMP_DIR}/${ARCHIVE}" "$DOWNLOAD_URL"
    curl -fL -o "${TMP_DIR}/${ARCHIVE}.sha256" "$CHECKSUM_URL" || true
elif command -v wget >/dev/null 2>&1; then
    wget -qO "${TMP_DIR}/${ARCHIVE}" "$DOWNLOAD_URL"
    wget -qO "${TMP_DIR}/${ARCHIVE}.sha256" "$CHECKSUM_URL" || true
else
    echo "Error: curl or wget is required to download" >&2
    exit 1
fi

# Verify checksum if available
if [ -f "${TMP_DIR}/${ARCHIVE}.sha256" ] && [ -s "${TMP_DIR}/${ARCHIVE}.sha256" ]; then
    echo "Verifying SHA-256 checksum..."
    (
        cd "$TMP_DIR"
        if command -v sha256sum >/dev/null 2>&1; then
            sha256sum -c "${ARCHIVE}.sha256"
        elif command -v shasum >/dev/null 2>&1; then
            shasum -a 256 -c "${ARCHIVE}.sha256"
        fi
    )
fi

echo "Extracting archive..."
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"

# Look for binary in temp directory
if [ -f "${TMP_DIR}/${BIN_NAME}" ]; then
    SRC_BIN="${TMP_DIR}/${BIN_NAME}"
elif [ -f "${TMP_DIR}/${BIN_NAME}-${TARGET}/${BIN_NAME}" ]; then
    SRC_BIN="${TMP_DIR}/${BIN_NAME}-${TARGET}/${BIN_NAME}"
else
    SRC_BIN="$(find "$TMP_DIR" -type f -name "$BIN_NAME" | head -n 1)"
fi

if [ -z "$SRC_BIN" ] || [ ! -f "$SRC_BIN" ]; then
    echo "Error: Could not locate '${BIN_NAME}' inside extracted archive" >&2
    exit 1
fi

chmod +x "$SRC_BIN"
cp "$SRC_BIN" "${INSTALL_DIR}/${BIN_NAME}"

echo "Successfully installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"

# PATH check
case ":$PATH:" in
    *":${INSTALL_DIR}:"*)
        ;;
    *)
        echo ""
        echo "Notice: ${INSTALL_DIR} is not in your PATH."
        echo "Add it to your shell configuration (e.g. ~/.bashrc or ~/.zshrc):"
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        echo ""
        ;;
esac

"${INSTALL_DIR}/${BIN_NAME}" --version || true
echo ""
echo "Next step: run 'gurd sync' to get the latest drug database:"
echo "    gurd sync"
echo ""
