#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

export SQLX_OFFLINE=true

if [[ -z "${TEST_DATABASE_ADMIN_URL:-}" ]]; then
  printf '%s\n' 'Set TEST_DATABASE_ADMIN_URL to a disposable loopback PostgreSQL admin URL ending in /postgres.' >&2
  exit 2
fi

case "$TEST_DATABASE_ADMIN_URL" in
  postgres://*|postgresql://*) ;;
  *) printf '%s\n' 'TEST_DATABASE_ADMIN_URL must use postgres:// or postgresql://.' >&2; exit 2 ;;
esac

url_authority="${TEST_DATABASE_ADMIN_URL#*://}"
authority_hostport="${url_authority##*@}"
authority_hostport="${authority_hostport%%/*}"
database_path="${url_authority##*/}"
database_path="${database_path%%\?*}"
case "$authority_hostport" in
  127.0.0.1|127.0.0.1:*|localhost|localhost:*|\[::1\]|\[::1\]:*) ;;
  *) printf '%s\n' 'TEST_DATABASE_ADMIN_URL must connect through a loopback host.' >&2; exit 2 ;;
esac
if [[ "$database_path" != postgres ]]; then
  printf '%s\n' 'TEST_DATABASE_ADMIN_URL must target the /postgres maintenance database.' >&2
  exit 2
fi

exec cargo test --locked --features integration-tests --test foundation --test workspaces --test auth_store --test authentication -- --test-threads=1
