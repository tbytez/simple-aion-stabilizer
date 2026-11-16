#!/usr/bin/env bash
# Build SimpleAionStabilizer.exe and assemble the release zip.
#
# Usage: ./package.sh [path-to-extracted-zapret-bundle]
#   The bundle dir must contain bin\ and lists\ (e.g. an extracted
#   zapret-discord-youtube-1.10.4 release zip).
set -euo pipefail

APP="SimpleAionStabilizer"
HERE="$(cd "$(dirname "$0")" && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$HERE/Cargo.toml" | head -1)"
[ -n "$VERSION" ] || { echo "cannot read version from Cargo.toml"; exit 1; }
ZAPRET_SRC="${1:-/tmp/zapret-dl/zapret/zapret-discord-youtube-1.10.4}"
OUT="$HERE/dist/${APP}"
ZIP="$HERE/dist/${APP}-v${VERSION}-win64.zip"

export PATH="$HOME/.cargo/bin:/opt/homebrew/opt/rustup/bin:$PATH"

echo "==> Building release exe"
cargo build --release --target x86_64-pc-windows-gnu --manifest-path "$HERE/Cargo.toml"

EXE="$HERE/target/x86_64-pc-windows-gnu/release/aion-stabilizer.exe"
[ -f "$EXE" ] || { echo "build failed: $EXE missing"; exit 1; }

echo "==> Assembling $OUT"
rm -rf "$OUT" "$ZIP"
mkdir -p "$OUT"

cp "$EXE" "$OUT/AionStabilizer.exe"
cp "$HERE/assets/profile.txt" "$OUT/profile.txt"
cp "$HERE/assets/uninstall.cmd" "$OUT/uninstall.cmd"
cp "$HERE/README.md" "$HERE/LICENSE" "$HERE/THIRD_PARTY_NOTICES.txt" "$OUT/"

# zapret engine binaries
cp -R "$ZAPRET_SRC/bin" "$OUT/bin"
# zapret lists (hostlists/excludes), then overlay our Aion additions
cp -R "$ZAPRET_SRC/lists" "$OUT/lists"
cp "$HERE/assets/lists/list-aion.txt" "$OUT/lists/list-aion.txt"
cp "$HERE/assets/lists/ipset-all.txt" "$OUT/lists/ipset-all.txt"

echo "==> Creating $ZIP"
(cd "$HERE/dist" && zip -qry "$(basename "$ZIP")" "$(basename "$OUT")")

echo "==> Done"
ls -lh "$ZIP"
