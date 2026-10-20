# Explain behavior through structured observations rather than session replay

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Record website/CLI observations, measurement units, freshness, and traffic classification.

## Context

Operators need to understand where visitors read, stay, progress, encounter
problems or leave. CLI usage adds commands and executions with different units.
A recording of the screen or DOM would introduce collection and privacy work
that is outside the selected product.

## Decision

Collect a bounded stream of named observations, subject to the
[project's permissions](0006-privacy-consent-and-recognition.md).

- For websites, provide page transitions, stable section visibility and active
  dwell signals through the SDK where permitted. Explicitly instrument business
  actions, conversion outcomes and relevant failure signals. Reading, dwell and
  abandonment conclusions remain inferences from observations.
- For CLI tools, record command name, CLI version, platform, outcome and coarse
  duration. Keep command executions, update checks and recognized systems
  separate. Collection failure must not make a CLI command unusable.
- Exclude session replay, DOM copies, keystrokes, form contents and continuous
  pointer movement. Exclude CLI arguments, file paths and project contents.
- Keep projects inside isolated workspaces. Do not automatically connect
  identities across projects or customers. Label authenticated server-origin
  business outcomes separately from public browser/CLI observations.
- Preserve the measurement unit, observation period, source freshness, coverage,
  recognition method and uncertainty in queries and reports. A pseudonym or
  update check is not proof of a unique person; distinct counts are not additive
  across days merely because event counts are.
- Distinguish observed bots, suspected automation, AI crawlers, internal tests,
  automated CLI use and unclassified traffic. Missing bot signals do not prove
  human activity. Select an available classification source during implementation;
  no Vercel or other vendor integration is selected by this decision.

Use a versioned batch contract with stable event IDs and receipt/occurrence
times. A successful collection response follows durable commit; retries must
not count the same accepted observation twice. The detailed write contract and
proposed payload bounds live in the [implementation plan](../plans/analytics-service.md).

Target query availability through MCP within five minutes of service receipt.
Retain occurrence time for late observations. This target needs end-to-end
validation; it is not an implemented freshness guarantee.

## Consequences

The service can reconstruct useful behavior without replaying a session. It
cannot observe every action or establish why someone left. Explicit outcome
instrumentation and visible collection gaps are necessary for useful insights.
Automation classification remains uncertain and separate from active blocking.

## Validation and review triggers

Use known page/section/CLI sequences to check counts, active dwell, deduplication,
permissions, source distinctions and query freshness. Verify forbidden payloads
do not leave the client and refusal clears optional queued events. Revisit an
observation only when it answers a concrete product question within the existing
privacy boundary; replay requires a separate product decision.

## References

- [Product scope](0004-product-scope-and-priorities.md)
- [Product brief](../discovery/analytics-service-brief.md)
- [Domain vocabulary](../../GLOSSARY.md)
