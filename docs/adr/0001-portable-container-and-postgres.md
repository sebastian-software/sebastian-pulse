# Use a portable container application with standard PostgreSQL

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Keep the analytics container portable while permitting a separate identity provider.

## Context

The service starts at a small collection volume and must remain movable between
container runners and compatible PostgreSQL hosts. A separate edge scheduler or
provider-specific data API would add dependencies without a demonstrated need.

## Decision

Run the analytics service as one long-lived container application backed by
ordinary PostgreSQL, initially on Bunny Magic Containers and PlanetScale.
Keep event meaning, permissions, scheduled work, and reporting independent of
provider SDKs so hosting and database providers can change without replacing
business logic. Require export/restore evidence before live use. A separate, including managed,
identity provider is allowed under [ADR 0010](0010-external-identity-and-workspace-grants.md);
its portability and data-location limits need separate validation.

[ADR 0003](0003-rust-backend.md) selects Rust for the backend within this deployment
boundary. The [implementation plan](../plans/analytics-service.md) owns starting
configuration, delivery order, and acceptance evidence; the
[product brief](../discovery/analytics-service-brief.md) owns required behavior.

## Consequences

One application avoids a separate scheduling service at launch. The application
still owns durable jobs, reconnect behavior, credential separation, and planned
migration cutovers. Standard protocols make a move practical but do not prove
provider compatibility or make cutovers interruption-free.

## Validation and review triggers

The local foundation is implemented in Rust. Live provider deployment and the
full export/restore rehearsal remain pending. Verify the same application image
against ordinary PostgreSQL and the actual target before live collection. Revisit the
boundary when measured workload or operational requirements justify a split.
