# Name the product Sebastian Pulse

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Adopt Pulse for the product and delivery artifacts alongside Sebastian Auth.

## Context

The product collects permitted observations and explains behavior and trends,
including proactive reports. Analytics describes its domain but does not express
the ongoing observation and notification aspect. Insights emphasizes analysis
results; Pulse better expresses the ongoing product direction.

## Decision

Use **Sebastian Pulse** as the product name and `sebastian-pulse` for the
repository, Rust package and binary, container image, and default Compose
project. Keep **Sebastian Auth** as the independently operated identity service.

Keep analytics as the domain term in the PostgreSQL schema, roles, migration
ledger, API concepts, and existing documentation filenames. A product rename
does not change data contracts or justify rewriting applied migrations.
Use the product name in future deployment and client display names.

## Consequences

The name fits proactive reporting while the glossary explains the product's
scope. Automation, imports, links, and image references need a coordinated
rename. Existing local Compose projects must retain their previous project name
to reuse volumes; the default project name does not migrate stored data.

## Validation and review triggers

Run the existing Rust, PostgreSQL, and container checks with the new binary and
image names, verify the repository redirect, and update Auth's consumer links.
Revisit the naming only when the product direction changes materially.

## References

- [Product brief](../discovery/analytics-service-brief.md)
- [Glossary](../../GLOSSARY.md)
- [External identity boundary](0010-external-identity-and-workspace-grants.md)
