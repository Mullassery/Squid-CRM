#!/usr/bin/env bash
# Heuristic guard for the reference firewall (see FIREWALL.md).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGETS=("$ROOT/crates")

FORBIDDEN=(
  '@nestjs/'
  'typeorm'
  '/* @license Enterprise */'
  'twenty-server/src/'
  'twenty-front/src/'
)

status=0
for pattern in "${FORBIDDEN[@]}"; do
  if grep -rn --include='*.rs' -F "$pattern" "${TARGETS[@]}" >/dev/null 2>&1; then
    echo "FIREWALL: found forbidden upstream marker: $pattern"
    grep -rn --include='*.rs' -F "$pattern" "${TARGETS[@]}" | head -20
    status=1
  fi
done

if [ "$status" -eq 0 ]; then
  echo "firewall check passed"
fi
exit "$status"
