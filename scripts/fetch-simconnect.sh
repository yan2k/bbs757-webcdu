#!/usr/bin/env bash
# Fetch SimConnect.dll into vendor/ (it's embedded into bbs757-webcdu.exe at build time).
#
# Source: the "SimConnect" Python package on PyPI, which redistributes the MSFS SDK's
# SimConnect.dll. Both the wheel and the DLL are pinned by SHA-256.
set -euo pipefail
cd "$(dirname "$0")/.."

WHEEL_URL="https://files.pythonhosted.org/packages/76/bf/3baa88079144b6e5ab224b76725fa36f502554e95b9c27c42bac9c6de48f/SimConnect-0.4.26-py2.py3-none-any.whl"
WHEEL_SHA256="b77ca43f52df236a34f1f6b1b7c7cfdb0ff5677f86f8a6fd2c1819078fe07d76"
DLL_SHA256="677fd3849d54e2a6831dc1aa348d974ead2255ec609fbf5830ef32bf0804b8bc"
OUT="vendor/SimConnect.dll"

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

if [[ -f "$OUT" && "$(sha "$OUT")" == "$DLL_SHA256" ]]; then
  echo "SimConnect.dll already present"
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "downloading SimConnect wheel..."
curl -fsSL -o "$tmp/simconnect.whl" "$WHEEL_URL"
[[ "$(sha "$tmp/simconnect.whl")" == "$WHEEL_SHA256" ]] || { echo "wheel checksum mismatch" >&2; exit 1; }

unzip -q -o -j "$tmp/simconnect.whl" "SimConnect/SimConnect.dll" -d "$tmp"
[[ "$(sha "$tmp/SimConnect.dll")" == "$DLL_SHA256" ]] || { echo "DLL checksum mismatch" >&2; exit 1; }

mkdir -p vendor
mv "$tmp/SimConnect.dll" "$OUT"
echo "wrote $OUT"
