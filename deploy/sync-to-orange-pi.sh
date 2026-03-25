#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "Usage: $0 <user@host:/remote/path> [--delete]" >&2
  exit 1
fi

DEST="$1"
DELETE_FLAG="${2:-}"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
EXCLUDES_FILE="${ROOT_DIR}/deploy/rsync-excludes.txt"
LOCAL_ENV_FILE="${ROOT_DIR}/.env.orange-pi"
LOCAL_COMPOSE_FILE="${ROOT_DIR}/docker-compose.orange-pi.yml"
LOCAL_DEPLOY_KEY="${ROOT_DIR}/.local/ssh/orangepi_deploy_ed25519"

if [[ "${DEST}" != *:* ]]; then
  echo "Destination must be in the form user@host:/remote/path" >&2
  exit 1
fi

REMOTE_HOST="${DEST%%:*}"
REMOTE_PATH="${DEST#*:}"

if [[ ! -f "${EXCLUDES_FILE}" ]]; then
  echo "Missing excludes file: ${EXCLUDES_FILE}" >&2
  exit 1
fi

if [[ ! -f "${LOCAL_ENV_FILE}" ]]; then
  echo "Missing ${LOCAL_ENV_FILE}. Create it from deploy/.env.orange-pi.example first." >&2
  exit 1
fi

if [[ ! -f "${LOCAL_COMPOSE_FILE}" ]]; then
  echo "Missing ${LOCAL_COMPOSE_FILE}" >&2
  exit 1
fi

if [[ -z "${RSYNC_RSH:-}" && -f "${LOCAL_DEPLOY_KEY}" ]]; then
  RSYNC_RSH="ssh -i ${LOCAL_DEPLOY_KEY}"
  export RSYNC_RSH
fi

RSYNC_ARGS=(
  -az
  --info=progress2
  --exclude-from "${EXCLUDES_FILE}"
)

if [[ "${DELETE_FLAG}" == "--delete" ]]; then
  RSYNC_ARGS+=(--delete)
fi

echo "Syncing ${ROOT_DIR}/ -> ${DEST}"
rsync "${RSYNC_ARGS[@]}" "${ROOT_DIR}/" "${DEST}"

echo "Preparing target deploy files at ${REMOTE_HOST}:${REMOTE_PATH}"
if [[ -n "${RSYNC_RSH:-}" ]]; then
  SSH_CMD="${RSYNC_RSH}"
else
  SSH_CMD="ssh"
fi

${SSH_CMD} "${REMOTE_HOST}" "set -e; cd '${REMOTE_PATH}'; cp -f .env.orange-pi .env; cp -f docker-compose.orange-pi.yml docker-compose.yml; mkdir -p state"

echo "Target ready. Run on target: cd '${REMOTE_PATH}' && docker compose up -d --build"
