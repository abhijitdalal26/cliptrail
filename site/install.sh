#!/bin/sh
# ClipTrail installer for Linux:
#
#   curl -fsSL https://abhijitdalal26.github.io/cliptrail/install.sh | sh
#
# Finds the newest release on GitHub and checks the download against the
# SHA-256 GitHub publishes for it. Debian, Ubuntu and derivatives get the .deb
# (installed with apt, which asks for your password); other distros get the
# AppImage in ~/.local/bin with a menu entry. Pass --appimage to always use it.
set -eu

REPO="abhijitdalal26/cliptrail"
MODE=auto
for arg in "$@"; do
    case "$arg" in
        --appimage) MODE=appimage ;;
        --help|-h) echo "Usage: install.sh [--appimage]"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

say() { echo "-> $*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1" >&2; exit 1; }; }
need curl; need sha256sum; need awk

echo "ClipTrail installer"
[ "$(uname -s)" = Linux ] || { echo "this installer is for Linux; see https://abhijitdalal26.github.io/cliptrail/download.html" >&2; exit 1; }
case "$(uname -m)" in
    x86_64|amd64) ;;
    *) echo "ClipTrail's desktop app is built for x86_64 only (this is $(uname -m))" >&2; exit 1 ;;
esac

if [ "$MODE" = auto ]; then
    if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then MODE=deb; else MODE=appimage; fi
fi
case "$MODE" in
    deb) SUFFIX="_amd64.deb" ;;
    appimage) SUFFIX="_amd64.AppImage" ;;
esac

JSON="$(curl -fsSL -H "User-Agent: cliptrail-installer" "https://api.github.com/repos/$REPO/releases/latest")"
TAG="$(printf '%s\n' "$JSON" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -1)"
[ -n "$TAG" ] || { echo "no published release found" >&2; exit 1; }

# The asset whose name ends in $SUFFIX: its name and its "sha256:<hex>" digest.
# The API may return everything on one line, so put one field per line first.
ASSET="$(printf '%s\n' "$JSON" | tr ',' '\n' | awk -v suf="$SUFFIX" '
    /"name":/ { n = $0; sub(/.*"name": *"/, "", n); sub(/".*/, "", n) }
    /"digest":/ && length(n) >= length(suf) && substr(n, length(n) - length(suf) + 1) == suf {
        d = $0; sub(/.*"digest": *"sha256:/, "", d); sub(/".*/, "", d); print n " " d; exit
    }')"
NAME="${ASSET%% *}"
SUM="${ASSET#* }"
[ -n "$ASSET" ] && [ ${#SUM} -eq 64 ] || { echo "no $SUFFIX with a checksum in $TAG" >&2; exit 1; }
say "Latest version: ${TAG#v}"

TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
say "Downloading $NAME"
curl -fsSL "https://github.com/$REPO/releases/download/$TAG/$NAME" -o "$TMP/$NAME"
echo "$SUM  $TMP/$NAME" | sha256sum -c --status || { echo "checksum mismatch: not installing" >&2; exit 1; }
say "Checksum OK"

if [ "$MODE" = deb ]; then
    say "Installing with apt (it may ask for your password)"
    SUDO=""; [ "$(id -u)" -eq 0 ] || SUDO=sudo
    $SUDO apt-get install -y "$TMP/$NAME"
    echo "Done: ClipTrail ${TAG#v} is installed. Open it from your app menu."
else
    BIN="$HOME/.local/bin"; APPS="$HOME/.local/share/applications"
    mkdir -p "$BIN" "$APPS"
    install -m 0755 "$TMP/$NAME" "$BIN/ClipTrail.AppImage"
    cat > "$APPS/cliptrail.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=ClipTrail
Comment=Clipboard history that remembers everything you copy
Exec=$BIN/ClipTrail.AppImage
Categories=Utility;
Terminal=false
EOF
    echo "Done: ClipTrail ${TAG#v} is in $BIN/ClipTrail.AppImage and your app menu."
    echo "If it doesn't start, install FUSE 2 (e.g. 'sudo apt install libfuse2' or 'sudo dnf install fuse')."
fi
