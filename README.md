# Analytics service

A portable foundation for the analytics service described in the
[product brief](docs/discovery/analytics-service-brief.md). The initial deployment
target is Bunny Magic Containers with PlanetScale Postgres **PS-10 ARM, single
node, Frankfurt**. The application uses standard HTTP and PostgreSQL protocols.

Package 1 provides container boot, database roles, SQL migrations, health checks,
shutdown, and CI. Collection, workspace isolation, authentication, MCP, reports,
and background jobs are subsequent packages in the [plan](docs/plans/analytics-service.md).
No live analytics data is collected by this foundation.

## Run locally

Install Node **24.22.0**, pnpm **11.25.0**, and Docker with its Compose plugin.
PostgreSQL **18.6** runs in Compose; no host PostgreSQL installation is needed.

```sh
pnpm install --frozen-lockfile
docker compose up --build --wait
pnpm smoke
```

The API listens at `http://127.0.0.1:3000`. The database listens only on
`127.0.0.1:55432`; local credentials in `compose.yaml` are disposable fixtures.
Compose initializes restricted roles, runs a one-shot migrator, and starts the
application only after migration success. The app has no migration credential.
The same image runs both processes with different commands and credentials.

To run the application on the host with watch mode instead:

```sh
cp .env.example .env
cp .env.migration.example .env.migration
docker compose up -d --wait postgres
pnpm db:migrate
pnpm dev
```

Do not run both app modes on the same port. Change `APP_PORT` / `POSTGRES_PORT`
for Compose or `PORT` / database URLs for host development when necessary.
`docker compose down` keeps database data; adding `--volumes` deletes that local
database. Bootstrap SQL runs only on a fresh volume.

## Check the foundation

```sh
pnpm check
cp .env.test.example .env.test
docker compose up -d --wait postgres
pnpm test:integration
docker compose build
pnpm test:container
```

`check` runs typechecking, focused unit tests, and compilation. Integration tests
require the disposable **loopback** database administrator URL in `.env.test` and
create/drop databases with generated names. They verify transactional migration
rollback, concurrent migration locking, immutable history, restricted runtime
permissions, synthetic data operations, and HTTP readiness through a database
outage and recovery. They fail when the database URL is absent; they do not skip.

The container test creates its own Compose project and volumes on random host
ports, checks fresh boot, non-root execution, absence of migration credentials,
database stop/restart, and SIGTERM exit. It removes only that generated project.
Set `ANALYTICS_IMAGE` to test an already built image. CI builds `linux/amd64`, runs
these checks, and publishes the exact tested image to GHCR with its commit SHA.
Use the recorded image digest for deployment; CI does not deploy to Bunny.

## Runtime contract

| Setting | Contract |
| --- | --- |
| `DATABASE_URL` | Required PostgreSQL runtime login. No inline SSL options; no schema ownership or RLS bypass. |
| `DATABASE_TLS` | `verify-full` by default. `disable` requires explicit `APP_ENV=local` or `test`. |
| `DATABASE_CA_CERT` | Optional PEM CA for a host whose CA is not in the system trust store. Verification stays enabled. |
| `PORT` | HTTP port, default `3000`; binds to `0.0.0.0` inside the container. |
| `RELEASE_ID` | Safe release identifier, default `local`; set to the commit SHA at image build time. |
| `DATABASE_MIGRATION_URL` | **Migrator only.** The application refuses to start if this is present. |

The runtime has a pool of at most five connections, a two-second connection
acquisition timeout, two-second server statement timeout, and three-second driver
query timeout. Idle connection failures are handled without logging credentials
or driver messages. Requests are not automatically replayed; a later request can
open a new connection after recovery. There is no hidden retry of data writes.

`GET /health/live` checks the process; `GET /health/ready` also checks PostgreSQL
18, the complete migration checksums, and restricted role permissions. Both return
the release ID and disable caching. Readiness returns `503` on database/schema/
role failures. Concurrent readiness requests share one database probe.
`SIGTERM` stops new work, drains HTTP, and closes the pool within a 20-second
deadline. Other routes return `404`.

## Migrations and the deployment target

Read [database operations](ops/database.md) before adding SQL or configuring a
remote database. Read [Bunny deployment](ops/bunny/README.md) for the target
container settings and outstanding live-environment checks.

PostgreSQL 18 is the selected local compatibility baseline. Before provisioning
PS-10, confirm its supported major and match it here if necessary. No database
subscription, production secret, or Bunny application is created by this package.
The actual Bunny-to-PlanetScale TLS/permission path and full provider exit rehearsal
remain release gates before live collection.
