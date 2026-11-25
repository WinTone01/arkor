#!/usr/bin/env bash
# Builds Arkor and packs a release archive for GitHub Releases:
#   dist/arkor-<version>-x86_64-linux.tar.gz
#     arkor-<version>/bin/arkor
#     arkor-<version>/bin/arkor-login   (store logins in a window of their own)
#     arkor-<version>/share/applications/arkor.desktop
#     arkor-<version>/share/icons/hicolor/.../arkor.(svg|png)
#     arkor-<version>/share/licenses/arkor/LICENSE
# The install script and Arkor's own updater unpack this layout.
set -euo pipefail
cd "$(dirname "$0")/.."
version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
name="arkor-$version"
cargo build --release --locked -p arkor -p arkor-login
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
install -Dm755 target/release/arkor "$root/bin/arkor"
install -Dm755 target/release/arkor-login "$root/bin/arkor-login"
strip "$root/bin/arkor" "$root/bin/arkor-login" 2>/dev/null || true
install -Dm644 packaging/arkor.desktop "$root/share/applications/arkor.desktop"
install -Dm644 crates/arkor/assets/logo/arkor.svg "$root/share/icons/hicolor/scalable/apps/arkor.svg"
for size in 16 24 32 48 64 128 256 512; do
  install -Dm644 "crates/arkor/assets/logo/arkor-$size.png" "$root/share/icons/hicolor/${size}x${size}/apps/arkor.png"
done
install -Dm644 LICENSE "$root/share/licenses/arkor/LICENSE"
mkdir -p dist
tar -C "$stage" -czf "dist/$name-x86_64-linux.tar.gz" "$name"
(cd dist && sha256sum "$name-x86_64-linux.tar.gz" > "$name-x86_64-linux.tar.gz.sha256")
echo "dist/$name-x86_64-linux.tar.gz"
