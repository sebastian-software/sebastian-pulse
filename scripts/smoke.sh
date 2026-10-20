#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

if [[ -z "${SMOKE_BASE_URL:-}" ]]; then
  printf '%s\n' 'Set SMOKE_BASE_URL to the running service URL.' >&2
  exit 2
fi

exec cargo run --locked --bin sebastian-analytics -- smoke
