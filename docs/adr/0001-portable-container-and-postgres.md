# Use a shared, portable container application with standard PostgreSQL

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Select Frankfurt PS-5 HA for the shared Auth/Pulse cluster; keep portable, isolated logical databases and future dedicated deployments.

## Context

The service must handle growth beyond its small launch forecast while remaining
movable between container runners and compatible PostgreSQL hosts. Most customers
need proactively delivered insights rather than an analytics installation to
operate. Keeping deployment, upgrades, migrations, monitoring, and recovery
centralized is therefore an important operating goal.

Separate customer installations with embedded
[Hiqlite/SQLite](https://github.com/sebadob/hiqlite) were considered.
They offer local data access, independent recovery, and customer-level resource
isolation. Making them the default would also require managing many deployments
and database lifecycles. A separate database per customer would not require a
separate service process, but would still multiply migration and recovery work.
The database choice is an operational decision, not a measured capacity verdict
against Hiqlite or a guarantee of PostgreSQL throughput.

## Decision

Run one shared, long-lived Rust analytics application backed by ordinary
PostgreSQL. Multiple analytics workspaces use the same deployment and database;
application permissions, workspace-scoped relationships, PostgreSQL row level
security, and restricted runtime credentials enforce the tenant boundary.
Do not deploy a service or database per customer by default.

Bunny Magic Containers and PlanetScale remain the initial hosting targets.
Start with PlanetScale PostgreSQL 18.6 on PS-5 ARM HA in Frankfurt: one primary
and two replicas in separate availability zones, 512 MB RAM and 1/16 vCPU per
node, 10 GB storage with an initial 10-GB cap, and $15/month before taxes.
This replaces the earlier PS-10 single-node target ($14/month). The small launch
workload makes the lower primary capacity an acceptable starting hypothesis; the
extra dollar buys database failover, not additional primary write capacity.

Auth and Pulse may share this cluster with separate logical databases, owner
roles, and credentials. The Pulse runtime remains restricted and separate from
its migrator; Auth owns only its own database. Shared compute, contention, and
cluster recovery are explicit dependencies. Database HA does not make either
single-instance application highly available. Measure collection and reporting
workloads before making a future capacity commitment.

Keep event meaning, permissions, scheduled work, and reporting independent of
provider SDKs so hosting and database providers can change without replacing
business logic. Require export/restore evidence before live use. A separate,
including managed, identity provider is allowed under
[ADR 0010](0010-external-identity-and-workspace-grants.md); its portability and
data-location limits need separate validation.

Preserve the option of a future dedicated customer installation using the same
application image, PostgreSQL schema, and migration history. It would use its
own PostgreSQL database and deployment configuration rather than a customer code
fork or a different storage engine. Keep database connections, service URLs,
identity-provider configuration, and integration credentials deployment-specific;
do not make business operations depend on the primary installation. Workspace
permissions still apply in a dedicated installation.

This option does not add dedicated-installation support to the initial product
scope. Automated provisioning, customer-facing operations, and moving an existing
workspace between installations require separate design and validation when a
concrete customer or operational need arises. Do not build a fleet manager,
per-customer database router, or generic storage-engine abstraction in advance.

[ADR 0003](0003-rust-backend.md) selects Rust for the backend within this deployment
boundary. The [implementation plan](../plans/analytics-service.md) owns starting
configuration, delivery order, and acceptance evidence; the
[product brief](../discovery/analytics-service-brief.md) owns required behavior.

## Consequences

A shared deployment keeps releases, migrations, monitoring, and recovery in one
operating path. It avoids a separate scheduling service and a default fleet of
customer installations. Strong tenant isolation remains a responsibility of the
application and PostgreSQL, and shared resources require bounded queries, jobs,
and collection workloads.

The application still owns durable jobs, reconnect behavior, credential
separation, and planned migration cutovers. Standard protocols and one reusable
image make later moves practical but do not prove provider compatibility,
dedicated-installation readiness, or interruption-free cutovers. A dedicated
customer deployment would also require its own operating and recovery setup.

## Validation and review triggers

The local Rust foundation and configuration/authentication tenant checks exist.
Live provider deployment, analytics workload measurements, and the full
export/restore rehearsal remain pending. Verify the same application image
against ordinary PostgreSQL and the actual target before live collection.
Measure collection bursts and retained data volume together with concurrent
analysis and background jobs; visitor counts alone are not a capacity test.

Before offering a dedicated installation, verify the unchanged image with an
independent database, deployment and identity configuration, tenant checks,
migrations, and backup/restore. A workspace move also needs an explicit data,
access, credential, and client-endpoint cutover plan. Revisit deployment or data
boundaries when measured contention, contractual isolation requirements, or a
concrete customer hosting need justifies the additional operations.
