# Architecture decisions

ADRs are living, repository-owned documents written in US English. Each file
contains the current decision for one topic; Git preserves its previous versions.
Read accepted ADRs before changing the affected architecture. An accepted target
is not proof that its implementation or deployment is complete.

| ID | Decision | Status | Implementation |
| --- | --- | --- | --- |
| 0001 | [Portable container and PostgreSQL](0001-portable-container-and-postgres.md) | accepted | Local TypeScript foundation exists; Rust port and live deployment pending. |
| 0002 | [Living architecture decisions](0002-living-architecture-decisions.md) | accepted | This index, template, and repository instructions establish the convention. |
| 0003 | [Rust backend](0003-rust-backend.md) | accepted | Target architecture; foundation port pending. |

Follow [ADR 0002](0002-living-architecture-decisions.md) when creating or updating
a record. Start with the [template](template.md), assign the next unused
four-digit number, and add the record here. Keep each topic's filename stable.

The [product brief](../discovery/analytics-service-brief.md) owns requirements;
the [implementation plan](../plans/analytics-service.md) owns delivery work;
ADRs own architectural direction and rationale. Executable settings and commands
remain in code, configuration, the root README, and operations guides.
