# Gate recognition and collection by project permissions and policy

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Record the agreed privacy baseline, owned consent interface, and recognition boundaries.

## Context

The product should provide as much lawful audience and product insight as
possible. Cookieless operation is desirable when useful but is not a mandatory
restriction. The initial policy-applicability scope covers the EU/EEA, UK and
US. Visitor region alone does not settle the operator, purpose, destination or
applicable policy. Abstract hashes do not by themselves establish anonymity.

## Decision

Provide an owned consent interface in P1, including visitor choices and
configurable services/purposes for measurement and other governed services such
as video, maps or chat. Respect refusal, withdrawal and applicable opt-out
signals. An analytics choice does not silently override an advertising opt-out.
Adapters to existing consent managers are P3 when they add work.

Use a conservative shared baseline while applicability or permissions remain
unresolved. Disable optional recognition, fingerprinting, individual histories
and advertising disclosures until permitted. Even coarse aggregate request/error
measurement needs a permitted concrete collection path. This is the selected
product fallback, not a finding that one universal rule guarantees compliance.

| Recognition case | Accepted boundary |
| --- | --- |
| First-party browser continuity | Cookies are allowed where appropriate; recognition stays project-specific and subject to applicable permissions. |
| Authenticated products | Internal account identifiers may connect permitted activity across visits; this does not automatically permit cross-project/customer linking. |
| Approximate browser recognition | May be used only as an explicit estimate within the applicable policy. No fingerprint algorithm, accuracy level or default activation is selected. |
| CLI system continuity | Derive a project-specific pseudonym locally where permitted; do not transmit raw system identity. Support reset and disabling, and describe OS reinstall, cloned/shared environment and CI limitations. |
| AI analysis | Use project/analysis-scoped actor labels so related activity can be distinguished. Remove direct identity and identifying payload/URL contents before model access; labels do not make the whole evidence bundle anonymous. |

Require EU processing and a contractual prohibition on model training with
supplied data for automatic AI analysis. A model/provider selection still needs
to satisfy that boundary. Establish concrete regional/purpose rules, recognition
validation, CLI platform derivation and noninteractive permission behavior before
activating the affected collection.

## Consequences

Measurement can use permitted continuity without promising verified unique
people. The fallback yields less connected evidence when policy or consent is
unresolved; reports must preserve that limitation. Owning the consent surface
adds product work but avoids making third-party consent integration a P1
prerequisite. Hashing is a technical representation, not a legal basis.

## Validation and review triggers

Exercise acceptance, refusal, withdrawal, reset and applicable opt-out paths.
Verify optional collection remains disabled under unresolved policy, raw CLI
identity is not transmitted, and AI evidence is scoped and filtered. Revisit
policy when purposes, regions, destinations or recognition methods change.
This record captures product boundaries; actual applicability remains a release
question for the concrete deployment and processing path.

## References

- [Product brief](../discovery/analytics-service-brief.md)
- [Structured observations](0005-structured-observations.md)
- [Data lifecycle](0007-data-lifecycle-and-evidence.md)
