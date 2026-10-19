#!/usr/bin/env bash
# Recreate the pristine, read-only upstream reference at the pinned commit.
# Do not modify anything under reference/twenty.
set -euo pipefail

UPSTREAM_URL="https://github.com/twentyhq/twenty.git"
PINNED_COMMIT="08a0e5f1ee2e7db7c1e6ef3aeec9bcef731f4e98"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="${ROOT}/reference/twenty"

if [ -d "${DEST}/.git" ]; then
  echo "Reference already present at ${DEST}"
  git -C "${DEST}" fetch --depth 1 origin "${PINNED_COMMIT}" || true
  git -C "${DEST}" checkout "${PINNED_COMMIT}"
else
  mkdir -p "${ROOT}/reference"
  git clone --no-checkout "${UPSTREAM_URL}" "${DEST}"
  git -C "${DEST}" fetch --depth 1 origin "${PINNED_COMMIT}"
  git -C "${DEST}" checkout "${PINNED_COMMIT}"
fi

echo "Reference ready at ${DEST} @ ${PINNED_COMMIT}"
