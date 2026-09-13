#!/usr/bin/env sh
# Installs the latest mangapress release for macOS/Linux.
#   curl -fsSL https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.sh | sh
set -eu

REPO="gustavommcv/mangapress"
BIN_NAME="mangapress"
INSTALL_DIR="${MANGAPRESS_INSTALL_DIR:-$HOME/.local/bin}"

detect_os() {
  case "$(uname -s)" in
    Linux) echo "linux" ;;
    Darwin) echo "darwin" ;;
    *)
      echo "error: unsupported OS '$(uname -s)' -- build from source instead (see README)" >&2
      exit 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64 | amd64) echo "x86_64" ;;
    arm64 | aarch64) echo "aarch64" ;;
    *)
      echo "error: unsupported architecture '$(uname -m)' -- build from source instead (see README)" >&2
      exit 1
      ;;
  esac
}

target_triple() {
  case "$(detect_os)-$(detect_arch)" in
    linux-x86_64) echo "x86_64-unknown-linux-musl" ;;
    darwin-x86_64) echo "x86_64-apple-darwin" ;;
    darwin-aarch64) echo "aarch64-apple-darwin" ;;
    *)
      echo "error: no prebuilt binary for $(detect_os)/$(detect_arch) -- build from source instead (see README)" >&2
      exit 1
      ;;
  esac
}

TARGET="$(target_triple)"
ASSET="${BIN_NAME}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/latest/download/${ASSET}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading ${ASSET}..."
curl -fsSL "$URL" -o "$TMP_DIR/$ASSET"
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

mkdir -p "$INSTALL_DIR"
mv "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
chmod +x "$INSTALL_DIR/$BIN_NAME"

echo "Installed mangapress to $INSTALL_DIR/$BIN_NAME"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

"$INSTALL_DIR/$BIN_NAME" --version
