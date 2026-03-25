#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST_PATH="${ROOT_DIR}/ffmpeg/jellyfin-ffmpeg-rk3588.env"
DISTRO="${1:-bookworm}"
ARCH="${2:-arm64}"

LATEST_JSON="$(curl -fsSL https://api.github.com/repos/jellyfin/jellyfin-ffmpeg/releases/latest)"
TAG="$(printf '%s' "${LATEST_JSON}" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"

if [[ -z "${TAG}" ]]; then
  echo "Failed to resolve latest jellyfin-ffmpeg tag from GitHub API" >&2
  exit 1
fi

ASSET_URL="$(printf '%s' "${LATEST_JSON}" | grep -oE "https://[^"]*jellyfin-ffmpeg7_[^"]*_${DISTRO}_${ARCH}\\.deb" | head -n 1)"

if [[ -z "${ASSET_URL}" ]]; then
  echo "Failed to find asset URL for distro=${DISTRO} arch=${ARCH} in latest release ${TAG}" >&2
  exit 1
fi

ASSET_DEB="$(basename "${ASSET_URL}")"

TMP_DEB="$(mktemp)"
cleanup() {
  rm -f "${TMP_DEB}"
}
trap cleanup EXIT

curl -fL "${ASSET_URL}" -o "${TMP_DEB}"
SHA256="$(sha256sum "${TMP_DEB}" | awk '{print $1}')"

cat > "${MANIFEST_PATH}" <<EOF
JELLYFIN_FFMPEG_TAG=${TAG}
JELLYFIN_FFMPEG_DISTRO=${DISTRO}
JELLYFIN_FFMPEG_ARCH=${ARCH}
JELLYFIN_FFMPEG_DEB=${ASSET_DEB}
JELLYFIN_FFMPEG_URL=${ASSET_URL}
JELLYFIN_FFMPEG_SHA256=${SHA256}
EOF

echo "Updated ${MANIFEST_PATH}"
echo "  tag: ${TAG}"
echo "  deb: ${ASSET_DEB}"
echo "  sha256: ${SHA256}"
