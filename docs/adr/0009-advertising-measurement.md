# Combine first-party campaign attribution with advertising reports

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Record UTM attribution, initial advertising reporting, and the later conversion-export boundary.

## Context

Operators need to understand where interest comes from and whether advertising
leads to useful outcomes. Website observations, ad-platform reporting and
inferred recognition describe different evidence; combining them must not turn
an estimate into proof that an advertisement caused a conversion.

## Decision

Include standard UTM capture, configurable conversion goals, ad costs/delivery
metrics and outcome comparisons in P1. Initial reporting integrations cover
Google Ads, Meta/Instagram and LinkedIn. Build read-only reporting adapters;
actual account/API access and platform approval remain prerequisites for each
live integration.

Use a 30-day attribution window with first-touch and last-known-non-direct
models. A direct return preserves eligible preceding non-direct campaign
credit within the window. Keep estimated recognition and unavailable history
explicit; a model allocates credit rather than proving causation.

Keep account/campaign identity and UTM mapping explicit. Preserve source timezone,
currency, reporting period, import time and source-data freshness. Handle
reimports and source corrections without double-counting spend. Show unmatched
campaigns, stale sources and unavailable accounts; do not force platform-reported
conversions to equal first-party observed goals or combine currencies without
an explicit conversion policy.

Make campaign costs and observed outcomes available to internal MCP, admin and
proactive reports. Reporting cadence and access follow the
[reporting decision](0008-proactive-reports-and-internal-mcp.md).

Conversion exports to advertising networks are P2 and require the applicable
purpose/disclosure permissions. P1 reporting does not authorize advertising
writes or bypass the [privacy decision](0006-privacy-consent-and-recognition.md).
Exact platform adapters, account approvals, credential setup and import behavior
are implementation work, not already available capabilities.

## Consequences

The product can compare campaign delivery and cost with first-party outcomes
while preserving discrepancies. Provider access and metric definitions add
integration work; a missing provider need not prevent the basic collection
pilot. Attribution quality depends on recorded contacts, permissions and the
recognition method, and those limits must accompany effectiveness statements.

## Validation and review triggers

Use known UTM journeys to check both attribution models, direct returns and
window expiry. Reimport the same reporting period and compare authorized live
reads with stored values. Verify missing pages/accounts, source corrections,
timezones and currency boundaries remain visible. Revisit attribution or export
scope only when the product decision and applicable permissions change.

## References

- [Product scope](0004-product-scope-and-priorities.md)
- [Product brief](../discovery/analytics-service-brief.md)
- [Implementation plan](../plans/analytics-service.md)
