# Product and architecture decisions

ADRs are living, repository-owned documents written in US English. Each file
contains the current decision for one topic; Git preserves its previous versions.
Read accepted ADRs before changing the affected product scope, policy, or
architecture. An accepted target is not proof that its implementation or
deployment is complete.

| ID | Decision | Status | Implementation |
| --- | --- | --- | --- |
| 0001 | [Shared portable container and PostgreSQL](0001-portable-container-and-postgres.md) | accepted | Shared Rust/PostgreSQL foundation and tenant checks exist; Frankfurt PS-5 HA cluster provisioned, application deployment and dedicated validation pending. |
| 0002 | [Living decision records](0002-living-architecture-decisions.md) | accepted | This index, template, and repository instructions establish the convention. |
| 0003 | [Rust backend](0003-rust-backend.md) | accepted | Rust foundation and configuration/auth/MCP exist; collection and live deployment pending. |
| 0004 | [Product scope and priorities](0004-product-scope-and-priorities.md) | accepted | P1/P2/P3 scope agreed; configuration persistence exists, other product features pending. |
| 0005 | [Structured observations](0005-structured-observations.md) | accepted | Website/CLI collection and traffic classification pending. |
| 0006 | [Privacy, consent, and recognition](0006-privacy-consent-and-recognition.md) | accepted | Product boundaries agreed; concrete policy and methods need validation before use. |
| 0007 | [Data lifecycle and evidence](0007-data-lifecycle-and-evidence.md) | accepted | Local auth expiry/cleanup implemented; analytics retention, corrections, and restore proof pending. |
| 0008 | [Proactive reports and internal MCP](0008-proactive-reports-and-internal-mcp.md) | accepted | Configuration HTTP/MCP and OIDC sessions exist; reports, admin UI, and live auth pending. |
| 0009 | [Advertising measurement](0009-advertising-measurement.md) | accepted | Attribution, provider access, and reporting integrations pending; exports remain P2. |
| 0010 | [Managed WorkOS identity and permissions](0010-external-identity-and-workspace-grants.md) | accepted | Native SDK, browser/MCP routes, and managed RLS context are fixture-tested; real browser login passed locally; real MCP client proof and deployment pending. |
| 0011 | [Public Apache 2.0 repository](0011-public-apache-license.md) | accepted | Apache-2.0 LICENSE/NOTICE; GitHub visibility and CI are checked at publication. |
| 0012 | [Sebastian Pulse product name](0012-sebastian-pulse-product-name.md) | accepted | Product, repository, Rust package/binary, image, and Compose names use Pulse; database contracts retain analytics terminology. |

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
