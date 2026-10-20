# Prioritize proactive analytics across websites, products, and CLI tools

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Record the agreed product scope, priorities, and initial exclusions.

## Context

The service should explain changes in audience, engagement, abandonment, and
advertising outcomes without requiring someone to inspect dashboards routinely.
It covers owned commercial and OSS product sites, agency customer projects,
future VorOrt customer projects and authenticated product areas, and CLI tools
such as Dalo and Palamedes.

The launch forecast is about 1,000 pageviews per day across the portfolio. The
later planning ceiling is 20 sites at 10,000 pageviews each per day, before
additional events. These are planning inputs, not measured traffic or a service
capacity guarantee.

## Decision

Use the following scope and priorities. P1 is the initial product target, not a
claim that the capabilities exist or must all ship in the first pilot.

| Priority | Capability | Boundary and decision owner |
| --- | --- | --- |
| P1 | Website and authenticated-product analytics | Page transitions, section visibility, active dwell, explicit actions/goals, progression and possible abandonment; [structured observations](0005-structured-observations.md). |
| P1 | CLI analytics | Commands, versions, platforms, outcomes and coarse durations with permitted system continuity; execution and update-check counts remain distinct from people. |
| P1 | AI behavior insights | Explain trends and journeys from recorded observations; preserve gaps and uncertainty. This is distinct from analyzing an application's LLM usage. |
| P1 | Campaign measurement | UTM attribution, configurable conversions, and Google, Meta/Instagram and LinkedIn reporting; [advertising decision](0009-advertising-measurement.md). |
| P1 | Proactive reports and notices | Weekly internal trends, targeted anomaly notices and monthly customer reports; email notification with a browser report. |
| P1 | Internal MCP and admin tables | Analysis, filtering and configuration CRUD with scoped read/write access; [access and delivery decision](0008-proactive-reports-and-internal-mcp.md). |
| P1 | Owned consent interface | Visitor choices plus service/purpose configuration; [privacy and recognition decision](0006-privacy-consent-and-recognition.md). |
| P1 | Traffic classification | Observed/suspected automation, AI crawlers, internal tests, CLI automation and unknown traffic; detection does not establish human identity. |
| P1 | Data lifecycle and evidence integrity | Bounded retention, recorded exclusions, deletion and identifiable report corrections; [lifecycle decision](0007-data-lifecycle-and-evidence.md). |
| P2 | Conversion exports and operational feature flags | Advertising disclosure permissions apply to exports. Flags target roughly one-minute propagation with application fallbacks; this does not imply immediate security shutdown. |
| P3 | Voluntary surveys, LLM usage analytics, external consent-manager adapters | LLM usage means model calls/tokens/cost/latency. External consent-manager integration remains P3 when it adds work. |

Exclude session replay. Keep customer MCP and a customer self-service analytics
or configuration portal outside the initial scope. Statistical experiments,
full logs/tracing, arbitrary warehouse imports and active bot enforcement are
deferred. The internal admin console is for inspection and controlled management,
not a second full analytics visualization product.

Start the useful pilot with one website and one CLI, collection/consent, internal
inspection and MCP analysis. Full P1 also includes advertising and automated
reporting. Keep the existing public OSS Metrics application and its contracts
separate. The [implementation plan](../plans/analytics-service.md) owns sequencing.

## Consequences

The product serves internal operators and customers through different interfaces
while sharing evidence and permission enforcement. The smaller pilot tests collection and
analysis before every external integration is available; it does not redefine
P1 as complete. Breadth across sources increases integration and lifecycle work.

## Validation and review triggers

Verify the pilot end to end before broadening rollout. Complete P1 against the
entire scope above, including access, privacy and reporting behavior. Revisit
priorities when actual usage, customer needs or integration constraints change;
update this record and the affected requirements together.

## References

- [Product brief](../discovery/analytics-service-brief.md)
- [Implementation plan](../plans/analytics-service.md)
