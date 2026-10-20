# Implement the backend in Rust

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Port the foundation to Rust, preserving the existing SQL ledger and adding offline query checks.

## Context

The service receives website and CLI events, stores them durably, and exposes
analytics through MCP and browser reports. The initial forecast is about 1,000
pageviews per day across the portfolio. The later planning ceiling is 200,000
pageviews per day, before additional events and traffic bursts.

The team is equally comfortable with Rust and TypeScript. Rust is preferred
for frequent HTTP collection, predictable resource control, and a long-lived
processing core. Experience with lower Rust HTTP response times informs this
choice; no representative Rust-versus-TypeScript benchmark has been run for
this service, and the forecast alone does not establish a TypeScript bottleneck.

At the language decision, the repository contained only a TypeScript foundation
for startup, migrations, health checks, shutdown, and CI. That foundation is now
ported to Rust. Collection, authentication, MCP, and reports remain later work.
Changing languages before those capabilities existed kept the migration bounded.

## Decision

Implement the complete backend in Rust as one application image and one service
process. Use these working library choices, with versions and compatibility
verified and pinned during implementation:

| Responsibility | Choice |
| --- | --- |
| HTTP routing and asynchronous execution | Axum and Tokio |
| PostgreSQL access | SQLx over verified TLS; versioned SQL migrations remain the schema owner. |
| MCP transport and tools | The official Rust SDK, `rmcp`. |
| Aggregation, durable jobs, reports, and external integrations | Rust modules using shared application operations and permissions. |
| Browser SDK, consent interface, admin and report UI | TypeScript browser code; compiled assets served by the Rust application. |

Use SQLx's checked query macros for static SQL where applicable. Prepare offline
query metadata against the migrated test schema and verify it in CI; builds do
not need a production database. Dynamic SQL and PostgreSQL nullability edge cases
still require deliberate typing and real-database checks.

Keep the [portable-container decision](0001-portable-container-and-postgres.md):
Bunny Magic Containers and PlanetScale PostgreSQL remain the initial targets,
with standard HTTP/container and PostgreSQL boundaries. No separate TypeScript
backend, broker, or worker service is introduced by this decision. Background
work is authoritative in PostgreSQL rather than process-local timers.

Use versioned wire contracts between Rust and browser code. Select type/schema
generation during implementation and verify that generated types match the
actual JSON representation, including large integers, decimals, and nullability.

Authentication and MCP authorization need a maintained solution compatible with
the Rust backend. The earlier Better Auth working choice no longer applies.
Keep domain-owned workspace/report grants and the owned issuer domain; do not
implement a custom OAuth authorization server. Record the auth selection before
shipping the affected capability. The MCP SDK alone is not user management.

## Consequences

One backend language keeps collection, analysis, and permissions in the same
application. Rust offers explicit ownership and concurrency controls, while
SQL remains suitable for analytics and portable PostgreSQL operations.

The port preserves the foundation's migration-history contract, credential
separation, TLS verification, database/probe timeouts, readiness, shutdown, and
CI checks. Applied SQL files and `analytics.schema_migrations` remain unchanged;
SQLx's migration ledger is not introduced. Browser/backend types need an explicit shared contract, and
auth and provider integrations need Rust-compatible implementation choices.

Retaining TypeScript was viable and would preserve the current foundation.
A Rust collector with a TypeScript backend was also viable, but would add a
process boundary and separate deployment/auth coordination without a current
team-language advantage. Neither alternative is selected.

## Validation and review triggers

The foundation retains real PostgreSQL and container failure checks. SQLx
metadata is checked against a disposable migrated schema, and normal builds are
offline. The Rust migrator also accepts the existing TypeScript ledger without
reapplying migrations. Next exercise validation, deduplication, and
commit-before-success when implementing collection. Measure latency percentiles, CPU, memory, and
database pressure with representative batches and concurrent queries. No
specific speedup, cost saving, or future capacity is asserted by this ADR.

Revisit a process split or language boundary only when measured workload,
failure isolation, or a concrete integration requirement justifies its cost.
Auth compatibility remains a release gate. The [implementation plan](../plans/analytics-service.md)
owns sequencing and completion evidence.

## References

- [Axum](https://github.com/tokio-rs/axum)
- [SQLx query checking](https://docs.rs/sqlx/latest/sqlx/macro.query.html)
- [SQLx offline metadata and CI checks](https://github.com/transact-rs/sqlx/blob/main/sqlx-cli/README.md)
- [Official Rust MCP SDK](https://github.com/modelcontextprotocol/rust-sdk)
