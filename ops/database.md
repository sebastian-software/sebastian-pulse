# Database operations

## Roles and ownership

The local bootstrap creates `analytics_migrator` (login/schema owner),
`analytics_runtime` (non-login data privilege group), and `analytics_app`
(restricted login inheriting the runtime group). Both logins are non-superusers,
cannot create databases/roles, and cannot bypass RLS. The public schema is not
writable. Runtime table privileges exclude DDL, ownership, and TRUNCATE.

The bootstrap under `ops/local/` is a **local fixture**, not a production setup
script. On PlanetScale or another host, an operator must establish equivalent
ownership and grants using that host's supported role-management path, with fresh
secrets and a connection to the intended database. Confirm this path on the actual
PS-10 instance before deployment; the local test is not proof of provider permissions.

Required SQL relationships, with the actual login names substituted by an operator:

```sql
-- Run as an authorized database/role administrator after creating the logins.
CREATE ROLE analytics_runtime NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS;
GRANT analytics_runtime TO app_login;
REVOKE ALL ON SCHEMA public FROM PUBLIC;
CREATE SCHEMA analytics AUTHORIZATION migration_login;
```

The app login needs CONNECT to the database and membership in `analytics_runtime`.
The migration login needs CONNECT and schema ownership, with no grant to the app
login to assume that role. Revoke PUBLIC database permissions where supported and
appropriate for the dedicated database. The migrator establishes runtime schema/
table grants. Database credentials are server secrets; never embed them in SDKs,
browser assets, images, logs, or repository files.

This package does not create tenant tables or policies; tenant isolation is W2.
Restricted credentials are a prerequisite, not a claim of completed isolation.

## Apply a release

Add a consecutive `migrations/000002_description.sql` file. Never edit an applied
file. SQL files must not contain transaction control or commands that cannot run
inside a transaction, such as `CREATE INDEX CONCURRENTLY`. All pending files and
their history records commit together under a transaction-scoped advisory lock.
The lock and statements are bounded by server timeouts. On failure the transaction
rolls back and the command exits nonzero. Applied filenames and SHA-256 checksums
must match this release's prefix; unknown, removed, or edited history is rejected.

Run migrations once per release, **before** routing traffic to the new image:

```sh
# Environment is supplied by the release runner's secret store.
docker run --rm --env DATABASE_MIGRATION_URL --env DATABASE_CA_CERT \
  ghcr.io/sebastian-software/analytics-service@sha256:REPLACE_WITH_TESTED_DIGEST \
  migrate
```

TLS verification is enabled without any additional setting. Do not pass the
migration URL to the ordinary application container. The application never runs
migrations automatically. Plan backward-compatible expand/migrate/contract changes
before rolling a live instance; an older image intentionally rejects unexpected
migration history. A maintenance window is the current safe release procedure
until an explicit compatible-schema-range contract is implemented.

If a migration fails, inspect the failed release and SQL using authorized operator
tools. Logs contain only stable error codes, never full database errors. Do not
repair history by changing checksums to hide drift. Correct an unapplied migration
or add a new forward migration after understanding the actual schema state.

## Change providers

Keep the image and application contracts unchanged. Recreate equivalent roles,
match the PostgreSQL major, and use a normal verified TLS connection URL. There
is no required PlanetScale extension, HTTP driver, branch API, or Bunny runtime SDK.

Before live data, rehearse `pg_dump`/`pg_restore` to an independent ordinary
PostgreSQL instance with synthetic data. A per-database dump does not create cluster
roles or transfer provider backup history. Restore schema ownership and grants
deliberately. For a real cutover, stop all writers, export/restore, verify data and
permissions, change runtime configuration, and resume. Keep the source frozen;
after new writes on the target, do not switch back without reconciling them.
See W9 in the plan for auth, job, report, expiry, and deletion checks once those
features exist. This package has not performed the production exit rehearsal.
