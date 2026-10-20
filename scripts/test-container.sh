#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
compose_file="$repo_root/compose.yaml"
project="sebastian-analytics-test-$$-${RANDOM}-${RANDOM}"

if [[ -z "${ANALYTICS_IMAGE:-}" ]]; then
  printf '%s\n' 'Set ANALYTICS_IMAGE to the image that this container check should exercise.' >&2
  exit 2
fi

export APP_PORT=0
export POSTGRES_PORT=0
export ANALYTICS_IMAGE

compose() {
  docker compose --project-directory "$repo_root" -f "$compose_file" -p "$project" "$@"
}

cleanup() {
  local exit_code=$?
  if (( exit_code != 0 )); then
    compose logs --no-color || true
  fi
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  return "$exit_code"
}
trap cleanup EXIT

compose up -d --no-build --wait --wait-timeout 90

address="$(compose port app 3000 | tail -n 1)"
port="${address##*:}"
if [[ -z "$port" || "$port" == "$address" ]]; then
  printf '%s\n' 'Could not resolve the isolated application port.' >&2
  exit 1
fi
base="http://127.0.0.1:$port"

http_status() {
  curl --silent --output /dev/null --max-time 5 --write-out '%{http_code}' "$base$1" 2>/dev/null || true
}

wait_for_status() {
  local path="$1"
  local expected="$2"
  local deadline=$((SECONDS + 30))
  local actual
  while (( SECONDS < deadline )); do
    actual="$(http_status "$path")"
    if [[ "$actual" == "$expected" ]]; then
      return 0
    fi
    sleep 0.2
  done
  printf 'Expected %s to return HTTP %s; last response was %s.\n' "$path" "$expected" "$actual" >&2
  return 1
}

wait_for_status /health/ready 200
[[ "$(http_status /health/live)" == 200 ]]

migration_container="$(compose ps -a -q migrate)"
if [[ -z "$migration_container" ]]; then
  printf '%s\n' 'The one-shot migration container was not created.' >&2
  exit 1
fi
migration_exit="$(docker inspect --format '{{.State.ExitCode}}' "$migration_container")"
if [[ "$migration_exit" != 0 ]]; then
  printf 'The one-shot migration container exited with %s.\n' "$migration_exit" >&2
  exit 1
fi
[[ "$(compose exec -T app id -u | tr -d '\r\n')" == 1000 ]]
if compose exec -T app /bin/sh -c 'test -z "${DATABASE_MIGRATION_URL+x}"'; then
  :
else
  printf '%s\n' 'The application container received DATABASE_MIGRATION_URL.' >&2
  exit 1
fi

compose stop postgres
[[ "$(http_status /health/live)" == 200 ]]
wait_for_status /health/ready 503
compose start postgres
wait_for_status /health/ready 200

compose stop -t 25 app
app_container="$(compose ps -a -q app)"
app_exit="$(docker inspect --format '{{.State.ExitCode}}' "$app_container")"
if [[ "$app_exit" != 0 ]]; then
  printf 'The application exited with status %s after SIGTERM.\n' "$app_exit" >&2
  exit 1
fi
logs="$(compose logs --no-color app)"
if [[ "$logs" != *'"event":"shutdown_complete"'* ]]; then
  printf '%s\n' 'The application did not log shutdown_complete.' >&2
  exit 1
fi
if [[ "$logs" =~ local-runtime-only|local-migration-only|postgresql:// ]]; then
  printf '%s\n' 'Application logs contain a database credential or URL.' >&2
  exit 1
fi

printf '%s\n' 'Container check passed: fresh migrations, UID 1000, no app migration credential, database outage and recovery, clean SIGTERM, sanitized logs.'
