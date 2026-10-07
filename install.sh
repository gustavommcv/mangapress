#!/usr/bin/env sh
# Installs a verified mangapress release for macOS/Linux.
#   curl -fsSL https://raw.githubusercontent.com/gustavommcv/mangapress/main/install.sh | sh
set -eu

REPO="gustavommcv/mangapress"
BIN_NAME="mangapress"
INSTALL_DIR="${MANGAPRESS_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${MANGAPRESS_VERSION:-latest}"
LICENSE_DIR="$INSTALL_DIR/mangapress-licenses"

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
if [ "$VERSION" = latest ]; then
  RELEASE_URL="$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest")"
  case "$RELEASE_URL" in
    "https://github.com/$REPO/releases/tag/"*) VERSION="${RELEASE_URL#"https://github.com/$REPO/releases/tag/"}" ;;
    *) echo "error: could not resolve the latest release tag" >&2; exit 1 ;;
  esac
fi
case "$VERSION" in
  v*) ;;
  *) VERSION="v$VERSION" ;;
esac
case "$VERSION" in
  *[!0-9A-Za-z.+-]*) echo "error: invalid release version '$VERSION'" >&2; exit 1 ;;
esac
if ! printf '%s\n' "$VERSION" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$'; then
  echo "error: invalid release version '$VERSION'" >&2
  exit 1
fi
URL="https://github.com/$REPO/releases/download/$VERSION"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' 0
trap 'exit 1' HUP INT TERM

echo "Downloading $ASSET from $VERSION..."
curl -fsSL "$URL/checksums.txt" -o "$TMP_DIR/checksums.txt"
curl -fsSL "$URL/$ASSET" -o "$TMP_DIR/$ASSET"

# Select exactly one entry; do not verify unrelated platform packages.
if ! awk -v asset="$ASSET" '
  { sub(/\r$/, "") }
  $2 == asset || $2 == "*" asset {
    count++
    if (NF != 2 || length($1) != 64 || $1 ~ /[^0-9a-fA-F]/) invalid = 1
    print $1 "  " asset
  }
  END { if (count != 1 || invalid) exit 1 }
' "$TMP_DIR/checksums.txt" > "$TMP_DIR/selected-checksum.txt"; then
  echo "error: missing, duplicate, or malformed checksum for $ASSET" >&2
  exit 1
fi
if command -v sha256sum >/dev/null 2>&1; then
  if ! (cd "$TMP_DIR" && sha256sum -c selected-checksum.txt); then
    echo "error: checksum verification failed for $ASSET" >&2; exit 1
  fi
elif command -v shasum >/dev/null 2>&1; then
  if ! (cd "$TMP_DIR" && shasum -a 256 -c selected-checksum.txt); then
    echo "error: checksum verification failed for $ASSET" >&2; exit 1
  fi
else
  echo "error: install sha256sum or shasum to verify the download" >&2
  exit 1
fi
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"
if [ ! -f "$TMP_DIR/$BIN_NAME" ] || [ -L "$TMP_DIR/$BIN_NAME" ]; then
  echo "error: the archive does not contain a regular $BIN_NAME executable" >&2
  exit 1
fi
chmod +x "$TMP_DIR/$BIN_NAME"
ACTUAL_VERSION="$("$TMP_DIR/$BIN_NAME" --version)"
if [ "$ACTUAL_VERSION" != "$BIN_NAME ${VERSION#v}" ]; then
  echo "error: the downloaded executable does not match $VERSION" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR" "$LICENSE_DIR"
install -m 755 "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
for NOTICE in LICENSE-MIT LICENSE-APACHE THIRD-PARTY-NOTICES.md DEPENDENCY-LICENSES.txt; do
  if [ -f "$TMP_DIR/$NOTICE" ] && [ ! -L "$TMP_DIR/$NOTICE" ]; then
    install -m 644 "$TMP_DIR/$NOTICE" "$LICENSE_DIR/$NOTICE"
  else
    # Older archives may omit a notice; do not leave another version's text.
    rm -f "$LICENSE_DIR/$NOTICE"
  fi
done

echo "Installed mangapress to $INSTALL_DIR/$BIN_NAME"
echo "License notices: $LICENSE_DIR"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    echo ""
    echo "$INSTALL_DIR is not on your PATH. Add this to your shell profile:"
    echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
    ;;
esac

printf '%s\n' "$ACTUAL_VERSION"
