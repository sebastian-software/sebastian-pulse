# Analytics service

A Rust backend for the analytics service described in the
[product brief](docs/discovery/analytics-service-brief.md). The initial deployment
target is Bunny Magic Containers with PlanetScale Postgres **PS-10 ARM, single
node, Frankfurt**. The application uses standard HTTP and PostgreSQL protocols.

The backend implements container boot, restricted database roles, SQL migrations,
health checks, shutdown, CI, and workspace configuration persistence with Axum,
Tokio, and SQLx. Projects, goals, and recipients have shared Rust operations with
membership checks, PostgreSQL RLS, version conflicts, and atomic audit records.
Login, public configuration routes, admin UI, MCP, collection, reports, and jobs
remain subsequent work in the [plan](docs/plans/analytics-service.md). The server
currently exposes only health routes; it collects no live analytics data.

## Product and architecture decisions

Read the [ADR index](docs/adr/README.md), starting with
[feature scope and priorities](docs/adr/0004-product-scope-and-priorities.md).
The records cover the Rust backend, observation design, privacy/recognition,
data lifecycle, reporting/access, and advertising. Follow the
[living ADR convention](docs/adr/0002-living-architecture-decisions.md) when a
decision changes. All repository work uses US English. Future browser SDK and
UI code remain TypeScript; the backend no longer needs Node or pnpm.

## Run locally

Install Docker with its Compose plugin. For host development and verification,
install Rustup; `rust-toolchain.toml` pins Rust **1.97.1**, Rustfmt, and Clippy.
Compose runs pinned PostgreSQL **18.6** without a host PostgreSQL installation.

```sh
docker compose up --build --wait
docker compose exec -T app analytics-service smoke
```

The API listens at `http://127.0.0.1:3000`; PostgreSQL is bound to
`127.0.0.1:55432`. Local credentials in `compose.yaml` are disposable fixtures.
Compose initializes restricted roles, runs the one-shot migrator, and starts
the app after migration success. The same image runs both commands with
separate credentials; the app receives no migration credential.

For host development, start PostgreSQL, migrate, then run the app:

```sh
docker compose up -d --wait postgres
APP_ENV=local DATABASE_TLS=disable \
  DATABASE_MIGRATION_URL=postgresql://analytics_migrator:local-migration-only@127.0.0.1:55432/analytics \
  cargo run --locked -- migrate
APP_ENV=local DATABASE_TLS=disable \
  DATABASE_URL=postgresql://analytics_app:local-runtime-only@127.0.0.1:55432/analytics \
  cargo run --locked -- serve
```

Use one app mode per port. Change Compose `APP_PORT` / `POSTGRES_PORT`, or the
host `PORT` / database URLs, when necessary. The binary reads process environment
variables and does not load `.env` files automatically; the example files document
separate runtime, migration, and test settings.
`docker compose down` retains local data; `--volumes` deletes that local database.
Bootstrap SQL runs only on a fresh volume.

## Check the foundation

```sh
scripts/check.sh
docker compose up -d --wait postgres
TEST_DATABASE_ADMIN_URL=postgresql://postgres:local-admin-only@127.0.0.1:55432/postgres \
  scripts/test-integration.sh
docker build -t analytics-service:local .
ANALYTICS_IMAGE=analytics-service:local scripts/test-container.sh
```

`check.sh` runs Rustfmt, Clippy, unit tests, and a locked build using committed
SQLx metadata. Integration checks create and drop uniquely named databases on
an explicit loopback administrator connection. They cover atomic migration
rollback, concurrent migrators, history drift, restricted runtime operations,
HTTP readiness during outages/recovery, and schema-owner rejection. The dedicated
command fails when its administrator URL is absent; ordinary unit checks need
no database. Fixture URLs cannot redirect to another host/database through query
parameters.

The workspace suite additionally checks admin/reader/report-only boundaries,
workspace isolation in both operations and raw SQL, composite project/goal foreign
keys, stale and concurrent edits, minimal append-only audit records, atomic audit
failure rollback, revocation, and transaction cleanup on one reused connection.
It does not simulate a completed HTTP or MCP login.

The container check creates its own Compose project on random ports. It checks
fresh migration/startup, UID 1000, credential separation, database outage/recovery,
clean SIGTERM, and sanitized logs, then removes only that generated project.
CI builds and tests `linux/amd64`, then publishes the exact tested image to GHCR
on `main` with its commit SHA. CI does not deploy to Bunny.

For an explicit running target:

```sh
SMOKE_BASE_URL=http://127.0.0.1:3000 scripts/smoke.sh
```

## Workspace configuration boundary

`src/workspaces/` owns the operations future browser and MCP adapters will share.
Internal admins can change configuration; readers can inspect it. A recipient
record grants neither membership nor verified report access. Lists use UUID keyset
pagination with 1–100 items per page. Mutations require the current positive
revision, increment it, and record identifiers/action/revision in the same
transaction. Deleting a project also audits its deleted goals.

Actor UUIDs must come from a trusted authentication adapter, never a request field.
Each operation sets actor and workspace context only for its database transaction;
RLS checks the same membership again. The runtime cannot provision principals,
workspaces, memberships, or edit/delete the audit log. Operator bootstrap and the
remaining login/IdP work are described in [database operations](ops/database.md)
and [ADR 0010](docs/adr/0010-external-identity-and-workspace-grants.md).

## SQL query metadata

Normal Cargo and container builds use `.sqlx/` and do not connect to the runtime
database. After changing checked SQL or schema, apply migrations to the disposable
local database and regenerate its metadata:

```sh
cargo install sqlx-cli --version 0.9.0 --no-default-features --features postgres,rustls
DATABASE_URL=postgresql://analytics_migrator:local-migration-only@127.0.0.1:55432/analytics \
  scripts/prepare-sqlx.sh
DATABASE_URL=postgresql://analytics_app:local-runtime-only@127.0.0.1:55432/analytics \
  cargo sqlx prepare --check -- --locked --all-targets --features integration-tests
```

Commit query metadata with the SQL/schema change. The custom migrator preserves
`analytics.schema_migrations`; it does not use SQLx's different migration ledger.

## Runtime contract

| Setting | Contract |
| --- | --- |
| `DATABASE_URL` | Required runtime PostgreSQL login without schema ownership or RLS bypass. |
| `DATABASE_TLS` | `verify-full` by default; `disable` requires explicit `APP_ENV=local` or `test`. Inline URL SSL options are rejected. |
| `DATABASE_CA_CERT` | Optional PEM CA; certificate and hostname verification remain enabled. |
| `PORT` | HTTP port, default `3000`; binds to `0.0.0.0` in the container. |
| `RELEASE_ID` | Safe release identifier, default `local`; CI sets the commit SHA at image build. |
| `DATABASE_MIGRATION_URL` | Migrator only. The app refuses startup when it is present. |
| `MIGRATIONS_DIR` | Optional explicit migration directory; the image includes `/app/migrations`. |

The pool has at most five connections, a two-second acquire timeout, 30-second
idle timeout, and five-minute maximum lifetime. Runtime statements have a
two-second server timeout and idle transactions have a five-second timeout.
Readiness probes have a three-second deadline and overlapping probes share one
check. Writes are not automatically replayed. SQL statements and raw driver
errors are not logged.

`GET /health/live` checks the process. `GET /health/ready` additionally verifies
PostgreSQL 18, the complete migration history/checksums, and restricted runtime
permissions; it returns `503` on failure. Both include the release ID and disable
caching. Other routes return JSON `404`. SIGTERM/SIGINT stop new work, drain HTTP,
and close the pool within a 20-second deadline.

## Database and deployment

Read [database operations](ops/database.md) for schema history and credentials,
and [Bunny deployment](ops/bunny/README.md) for target settings and remaining
live checks. Before provisioning PS-10, verify its PostgreSQL major and actual
role/TLS behavior. Provider provisioning and the full export/restore rehearsal
remain gates before live collection.
