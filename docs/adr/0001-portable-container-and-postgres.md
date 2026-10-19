# Use a portable container application with standard PostgreSQL

Status: accepted for implementation, 2026-10-20; not yet deployed.

Run the analytics service as one long-lived container application backed by
ordinary PostgreSQL, initially on Bunny Magic Containers and PlanetScale.
Keep event meaning, permissions, scheduled work and reporting independent of
provider SDKs so hosting and another compatible Postgres host can be changed
without replacing business logic, and require export/restore evidence before
live use. This avoids a separate edge scheduling service at the small launch
volume, while retaining responsibility for durable jobs, reconnects and planned
migration cutovers.

The [implementation plan](../plans/analytics-service.md) owns the selected starting
configuration, delivery order and acceptance evidence; the
[product brief](../discovery/analytics-service-brief.md) owns the required behavior.
