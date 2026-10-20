# Bound data retention and preserve identifiable evidence corrections

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Set local login-attempt/session deadlines and cleanup while retaining separate provider and audit release gates.

## Context

Behavior analysis needs enough detail to inspect recent journeys and enough
history to explain trends. Reports, derived metrics and model inputs can copy
that detail. Keeping a stable report must not become a way to retain expired
individual histories indefinitely.

## Decision

Apply the following product retention policy, allowing customer settings to
shorten the periods:

| Data | Retention boundary |
| --- | --- |
| Detailed behavior and individual-journey report copies | 90 days. |
| Condensed metrics and aggregate report snapshots | 24 months. |
| Local browser login attempts | Ten minutes; one use, then removed. |
| Local browser sessions | Thirty-minute absolute validity; removed on logout or expiry cleanup. |

Reports must not preserve expired journeys or individual actor identifiers as
long-lived copies. Derived data, report/model detail and job payloads follow
the applicable lifecycle; snapshot stability does not override required deletion.
Expired local authentication rows are denied independently of physical cleanup;
cleanup runs on application startup and every minute while storage is available.
Identity links remain only for the operator-managed access association. Set
separate purpose-specific periods for consent evidence, provider accounts/logs,
backups, and configuration audit before live release; those periods are not
selected here. See [ADR 0010](0010-external-identity-and-workspace-grants.md).

Recorded event contents are not freely editable through admin or MCP. Support
recorded exclusions and targeted deletion as distinct actions. Recompute affected
retained metrics where needed, identify report corrections and preserve dataset
revisions so an operator can explain a changed result.

Run expiry, corrections and related background work from durable database state
with retry-safe work identities and checkpoints. A process timer alone is not
the authority. Before a restored database serves data, reapply expiry and
exclusion/deletion obligations; restore must not reactivate prohibited detail.
The [implementation plan](../plans/analytics-service.md) owns job mechanics and
export/restore procedures.

## Consequences

Recent behavior remains inspectable while longer-term reports depend on
condensed evidence. Some analyses cannot be reconstructed from raw events after
expiry. Corrections and deletion may change previously stored report results;
these changes must remain identifiable rather than silently rewriting history.
Durable lifecycle work is part of correctness, including after restart or restore.

## Validation and review triggers

Check expiry boundaries and shorter customer settings across events, derived
copies and reports. Retry correction jobs and restore expired/excluded fixtures;
verify counts remain correct and removed detail stays unavailable. Revisit the
policy when a new stored copy, purpose or retention requirement is introduced.

## References

- [Product brief](../discovery/analytics-service-brief.md)
- [Privacy and recognition](0006-privacy-consent-and-recognition.md)
- [Reports and access](0008-proactive-reports-and-internal-mcp.md)
