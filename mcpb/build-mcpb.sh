#!/usr/bin/env bash
# build-mcpb.sh — Build a wireforge.mcpb bundle for MCP Registry submission.
#
# Usage:
#   bash mcpb/build-mcpb.sh [TARGET]
#
# TARGET defaults to the host triple detected by rustc.
# Examples:
#   bash mcpb/build-mcpb.sh                              # native
#   bash mcpb/build-mcpb.sh x86_64-pc-windows-msvc      # cross (requires target installed)
#   bash mcpb/build-mcpb.sh x86_64-unknown-linux-gnu
#
# Prerequisites (Windows Git Bash):
#   - cargo at C:/Users/Anita/.cargo/bin (already on PATH in shell, or set CARGO below)
#   - zip (ships with Git for Windows)
#   - sha256sum (ships with Git for Windows coreutils)
#
# Bundle layout produced (zip contents):
#   manifest.json
#   server/wf-mcp          (or server/wf-mcp.exe on win32)
#
# The resulting wireforge.mcpb is placed in mcpb/dist/.
# The SHA-256 of the bundle is printed and written to mcpb/dist/wireforge.mcpb.sha256.
# You will need that hash for server.json "fileSha256" before registry submission.

set -euo pipefail

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
CARGO="${CARGO:-C:/Users/Anita/.cargo/bin/cargo}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MANIFEST_SRC="${SCRIPT_DIR}/manifest.json"
DIST_DIR="${SCRIPT_DIR}/dist"
BUNDLE_NAME="wireforge.mcpb"
BUNDLE_PATH="${DIST_DIR}/${BUNDLE_NAME}"
CRATE="-p wf-mcp"

# Detect host target if not supplied
if [[ $# -ge 1 ]]; then
  TARGET="$1"
else
  RUSTC="${RUSTC:-$(dirname "${CARGO}")/rustc}"
  TARGET="$("${RUSTC}" -vV 2>/dev/null | grep '^host:' | awk '{print $2}')"
  # Fallback if rustc is not next to cargo either
  if [[ -z "${TARGET}" ]]; then
    TARGET="x86_64-pc-windows-msvc"
    echo "WARNING: could not auto-detect host triple, defaulting to ${TARGET}"
  fi
fi

echo "==> Target: ${TARGET}"

# Determine binary name (Windows needs .exe)
case "${TARGET}" in
  *-windows-*) BIN_NAME="wf-mcp.exe" ;;
  *)           BIN_NAME="wf-mcp"    ;;
esac

# ---------------------------------------------------------------------------
# Step 1: Build release binary
# ---------------------------------------------------------------------------
echo "==> Building release binary (cargo build --release ${CRATE} --target ${TARGET})"
cd "${REPO_ROOT}"
"${CARGO}" build --release ${CRATE} --target "${TARGET}"

BINARY_SRC="${REPO_ROOT}/target/${TARGET}/release/${BIN_NAME}"
if [[ ! -f "${BINARY_SRC}" ]]; then
  echo "ERROR: expected binary not found at ${BINARY_SRC}" >&2
  exit 1
fi
echo "    Binary: ${BINARY_SRC} ($(du -h "${BINARY_SRC}" | cut -f1))"

# ---------------------------------------------------------------------------
# Step 2: Stage bundle contents into a temp directory
# ---------------------------------------------------------------------------
STAGE_DIR="$(mktemp -d)"
trap 'rm -rf "${STAGE_DIR}"' EXIT

mkdir -p "${STAGE_DIR}/server"

# Copy manifest (always from the canonical source)
cp "${MANIFEST_SRC}" "${STAGE_DIR}/manifest.json"

# Copy binary into server/ using the platform-conventional name expected by manifest.json
# manifest.json platform_overrides points to server/wf-mcp.exe on win32, server/wf-mcp otherwise.
cp "${BINARY_SRC}" "${STAGE_DIR}/server/${BIN_NAME}"

# Make binary executable (no-op on Windows, required on Linux/macOS)
chmod +x "${STAGE_DIR}/server/${BIN_NAME}"

echo "==> Bundle stage layout:"
find "${STAGE_DIR}" -type f | sort | sed "s|${STAGE_DIR}/||"

# ---------------------------------------------------------------------------
# Step 3: Pack into .mcpb (zip)
# ---------------------------------------------------------------------------
mkdir -p "${DIST_DIR}"
rm -f "${BUNDLE_PATH}"

# zip with -j would lose directory structure; use -r from stage dir instead
cd "${STAGE_DIR}"
zip -r -9 "${BUNDLE_PATH}" manifest.json server/
cd "${REPO_ROOT}"

echo "==> Created: ${BUNDLE_PATH} ($(du -h "${BUNDLE_PATH}" | cut -f1))"

# ---------------------------------------------------------------------------
# Step 4: Compute and print SHA-256
# ---------------------------------------------------------------------------
SHA256="$(sha256sum "${BUNDLE_PATH}" | awk '{print $1}')"
echo "${SHA256}  ${BUNDLE_NAME}" > "${DIST_DIR}/${BUNDLE_NAME}.sha256"

echo ""
echo "========================================================"
echo "  Bundle:  ${BUNDLE_PATH}"
echo "  SHA-256: ${SHA256}"
echo "========================================================"
echo ""
echo "Next steps:"
echo "  1. Upload ${BUNDLE_NAME} as a GitHub Release asset."
echo "     The release URL must contain 'mcp' (already satisfied by .mcpb extension)."
echo "  2. In server.json, set:"
echo "       \"registryType\": \"mcpb\","
echo "       \"identifier\": \"https://github.com/hanmahong5-arch/wireforge-core/releases/download/v0.1.0/${BUNDLE_NAME}\","
echo "       \"fileSha256\": \"${SHA256}\""
echo "  3. Run: mcp-publisher login github && mcp-publisher publish"
