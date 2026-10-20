# Delegate login and OAuth while owning analytics access grants

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Allow a separate, including managed, identity provider and implement the domain-owned configuration boundary.

## Context

Verified browser login and MCP authorization require an identity service and an
OAuth authorization server. Rust client/session libraries do not supply that
complete service. Implementing an authorization server would add security and
maintenance responsibilities outside this product's purpose.

The analytics application remains one Rust service. A separate identity provider
is acceptable, including a managed provider that removes identity-service
operations from this application. Provider selection and deployment are still
open; WorkOS and Better Auth are candidates to assess, not configured services.

## Decision

Delegate identity verification and OAuth authorization-server behavior to a
maintained integration. Do not implement a custom authorization server. Allow
its process, hosting, and implementation language to differ from analytics.
Prefer standard OIDC/OAuth integration with an owned public issuer domain where
supported. A managed provider's identity state need not be in our PostgreSQL
database; document its export and migration limits before selecting it.

Keep analytics identities and permissions in application-owned PostgreSQL tables.
Bind provider identities by the exact issuer and subject, not by an unverified
email address. Identity-provider groups or a successful login alone do not grant
workspace or report access. Verify email ownership before granting named-recipient
report access. Workspace grants and report-recipient grants remain distinct.

Use workspace `admin` and `reader` memberships for internal configuration:
admins can read and change projects, goals, and recipients; readers can inspect
them. A recipient record creates no internal membership. Initial principal,
workspace, and membership provisioning requires a trusted operator connection;
the runtime cannot create or elevate those grants. Authenticated invitations and
membership administration remain subsequent work.

Every configuration operation applies application permissions and transaction-local
actor/workspace context. PostgreSQL RLS checks membership and write permission
again using a restricted, non-owner runtime role. This boundary trusts the backend
to assert the verified actor; it does not protect against stolen runtime database
credentials. Never accept an actor UUID from an unverified HTTP or MCP request.

Version configuration changes and reject stale mutations. Commit each mutation
with its audit records, including goals deleted with a project. Audit identifiers,
action, revision, actor, and time; omit configuration contents and recipient email
from audit payloads. Establish administrative-audit retention before exposing it
as a live capability under [ADR 0007](0007-data-lifecycle-and-evidence.md).

MCP resource/audience validation, separate read/write scopes, and application-owned
workspace grants are independent checks. Browser and MCP adapters must call the
same configuration operations. No public configuration routes are enabled by the
initial persistence implementation.

## Consequences

Analytics hosting and data remain portable without coupling workspace permissions
to a vendor's organization model. External identity introduces a separate
availability dependency and an identity migration path. Managed identity can
remove service operations but may restrict issuer domains, data location, export,
or pricing; self-hosting keeps those responsibilities with the operator.

## Validation and review triggers

The configuration slice checks denied cross-workspace access, restricted database
grants, pooled-connection cleanup, revocation, concurrency, and audit rollback
against PostgreSQL. Authentication, browser/admin routes, MCP authorization,
forwarded-report links, and identity export/restore are not implemented by it.

Before shipping login, test the selected provider and actual intended MCP client,
including exact resource audience, issuer/signature/expiry, scopes, unverified
email, absent/revoked application grants, session lifecycle, and client-registration
policy. Verify an identity export/restore or document the provider's migration
limitation. Check auth-region/DPA requirements and costs before provisioning.

## References

- [Rust backend](0003-rust-backend.md)
- [Reporting and access](0008-proactive-reports-and-internal-mcp.md)
- [Implementation plan](../plans/analytics-service.md)
- [Auth integration assessment](../discovery/auth-integration-assessment.md)
- [OIDC subject and standard claims](https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims)
- [OAuth resource indicators](https://www.rfc-editor.org/rfc/rfc8707.html)
