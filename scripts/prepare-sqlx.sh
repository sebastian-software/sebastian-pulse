#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -z "${DATABASE_URL:-}" ]]; then
  printf '%s\n' 'Set DATABASE_URL to a schema-ready PostgreSQL database before preparing SQLx metadata.' >&2
  exit 2
fi
if ! cargo sqlx --version >/dev/null 2>&1; then
  printf '%s\n' 'Install the pinned SQLx CLI 0.9.0 before preparing metadata.' >&2
  exit 2
fi

export SQLX_OFFLINE=false
exec cargo sqlx prepare -- --locked --all-targets --features integration-tests
