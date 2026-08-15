#!/usr/bin/env bash
# agent-session-grep install script (Unix: macOS/Linux)
# Usage: curl -sSL https://raw.githubusercontent.com/qin-devs/AgentSessions/main/scripts/install.sh | bash
set -euo pipefail

REPO="qin-devs/AgentSessions"
BINARY_NAME="agent-session-grep"
ALIAS_NAME="asg"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

echo "agent-session-grep installer (Unix)"
echo "-----------------------------------"

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin) PLATFORM="macos" ;;
  Linux)  PLATFORM="linux" ;;
  *) echo "Error: unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64) ARCH="x64" ;;
  arm64|aarch64) ARCH="arm64" ;;
  *) echo "Error: unsupported arch: $ARCH"; exit 1 ;;
esac

echo "Platform: $PLATFORM-$ARCH"
echo "Install dir: $INSTALL_DIR"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Build from source (binary releases deferred — requires GitHub Release setup)
if ! command -v cargo &>/dev/null; then
  echo "Error: cargo not found. Install Rust: https://rustup.rs"
  exit 1
fi

echo "Building from source..."
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

git clone --depth 1 "https://github.com/$REPO.git" "$TMPDIR/repo"
cd "$TMPDIR/repo"
cargo build --release --workspace

# Install binary + alias
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$BINARY_NAME"
cp "target/release/$BINARY_NAME" "$INSTALL_DIR/$ALIAS_NAME"

echo ""
echo "✓ Installed $BINARY_NAME and $ALIAS_NAME to $INSTALL_DIR"
echo ""

# PATH check
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Warning: $INSTALL_DIR is not in your PATH."
     echo "Add this to your shell profile (~/.bashrc, ~/.zshrc):"
     echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
     ;;
esac

echo ""
echo "Verify: $INSTALL_DIR/$BINARY_NAME --version"
echo "Quickstart: $ALIAS_NAME sync --discover && $ALIAS_NAME search hello"
