# Analytics service

Terms shared by the product brief, service contracts, and implementation.
Counting units describe observations rather than verified distinct people.

## Language

**CLI update check**:
A recorded request by a CLI to check for an available update. Repeated requests
and CI requests are checks too; a check does not identify a distinct person.
_Avoid_: User, active user, installation

**CLI execution event**:
A recorded observation of a CLI command execution and its outcome within the
agreed telemetry scope. An execution does not identify a distinct person.
_Avoid_: CLI update check, active user, installation

**Package download**:
A download counted by a package registry according to that registry's rules.
It is not a count of distinct people or installations.
_Avoid_: User, installation

**Complete measurement window**:
A time interval with verified continuous eligible collection and sufficient
data retention to support a count for the entire interval.
_Avoid_: Successful query, nonempty result

## Analytics service

**Analytics workspace**:
The isolated data and access boundary for a customer or an explicitly separated
set of owned offerings. It can contain multiple websites and products.
_Avoid_: Customer project, website

**Workspace membership**:
An explicit internal read or administration grant for one analytics workspace.
Membership in one workspace gives no access to another.
_Avoid_: Report recipient, verified email

**Report recipient**:
A named intended viewer of permitted browser reports whose email ownership must
be verified before access. Being a recipient does not grant internal workspace access.
_Avoid_: Workspace member, administrator

**Conversion goal**:
A defined action that represents a desired outcome for an offering, such as an
inquiry, registration, purchase, or download.
_Avoid_: Click, pageview

**Campaign attribution**:
The assignment of a visit or conversion to a campaign according to a stated
attribution rule.
_Avoid_: Ad spend reporting, conversion export

**Ad spend reporting**:
The comparison of advertising expenditure and delivery metrics with campaign
outcomes.
_Avoid_: Campaign attribution, conversion export

**Conversion export**:
The transmission of selected conversion information to an advertising network
for that network's measurement or optimization.
_Avoid_: Campaign attribution, ad spend reporting

**Trend report**:
A proactively delivered summary of changes over a stated reporting period,
including the comparison period and limitations of the underlying measurements.
_Avoid_: Dashboard

**Behavior stream**:
An ordered sequence of named actions and compact measurements that helps
explain recorded activity within its permitted recognition boundary. It does
not represent a visual recording or prove a person's intent.
_Avoid_: Session replay, screen recording, complete user history

**Behavior analysis**:
An evidence-based interpretation of behavior streams and aggregate patterns to
explain engagement, progression, and abandonment, including measurement limits.
_Avoid_: Model usage analytics, session replay

**Report notification**:
A message that informs a recipient that a report is available and directs them
to its browser view.
_Avoid_: Report, dashboard

**Browser report**:
A visual presentation of an analysis for a stated reporting period, accessible
through a browser link to permitted viewers.
_Avoid_: Notification, MCP access

**Report snapshot**:
Stored analysis results for a stated reporting period and revision, with
identifiable corrections and the applicable data-lifecycle limits.
_Avoid_: Live dashboard, notification, permanent journey archive

**Event exclusion**:
A recorded decision to omit an observation from analysis without rewriting
its original contents. Exclusion and deletion are separate actions.
_Avoid_: Edited event, deleted event

**Feature flag**:
An operational control that enables or disables application behavior for a
specified audience or environment.
_Avoid_: Usage event, statistical experiment

**AI actor label**:
An abstract identifier that lets an analysis distinguish subjects and connect
related activity without exposing the subject's direct identity in that label.
The label alone does not establish anonymity or a verified distinct person.
_Avoid_: Anonymous user, verified person

**Recognition unit**:
The entity that a recognition method attempts to distinguish, such as an account,
browser context, system installation, or group with shared characteristics.
It is not automatically equivalent to one person.
_Avoid_: Unique user, individual person

**System recognition identifier**:
A project-specific pseudonym derived from a CLI's local system identity to
connect activity across executions. It is not proof of a distinct physical
machine or person.
_Avoid_: Hardware identity, unique user

**Estimated recognition link**:
A suspected association between activities based on shared characteristics,
whose uncertainty remains part of the analysis.
_Avoid_: Verified identity, proven individual journey

**Consent interface**:
The visitor-facing surface for understanding and changing choices about
measurement and other governed purposes.
_Avoid_: Acknowledgement notice, cookie notice

**Privacy choice**:
A person's recorded permissions or objections for stated purposes within the
relevant project and policy context.
_Avoid_: Accept everything, cookie present

**Privacy baseline**:
The conservative measurement and recognition boundary used when applicable
permissions or policy requirements have not been established.
_Avoid_: Anonymous mode, cookieless mode, global compliance guarantee

**Attribution window**:
The period before a conversion in which an eligible contact can receive credit
under the selected attribution model.
_Avoid_: Event retention, identifier lifetime

**First-touch attribution**:
Attribution that credits the earliest eligible known contact within the
attribution window.
_Avoid_: Last-touch attribution

**Last-non-direct attribution**:
Attribution that credits the latest eligible known non-direct contact within
the attribution window, preserving that contact across a later direct return.
_Avoid_: Last pageview, last direct visit
