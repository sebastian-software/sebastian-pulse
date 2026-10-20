# Use living architecture decision records

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Establish the living lifecycle for all ADRs in this repository.

## Context

Architecture decisions must remain available to people and AI agents across
sessions and tools. Reading the current direction should not require reconstructing
chat history or following a chain of decisions that replaced one another.

## Decision

Keep living ADRs as plain Markdown in `docs/adr/`, with a discoverable
[index](README.md) and a lightweight [template](template.md). This convention
applies to the existing portable-container decision and all subsequent records.
Write repository content, including ADRs and agent instructions, in US English.

- Keep one durable topic per record. Use a stable `NNNN-topic.md` filename,
  starting with the next unused number; do not renumber existing records.
- Use `proposed`, `accepted`, `rejected`, or `deprecated` as the status.
  Accepted means the current agreed direction, not completed implementation.
- Update the same file when an authorized decision changes. Keep its original
  date, refresh `Updated`, and summarize the latest material change in
  `Change note`. Git history preserves earlier decisions and their reasoning.
- State context, decision, consequences, and meaningful validation or review
  triggers. Distinguish observations, forecasts, and experienced judgment.
- Keep the index, affected plans, and implementation-status notes consistent.
  Expose gaps between the accepted target and the running code.
- Agents read relevant accepted records before implementation and update them
  when the task changes their decision. Unresolved choices stay explicit;
  a proposal stays proposed until there is an agreed direction.

ADRs contain rationale and boundaries. Requirements, delivery tasks, commands,
provider settings, and verification results keep their existing owners and are
linked where useful. Do not store secrets, personal data, or private chat and
approval transcripts in an ADR.

## Consequences

Readers get one current decision per topic, with less context needed for later
agent sessions. Earlier reasoning is less visible in the current document and
requires Git history. Material edits therefore need a clear commit message and
an updated change note; accepted records are intentionally mutable.

## Validation and review triggers

Check ADR links, unique IDs, dates, statuses, and agreement with the plan when a
decision changes. Revisit the lifecycle if the team needs an immutable audit
record independent of Git history.
