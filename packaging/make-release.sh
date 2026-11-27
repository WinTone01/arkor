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
# Built in Ubuntu 22.04 (packaging/build/Containerfile) with podman, so the programs need no
# glibc newer than 2.35 and run on most distributions; the host's rustup toolchain is used
# read-only. ARKOR_NATIVE=1 builds on this machine instead (runs only where its glibc is).
max_glibc=2.35
if [ "${ARKOR_NATIVE:-}" = 1 ]; then
  cargo build --release --locked -p arkor -p arkor-login
  out=target/release
else
  command -v podman >/dev/null || { echo "podman is needed (or ARKOR_NATIVE=1)" >&2; exit 1; }
  toolchain=$(rustup run stable rustc --print sysroot)
  podman build -q -t arkor-build packaging/build >/dev/null
  # The host's crate downloads are shared (fetched first, so nothing is missing).
  cargo fetch --locked
  cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  mkdir -p "$cargo_home/registry" "$cargo_home/git"
  podman run --rm \
    -v "$PWD:/src" \
    -v "$cargo_home/registry:/cargo/registry" \
    -v "$cargo_home/git:/cargo/git" \
    -v "$toolchain:/rust:ro" \
    -e CARGO_HOME=/cargo -e PATH=/rust/bin:/usr/bin:/bin \
    -e CARGO_TARGET_DIR=/src/target/compat \
    -w /src arkor-build \
    cargo build --release --locked -j "${JOBS:-4}" -p arkor -p arkor-login
  out=target/compat/release
fi
# The newest glibc the programs ask for: no newer than the build system's.
for b in arkor arkor-login; do
  need=$(objdump -T "$out/$b" | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n1)
  if [ "${ARKOR_NATIVE:-}" != 1 ] && [ "$(printf '%s\n%s\n' "$need" "$max_glibc" | sort -V | tail -n1)" != "$max_glibc" ]; then
    echo "$b needs glibc $need (more than $max_glibc)" >&2
    exit 1
  fi
  echo "$b: glibc $need"
done
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
install -Dm755 "$out/arkor" "$root/bin/arkor"
install -Dm755 "$out/arkor-login" "$root/bin/arkor-login"
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
