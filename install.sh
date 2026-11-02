#!/bin/sh
# ClipTrail CLI installer — standard "curl | sh" path for any Linux distro:
#
#   curl -fsSL https://raw.githubusercontent.com/abhijitdalal26/cliptrail/main/install.sh | sh
#
# Downloads the newest GitHub release tarball for this machine's architecture
# (x86_64 or aarch64), verifies its SHA-256, and installs `clipd` into
# $CLIPD_INSTALL_DIR (default: ~/.local/bin; /usr/local/bin with --system).
# No sudo, no package manager, no runtime deps beyond libc + libxcb (X11).
set -eu

REPO="abhijitdalal26/cliptrail"
INSTALL_DIR="${CLIPD_INSTALL_DIR:-$HOME/.local/bin}"

for arg in "$@"; do
    case "$arg" in
        --system) INSTALL_DIR=/usr/local/bin ;;
        --help|-h)
            echo "Usage: install.sh [--system]"
            echo "  --system   install to /usr/local/bin (needs write access)"
            echo "  default    install to \$CLIPD_INSTALL_DIR or ~/.local/bin"
            exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }
need curl; need tar; need sha256sum

case "$(uname -s)" in
    Linux) ;;
    *) echo "this installer is Linux-only; see the release page for other platforms" >&2; exit 1 ;;
esac

case "$(uname -m)" in
    x86_64|amd64) TARGET=x86_64-unknown-linux-gnu ;;
    aarch64|arm64) TARGET=aarch64-unknown-linux-gnu ;;
    *) echo "unsupported architecture: $(uname -m) (build from source: cargo install --path crates/clipd-cli)" >&2; exit 1 ;;
esac

echo "Resolving latest release of $REPO ..."
TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$TAG" ] || { echo "no published release yet — build from source (cargo install --path crates/clipd-cli)" >&2; exit 1; }

BASE="https://github.com/$REPO/releases/download/$TAG"
NAME="clipd-$TAG-$TARGET"
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT

echo "Downloading $NAME.tar.gz ($TAG) ..."
curl -fsSL "$BASE/$NAME.tar.gz" -o "$TMP/$NAME.tar.gz"
curl -fsSL "$BASE/$NAME.tar.gz.sha256" -o "$TMP/$NAME.tar.gz.sha256"

(cd "$TMP" && sha256sum -c "$NAME.tar.gz.sha256")

tar xzf "$TMP/$NAME.tar.gz" -C "$TMP"
mkdir -p "$INSTALL_DIR"
install -m 0755 "$TMP/$NAME/clipd" "$INSTALL_DIR/clipd"

echo "Installed: $INSTALL_DIR/clipd"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) echo "note: $INSTALL_DIR is not on your PATH" ;;
esac
"$INSTALL_DIR/clipd" --version 2>/dev/null || true
