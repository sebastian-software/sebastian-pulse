# Product and architecture decisions

ADRs are living, repository-owned documents written in US English. Each file
contains the current decision for one topic; Git preserves its previous versions.
Read accepted ADRs before changing the affected product scope, policy, or
architecture. An accepted target is not proof that its implementation or
deployment is complete.

| ID | Decision | Status | Implementation |
| --- | --- | --- | --- |
| 0001 | [Portable container and PostgreSQL](0001-portable-container-and-postgres.md) | accepted | Local Rust foundation exists; live deployment pending. |
| 0002 | [Living decision records](0002-living-architecture-decisions.md) | accepted | This index, template, and repository instructions establish the convention. |
| 0003 | [Rust backend](0003-rust-backend.md) | accepted | Rust foundation implemented; collection and other product packages pending. |
| 0004 | [Product scope and priorities](0004-product-scope-and-priorities.md) | accepted | P1/P2/P3 scope agreed; product features pending. |
| 0005 | [Structured observations](0005-structured-observations.md) | accepted | Website/CLI collection and traffic classification pending. |
| 0006 | [Privacy, consent, and recognition](0006-privacy-consent-and-recognition.md) | accepted | Product boundaries agreed; concrete policy and methods need validation before use. |
| 0007 | [Data lifecycle and evidence](0007-data-lifecycle-and-evidence.md) | accepted | Retention, correction, deletion, and restore behavior pending. |
| 0008 | [Proactive reports and internal MCP](0008-proactive-reports-and-internal-mcp.md) | accepted | Reports, access, MCP, and admin capabilities pending. |
| 0009 | [Advertising measurement](0009-advertising-measurement.md) | accepted | Attribution, provider access, and reporting integrations pending; exports remain P2. |

Follow [ADR 0002](0002-living-architecture-decisions.md) when creating or updating
a record. Start with the [template](template.md), assign the next unused
four-digit number, and add the record here. Keep each topic's filename stable.

Start with [ADR 0004](0004-product-scope-and-priorities.md) for the feature scope
and priority decisions. The [product brief](../discovery/analytics-service-brief.md)
consolidates detailed requirements; the
[implementation plan](../plans/analytics-service.md) owns delivery work. ADRs own
durable product and architecture choices and their rationale. Update connected
requirements when a decision changes rather than maintaining competing scope
lists. Executable settings and commands remain in code, configuration, the root
README, and operations guides.
