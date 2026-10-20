# Analytics service: product brief

Status: accepted product scope, 2026-10-20, including the initial workload
and portability requirements.
This brief consolidates detailed requirements from discovery. The
[ADR index](../adr/README.md) records the decisions and rationale behind this
scope, starting with [scope and priorities](../adr/0004-product-scope-and-priorities.md).
Keep this consolidation aligned when a living product decision changes.
See the [Bunny and PlanetScale assessment](bunny-planetscale-assessment.md) for
the technical evidence and the [implementation plan](../plans/analytics-service.md)
for the selected Bunny Magic Containers / PlanetScale PS-5 starting configuration.
The Package 1 local foundation is implemented in Rust in this repository.
Workspace configuration persistence, RLS, OIDC login, protected HTTP, and scoped
MCP are implemented. Collection and reporting are pending. The Frankfurt PS-5 HA
cluster is provisioned; production roles, deployment, and login validation remain
in progress. See the README for commands and evidence.

| Product decision | Owning record |
| --- | --- |
| Coverage, P1/P2/P3, pilot and exclusions | [ADR 0004](../adr/0004-product-scope-and-priorities.md) |
| Behavior streams, CLI units, traffic classification and freshness | [ADR 0005](../adr/0005-structured-observations.md) |
| Consent, regional policy, recognition and AI processing boundaries | [ADR 0006](../adr/0006-privacy-consent-and-recognition.md) |
| Retention, corrections, deletion and restored data | [ADR 0007](../adr/0007-data-lifecycle-and-evidence.md) |
| Proactive reports, recipient access, internal MCP and admin | [ADR 0008](../adr/0008-proactive-reports-and-internal-mcp.md) |
| Advertising integrations, attribution and later exports | [ADR 0009](../adr/0009-advertising-measurement.md) |

## Product outcome

Understand how audiences, product usage, engagement, abandonment, and advertising
outcomes develop. Deliver useful explanations and recommendations proactively,
without requiring routine dashboard inspection. Internal staff can ask follow-up
questions and manage configuration through MCP; customers receive browser reports.

## Coverage and planning volume

- Owned commercial and OSS product websites.
- Agency customer projects and future VorOrt customer projects.
- Authenticated VorOrt product areas.
- CLI tools including Dalo and Palamedes.
- Initial forecast: approximately 1,000 pageviews/day, assumed portfolio-wide
  for estimation. Additional CLI/product event volume remains to be measured.
- Later planning ceiling: 20 websites at 10,000 pageviews each per day, or
  200,000 pageviews/day, before additional behavior/product/CLI events.
- Isolated customer workspaces may contain multiple offerings. Cross-workspace
  analysis is deliberate; recognition is not automatically shared across projects.

## P1: initial service

| Capability | Required behavior |
| --- | --- |
| Website collection | A script gives useful page-transition, section-visibility, and active-dwell signals by default where permitted. Business actions and conversion outcomes are explicitly instrumented. |
| Product behavior analysis | AI explains trends, progression, engagement, and possible abandonment using named behavior streams and aggregates. Proxies, gaps, and uncertainty remain visible. |
| CLI collection | Record command name, CLI version, platform, outcome, and coarse duration within the agreed telemetry permission. Distinguish command executions, update checks, recognized systems, and people. |
| Advertising measurement | Standard UTM attribution, ad costs, delivery metrics, configurable conversion goals, and outcome comparisons. Google, Meta/Instagram, and LinkedIn are in the desired integration scope. |
| Attribution | A 30-day window with first-touch and last-known-non-direct models. Direct returns preserve eligible preceding campaign credit; estimated recognition remains labeled. |
| Automated reporting | Weekly internal trends, targeted internal anomaly notices with supporting evidence, and monthly customer reports. Configure recipients and optional approval per customer; notify by email and show visual reports in the browser. |
| Report access | Named recipients verify their email address; forwarding a link alone does not grant access. Retained verified login sessions are acceptable. |
| Internal MCP | Query and analyze data; manage projects, goals, recipients, and the configuration of included or later-priority capabilities. Separate read/write rights and trace changes. |
| Internal admin console | Inspect, search, and filter event/journey tables. Create, update, and delete projects, goals, and report recipients. Apply the same access rules as MCP. |
| Evidence integrity | Recorded events are not freely rewritten. Targeted exclusions and deletions support correction and cleanup, with administrative actions recorded. Report corrections remain identifiable. |
| Owned consent interface | Provide the basic visitor interface and a configurable service/purpose interface for measurement and other governed services such as video, maps, or chat. |
| Automated-traffic classification | Distinguish observed bots, suspected automation, internal tests, automated CLI use, and unclassified traffic. AI crawlers remain separately visible. Missing bot signals do not establish human activity. |

Website/CLI events should become queryable through MCP within five minutes of
collection. Advertising reports show both import and source-data timestamps.
Report notifications follow their schedule; this freshness target is not an
unimplemented service guarantee.

## Privacy, recognition, and retention boundaries

- Use maximum lawful insight rather than a mandatory cookieless design. Cookies
  are permitted where appropriate. The initial applicability scope covers the
  EU/EEA, UK, and US, with operator context, purposes, destinations, and actual
  applicability considered alongside visitor region.
- Use a conservative shared baseline when permissions or policy applicability
  are unresolved. Disable optional recognition, fingerprinting, individual
  histories, and advertising disclosures until permitted. Coarse request/error
  totals require a lawful concrete collection and aggregation path too.
- Respect refusal, withdrawal, and applicable opt-out signals. A generic
  analytics choice must not silently override an applicable advertising opt-out.
- Do not collect session replay, DOM copies, keystrokes, form contents, or
  continuous pointer movement. CLI arguments, file paths, and project contents
  are also excluded.
- Authenticated product activity may use internal identifiers across visits.
  Approximate browser recognition is acceptable only as an explicit estimate;
  no fingerprint algorithm or recognition accuracy is selected.
- CLI continuity may use a locally derived, project-specific system identifier.
  Do not transmit the raw system identifier or automatically link activity
  across tools/customers. Support reset and disabling; a new OS installation
  may produce a different identifier.
- AI may follow project/analysis-scoped actor labels and behavior streams.
  Remove direct identity and identifying payload/URL contents before AI access;
  labels alone do not make the supplied data anonymous.
- Automatic AI analysis requires EU processing and a contractual prohibition
  on using supplied data for model training.
- Keep detailed behavior and individual-journey report copies for 90 days.
  Keep condensed metrics and report snapshots for 24 months; customer settings
  may shorten these periods. Reports do not preserve expired journeys or actor
  IDs as permanent copies. Snapshot stability does not override required deletion.

## Later priorities

| Priority | Capability |
| --- | --- |
| P2 | Export conversions to advertising networks; operational feature flags. |
| P3 | Simple voluntary surveys; product model-call/token/cost/latency analytics; adapters to existing consent managers when they add work. |

Feature flags target roughly one-minute propagation and predefined application
fallbacks when unavailable. Immediate security shutdown is a separate guarantee.
Operational visibility into our report-generation costs is distinct from a P3
model-observability product feature.

Session replay is excluded. Customer MCP and a customer self-service analytics
or configuration portal are outside the initial scope. Statistical experiments,
full logs/tracing, arbitrary warehouse imports, and active bot enforcement are
deferred.

## Technical decisions that follow product confirmation

The selected shape is one shared Rust backend and PostgreSQL database for
multiple customer workspaces, initially on Bunny Magic Containers with
PlanetScale Postgres PS-5 HA in Frankfurt. Workspace permissions and
PostgreSQL RLS enforce tenant isolation. Preserve the option of a future dedicated
customer installation using the same container image and a separate PostgreSQL
database, as scoped by [ADR 0001](../adr/0001-portable-container-and-postgres.md).
Dedicated provisioning and operations are outside the initial scope. Browser SDK
and interface code remain TypeScript. See the [ADR index](../adr/README.md),
especially the [Rust backend decision](../adr/0003-rust-backend.md). Keep provider switching
practical through a standard container, conventional Postgres, portable event
contracts, and a verified export path. Preserve existing public OSS Metrics
contracts; the analytics service has its own repository and runtime.

A separate maintained identity provider, including managed identity, is allowed
under [ADR 0010](../adr/0010-external-identity-and-workspace-grants.md). EU-operated identity is now the deployment requirement; self-hosted Rauthy is the
validation candidate behind a standard OIDC/OAuth boundary. The public
[sebastian-auth repository](https://github.com/sebastian-software/sebastian-auth)
owns provider operations independently; sebastian-pulse owns application
sessions and access grants. Live login/MCP
compatibility, identity export/restore, and all auth processing locations still
need validation. The next design pass must also resolve live database
compatibility, durable scheduled work, collection peaks and event volume, cost
boundaries, access roles, actual region/purpose rules, recognition validation,
CLI platform derivation and shared/CI permission handling, advertising integration
details, and deletion propagation. Provider and bot-signal availability must be
verified for the actual data path. Do not silently treat these open implementation decisions as proven
capabilities or user commitments.

## Confirmation

The product-level scope is confirmed. The technical assessment evaluates the
preferred shape and alternatives against this brief, including the smaller launch
workload, operating costs, portability, and remaining feasibility gaps.
The local Rust Package 1 foundation is implemented. Live provider provisioning
remains pending. Optional OIDC sessions and configuration HTTP/MCP transports
are implemented; invitations, report access, collection, and reporting remain
subsequent work. Accepted product scope is not implementation evidence.
