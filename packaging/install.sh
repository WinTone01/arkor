#!/usr/bin/env bash
# Installs the latest Arkor release from GitHub into ~/.local (no root, no build):
#   curl -fsSL https://raw.githubusercontent.com/WinTone01/arkor/master/packaging/install.sh | bash
# Run it again to update (Arkor can also update itself from Settings → General).
# Remove with: bash install.sh --uninstall
set -euo pipefail

repo="WinTone01/arkor"
prefix="${ARKOR_PREFIX:-$HOME/.local}"
bin="$prefix/bin/arkor"

say() { printf '\033[1;35m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
  rm -f "$bin" "$prefix/share/applications/arkor.desktop"
  find "$prefix/share/icons/hicolor" -name 'arkor.png' -o -name 'arkor.svg' 2>/dev/null | xargs -r rm -f
  rm -rf "$prefix/share/licenses/arkor"
  say "Arkor removed (your games, prefixes and settings in ~/.config/arkor are kept)."
  exit 0
fi

[ "$(uname -m)" = "x86_64" ] || die "Arkor releases are built for x86_64."
for tool in curl tar; do command -v "$tool" >/dev/null || die "$tool is needed."; done

say "Looking up the latest release…"
api=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest") || die "could not reach GitHub."
url=$(printf '%s' "$api" | grep -o '"browser_download_url": *"[^"]*x86_64-linux\.tar\.gz"' | head -n1 | sed 's/.*"\(https[^"]*\)"/\1/')
tag=$(printf '%s' "$api" | grep -o '"tag_name": *"[^"]*"' | head -n1 | sed 's/.*"\([^"]*\)"$/\1/')
[ -n "$url" ] || die "the latest release has no Linux build."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
say "Downloading Arkor $tag…"
curl -fL --progress-bar -o "$work/arkor.tar.gz" "$url"
if sha=$(curl -fsSL "$url.sha256" 2>/dev/null); then
  expected=${sha%% *}
  actual=$(sha256sum "$work/arkor.tar.gz" | cut -d' ' -f1)
  [ "$expected" = "$actual" ] || die "checksum mismatch; the download is damaged."
fi
tar -xzf "$work/arkor.tar.gz" -C "$work"
root=$(find "$work" -mindepth 1 -maxdepth 1 -type d | head -n1)
[ -x "$root/bin/arkor" ] || die "the archive has no bin/arkor."

say "Installing into $prefix…"
install -Dm755 "$root/bin/arkor" "$bin.new" && mv -f "$bin.new" "$bin"
mkdir -p "$prefix/share"
cp -r "$root/share/." "$prefix/share/"
# The menu entry starts this copy, even when ~/.local/bin is not on PATH.
sed -i "s|^Exec=.*|Exec=$bin|" "$prefix/share/applications/arkor.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" 2>/dev/null || true

say "Arkor $tag is installed. Start it from the app menu or run: $bin"
case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "   ($prefix/bin is not on your PATH; the app menu entry works anyway.)";; esac
