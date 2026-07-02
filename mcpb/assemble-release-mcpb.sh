#!/usr/bin/env bash
# assemble-release-mcpb.sh — Assemble the two-platform wireforge.mcpb from
# GitHub Release assets (instead of a local single-platform build).
#
# Usage:
#   bash mcpb/assemble-release-mcpb.sh <TAG>      # e.g. v0.1.0
#
# What it does:
#   1. Downloads the darwin-aarch64 tar.gz and windows-x86_64 zip release
#      assets for <TAG> via `gh release download`.
#   2. Extracts `wf-mcp` (darwin) and `wf-mcp.exe` (win32) from them.
#   3. Stages manifest.json + both binaries in the layout manifest.json
#      declares: server/wf-mcp (unix path -> darwin binary) and
#      server/wf-mcp.exe (platform_overrides.win32).
#   4. Zips to mcpb/dist/wireforge.mcpb and writes the SHA-256 you need
#      for server.json "fileSha256".
#
# Linux is intentionally NOT bundled: manifest.json compatibility lists
# darwin + win32 only (the platforms .mcpb installers actually target);
# Linux users install with `cargo install wf-mcp`.
#
# Prerequisites: gh (authenticated), unzip, tar, sha256sum, python 3.
#
# The zip is written by python (not Info-ZIP) so the darwin binary keeps its
# unix executable bit (external_attr 0755, create_system=unix) — a plain
# Windows-built zip would strip it and the extracted server would not run.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: bash mcpb/assemble-release-mcpb.sh <TAG>" >&2
  exit 1
fi
TAG="$1"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MANIFEST_SRC="${SCRIPT_DIR}/manifest.json"
DIST_DIR="${SCRIPT_DIR}/dist"
BUNDLE_NAME="wireforge.mcpb"
BUNDLE_PATH="${DIST_DIR}/${BUNDLE_NAME}"
REPO_SLUG="hanmahong5-arch/wireforge-core"

DARWIN_ASSET="wf-${TAG}-aarch64-apple-darwin.tar.gz"
WIN_ASSET="wf-${TAG}-x86_64-pc-windows-msvc.zip"

STAGE_DIR="$(mktemp -d)"
DL_DIR="$(mktemp -d)"
trap 'rm -rf "${STAGE_DIR}" "${DL_DIR}"' EXIT

echo "==> Downloading release assets for ${TAG}"
gh release download "${TAG}" -R "${REPO_SLUG}" -D "${DL_DIR}" \
  -p "${DARWIN_ASSET}" -p "${WIN_ASSET}"

echo "==> Extracting wf-mcp binaries"
tar -xzf "${DL_DIR}/${DARWIN_ASSET}" -C "${DL_DIR}"
unzip -q "${DL_DIR}/${WIN_ASSET}" -d "${DL_DIR}"

DARWIN_BIN="${DL_DIR}/wf-${TAG}-aarch64-apple-darwin/wf-mcp"
WIN_BIN="${DL_DIR}/wf-${TAG}-x86_64-pc-windows-msvc/wf-mcp.exe"
for f in "${DARWIN_BIN}" "${WIN_BIN}"; do
  [[ -f "$f" ]] || { echo "ERROR: expected binary not found: $f" >&2; exit 1; }
done

mkdir -p "${STAGE_DIR}/server"
cp "${MANIFEST_SRC}" "${STAGE_DIR}/manifest.json"
cp "${DARWIN_BIN}" "${STAGE_DIR}/server/wf-mcp"
cp "${WIN_BIN}" "${STAGE_DIR}/server/wf-mcp.exe"
chmod +x "${STAGE_DIR}/server/wf-mcp"

echo "==> Bundle stage layout:"
find "${STAGE_DIR}" -type f | sort | sed "s|${STAGE_DIR}/||"

mkdir -p "${DIST_DIR}"
rm -f "${BUNDLE_PATH}"
PYTHON="${PYTHON:-python}"
STAGE_ARG="${STAGE_DIR}"
BUNDLE_ARG="${BUNDLE_PATH}"
if command -v cygpath >/dev/null 2>&1; then
  STAGE_ARG="$(cygpath -w "${STAGE_DIR}")"
  BUNDLE_ARG="$(cygpath -w "${BUNDLE_PATH}")"
fi
"${PYTHON}" - "${BUNDLE_ARG}" "${STAGE_ARG}" <<'PYEOF'
import os, sys, zipfile

bundle, stage = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(bundle, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
    entries = []
    for root, _dirs, files in os.walk(stage):
        for name in files:
            full = os.path.join(root, name)
            rel = os.path.relpath(full, stage).replace(os.sep, "/")
            entries.append((rel, full))
    for rel, full in sorted(entries):
        info = zipfile.ZipInfo(rel)
        info.create_system = 3  # unix, so external_attr mode bits are honored
        info.compress_type = zipfile.ZIP_DEFLATED
        mode = 0o755 if rel.startswith("server/") else 0o644
        info.external_attr = (0o100000 | mode) << 16  # regular file + mode
        with open(full, "rb") as f:
            zf.writestr(info, f.read())
        print(f"  added {rel} (mode {oct(mode)})")
PYEOF

SHA256="$(sha256sum "${BUNDLE_PATH}" | awk '{print $1}')"
echo "${SHA256}  ${BUNDLE_NAME}" > "${DIST_DIR}/${BUNDLE_NAME}.sha256"

echo ""
echo "========================================================"
echo "  Bundle:  ${BUNDLE_PATH} ($(du -h "${BUNDLE_PATH}" | cut -f1))"
echo "  SHA-256: ${SHA256}"
echo "========================================================"
echo ""
echo "Next steps:"
echo "  1. gh release upload ${TAG} ${BUNDLE_PATH} ${BUNDLE_PATH}.sha256 -R ${REPO_SLUG}"
echo "  2. Set server.json packages[0].fileSha256 = ${SHA256}"
echo "  3. mcp-publisher login github && mcp-publisher publish"
