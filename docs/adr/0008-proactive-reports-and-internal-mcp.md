# Deliver proactive browser reports and reserve MCP for internal operators

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-21
- Change note: Record the successful local WorkOS browser/MCP proof and keep production authentication and report delivery gates explicit.

## Context

Neither internal operators nor customers should need routine dashboard
inspection to obtain value. Internal operators still need flexible follow-up
analysis and configuration. Customers initially need simpler monthly reports
rather than their own MCP client or analytics configuration portal.

## Decision

- Generate weekly internal trend reports and monthly customer reports. Add
  targeted internal anomaly notices when baseline evidence is sufficient, with
  comparison periods and collection-quality limits. Deduplicate repeated notices.
- Notify by email and present visualizations in an authorized browser report.
  Configure recipients and optional per-customer approval; otherwise scheduled
  delivery is automatic. Email carries the notification/link.
- Require named report recipients to verify their email. A forwarded URL alone
  grants no access. Retained verified sessions are acceptable, and revocation
  must remove access. Report-only recipients receive no internal event/admin
  access merely by being recipients.
- Provide internal MCP tools for trends, engagement, goals, journeys, sources,
  campaigns and comparisons, plus configuration management for projects, goals,
  recipients and included/later-priority capabilities as they are delivered.
  Expose bounded tools with separate read/write scopes rather than arbitrary SQL.
- Provide internal searchable/filterable event/journey tables and configuration
  CRUD. MCP and admin use the same permission and mutation operations, with
  recorded changes. Workspaces are isolated; cross-workspace analysis requires
  explicit grants for every selected workspace.
- AI behavior analysis is P1. Explain supported observations and uncertainty;
  missing source data must not silently become a product trend. Display units,
  periods, source freshness/coverage, recognition method and dataset revision.

Store structured report results and revisions as snapshots. Opening a report
reads its stored results rather than regenerating model output. Corrections and
retention follow the [data lifecycle](0007-data-lifecycle-and-evidence.md).
AI evidence must satisfy the [privacy decision](0006-privacy-consent-and-recognition.md).
Captured text and model outputs do not grant permissions or control recipients
or external writes. [ADR 0010](0010-external-identity-and-workspace-grants.md)
defines the managed WorkOS identity and permission boundary. Existing configuration
operations and RLS support WorkOS-managed authority as a request-scoped context;
real WorkOS staging browser login and scoped MCP reads, writes, and refresh passed
against isolated local data. Production authentication remains pending. The prior
OIDC/local-grant path remains an explicitly selected legacy mode. Model and mail integrations
remain selections to validate during implementation; the [Rust backend](0003-rust-backend.md)
decision does not settle them.

## Consequences

The service delivers value without active dashboard users while keeping internal
investigation possible. Scheduled analysis adds work for evidence quality, cost
bounds, failures and notification delivery. Simpler customer access deliberately
limits initial self-service. Stored reports are reproducible within their stated
revision and lifecycle, not permanent archives of individual journeys.

## Validation and review triggers

Exercise scheduled/retried delivery, missing sources and model failures. Verify
forwarded-link, revoked-recipient, read/write-scope and cross-workspace cases.
Compare internal table and MCP results for the same evidence. Revisit the customer
interface only when a concrete need justifies more self-service or customer MCP.

## References

- [Product scope](0004-product-scope-and-priorities.md)
- [Product brief](../discovery/analytics-service-brief.md)
- [Implementation plan](../plans/analytics-service.md)
