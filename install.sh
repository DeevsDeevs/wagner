#!/bin/sh
set -e

REPO="DeevsDeevs/wagner"
BINARY_NAME="wagner"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

main() {
    detect_platform
    get_latest_version
    download_and_install
    verify_installation
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  OS="linux" ;;
        Darwin) OS="darwin" ;;
        *)      error "Unsupported OS: $OS" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)  ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *)             error "Unsupported architecture: $ARCH" ;;
    esac

    PLATFORM="${BINARY_NAME}-${OS}-${ARCH}"
    echo "Detected platform: $PLATFORM"
}

get_latest_version() {
    echo "Fetching latest version..."
    VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest version"
    fi

    echo "Latest version: $VERSION"
}

download_and_install() {
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/${PLATFORM}.tar.gz"

    echo "Downloading $DOWNLOAD_URL..."

    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT

    curl -fsSL "$DOWNLOAD_URL" -o "$TMPDIR/$PLATFORM.tar.gz"

    echo "Extracting..."
    tar -xzf "$TMPDIR/$PLATFORM.tar.gz" -C "$TMPDIR"

    echo "Installing to $INSTALL_DIR..."
    mkdir -p "$INSTALL_DIR"
    mv "$TMPDIR/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
}

verify_installation() {
    if command -v "$BINARY_NAME" >/dev/null 2>&1; then
        echo ""
        echo "wagner installed successfully!"
        echo "Version: $($BINARY_NAME --version 2>/dev/null || echo $VERSION)"
    else
        echo ""
        echo "wagner installed to $INSTALL_DIR/$BINARY_NAME"
        echo ""
        echo "Add to your PATH if not already:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    fi
}

error() {
    echo "Error: $1" >&2
    exit 1
}

main
