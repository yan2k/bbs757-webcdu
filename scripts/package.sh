#!/usr/bin/env bash
# Build both release downloads into dist/:
#   bbs757-webcdu-<version>.zip      exe + SimConnect.dll side by side (+ licenses)
#   bbs757-webcdu-<version>-aio.exe  single exe with SimConnect.dll embedded
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=x86_64-pc-windows-gnu
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
NAME="bbs757-webcdu-$VERSION"
OUT="target/$TARGET/release/bbs757-webcdu.exe"

scripts/fetch-simconnect.sh
rm -rf dist && mkdir -p "dist/$NAME"

# 1) zip: plain exe + SimConnect.dll next to it
cargo build --release --locked --target "$TARGET"
cp "$OUT" "dist/$NAME/bbs757-webcdu.exe"
cp vendor/SimConnect.dll "dist/$NAME/"
cp LICENSE "dist/$NAME/LICENSE.txt"
cp assets/fonts/OFL.txt "dist/$NAME/OFL.txt"
(cd dist && zip -qr "$NAME.zip" "$NAME")

# 2) aio: one exe with SimConnect.dll inside
cargo build --release --locked --target "$TARGET" --features embed-simconnect
cp "$OUT" "dist/$NAME-aio.exe"

rm -rf "dist/$NAME"
ls -l dist
