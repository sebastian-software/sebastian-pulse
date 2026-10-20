#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

export SQLX_OFFLINE=true

cargo fmt --all -- --check
cargo clippy --locked --all-targets --features integration-tests -- -D warnings
cargo test --locked --lib
cargo build --locked
