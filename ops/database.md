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
PS-5 instance before deployment; the local test is not proof of provider permissions.

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

Migration 000002 adds workspaces, principals, memberships, projects, goals,
recipients, and minimal configuration audit records. It replaces the foundation's
future-table DML defaults with explicit grants. New application tables need their
own deliberate runtime privileges and tenant policies.

Tenant policies use transaction-local `analytics.actor_id` and
`analytics.workspace_id` settings and a fixed-search-path membership helper.
Missing context grants no tenant access. RLS trusts the backend's authenticated
actor assertion; stolen runtime credentials can impersonate that context and must
be treated as a compromise. The runtime must never own the tenant tables or the
membership helper, assume the migrator role, or bypass RLS.

The runtime can read only its own contextual membership and cannot change global
principals, workspaces, or membership grants. Initial provisioning uses the trusted
operator/migrator connection, outside the running service. OIDC login and exact
`(issuer, subject)` identity links are implemented; do not map a claimed email or
HTTP-supplied UUID to an operator grant. Invitations, audited membership
management, and live analytics retention remain later work. Test fixtures
provision synthetic UUID identities only.

Configuration mutations use `WorkspaceStore` transactions, including their audit
insert. Audit tables exclude recipient email and configuration contents; runtime
UPDATE/DELETE/TRUNCATE are not granted. A project deletion removes its goals and
records their deletions too. Raw runtime SQL has DML privileges within its RLS
scope, so the atomic audit invariant is enforced by shared application operations,
not a database trigger. Keep HTTP and MCP on those operations.

## Verified PlanetScale setup

The production `sebastian-pulse` cluster/main branch contains the logical database
`pulse` as of October 21, 2026. Ordinary SQL-created `analytics_migrator` and
`analytics_app` logins and the non-login `analytics_runtime` group implement the
same portable roles as the local fixture. SQL-created roles do not appear in
PlanetScale's managed-role inventory. Their connection usernames include the
PlanetScale branch suffix; the SQL role names above do not.

The migrator owns the database, application schema, and migrated objects. The
runtime cannot assume the migrator role, create schemas/databases/roles or
objects, use temporary tables, modify migration history, or bypass RLS. Public
schema CREATE and PUBLIC database access are revoked. Only the runtime URL is
injected into Bunny. Both connection URLs are concealed fields in the company
1Password item `Sebastian Pulse - Production`. The bootstrap administrator has
a 24-hour lifetime and is outside the application environment.

All four migrations ran successfully using the tested native WorkOS image in
[bunny/release.workos.json](bunny/release.workos.json). The actual server reports
PostgreSQL 18.6. A libpq connection with `sslmode=verify-full` and the system trust
store reports TLS 1.3; the same image's migrator and the live Bunny readiness
check also succeeded with `DATABASE_TLS=verify-full`. Check client connection
information for the TLS session: `pg_stat_ssl` on the backend behind the provider
proxy reported no backend SSL and is not evidence of the client's transport.

A schema-only `pg_dump` from PlanetScale was restored into isolated ordinary
PostgreSQL 18.6, preserving schema ownership, seven RLS tables, and restricted
runtime grants. No production rows were exported. A synthetic local workspace
remained invisible to the runtime without context. A complete data-bearing
backup/restore and recovery-time rehearsal has not been performed. Keep that
separate from the successful schema portability check.

## Apply a release

Add the next consecutive numbered SQL file after the current migration set. Never edit an applied
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
  ghcr.io/sebastian-software/sebastian-pulse@sha256:REPLACE_WITH_TESTED_DIGEST \
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
