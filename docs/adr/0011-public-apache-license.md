# Publish Sebastian Analytics under Apache 2.0

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Publish the repository and its owned material under Apache-2.0.

## Context

The service is shared infrastructure. Publishing the implementation and operating
knowledge supports reuse without committing to a commercial product or a public
hosted service. Repository visibility and access to deployed identity or analytics
data are separate concerns.

## Decision

Make `sebastian-software/sebastian-analytics` public. License the repository's owned code, configuration,
scripts, and documentation under Apache-2.0, with the canonical text in `LICENSE`
and copyright attribution in `NOTICE`. Referenced dependencies and upstream
container images keep their own licenses and attribution.

Keep production credentials, keys, generated configuration, backups, logs, and
customer data out of Git. Fixed local fixture credentials and deliberately public
test keys remain clearly labeled and must never be deployed. Review all published
refs and relevant discussions/workflow logs before changing visibility. Public
source does not grant access to a running service or change its permission model.

## Consequences

Source, history, and supported repository surfaces become readable and reusable.
The implementation remains evolving infrastructure; open source adds no hosting,
support, or commercial-product commitment. Publishing does not complete the
provider, deployment, recovery, or processing-location validation gates.

## Validation and review triggers

Verify public visibility and Apache-2.0 detection on GitHub, license attribution,
ignored operator files, and successful CI. Before publishing new operational
examples, confirm that their values are synthetic and their secrets are external.
Revisit contribution and release policies when external participation or support
needs justify them.
