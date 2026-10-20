# Delegate login and OAuth while owning analytics access grants

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-20
- Change note: Keep provider operations in sebastian-auth and application identity/access integration in sebastian-analytics.

## Context

Browser login and internal MCP require an identity provider and OAuth authorization
server. Building one is outside this product's purpose. Identity processing in the
United States is a material adoption barrier, including when contractual transfer
safeguards are available. WorkOS AuthKit/Connect was assessed as a managed option
without a custom login domain; its public terms include US processing and do not
establish EU-only identity residency. See the [privacy and exit assessment](../discovery/workos-privacy-and-exit.md).

## Decision

Keep the analytics application in Rust and delegate authentication and OAuth
issuance to a maintained separate provider. Prioritize self-hosted **Rauthy** on
Bunny in one EU region, with PostgreSQL and persistent cache storage. This is a
deployment candidate requiring a real client smoke test, target-platform proof,
and operating/processing terms; it is not an already deployed service. Do not
provision WorkOS while this EU-operated direction is being validated. A managed
EU alternative remains possible through the same boundary.

The separate private [sebastian-auth repository](https://github.com/sebastian-software/sebastian-auth)
owns Rauthy configuration, client registration, independent CI/releases, Bunny
operations, mail, backups, and provider recovery. This repository is
`sebastian-analytics` and owns its OIDC adapter, browser sessions, MCP token
validation, local identity links, and analytics access grants. Analytics releases
must not redeploy the identity service. Keep the self-contained protocol tests
here; the optional real-provider proof uses sebastian-auth's local fixture.

Use standard OIDC authorization code with PKCE S256 for browser login and standard
OAuth discovery/JWKS and resource-bound JWTs for MCP. The adapter contains no
WorkOS/Rauthy user API, organization model, or vendor SDK. Initial compatibility
uses same-origin provider endpoints, RS256 signatures, exact configured issuer
(including its path/trailing slash), and standard string/array audience claims.
A future provider must satisfy this profile or supply a small boundary adapter.
Do not implement a custom authorization server.

Use an owned, stable public identity domain for self-hosting, plus independently
owned analytics/MCP/report URLs. Provider relocation behind the same issuer can
preserve the public protocol; replacing the issuer requires fresh authentication.
Passkeys are tied to their relying-party domain; no lossless credential/session
migration is promised. Vendor-neutral protocols reduce application changes,
not the need to verify a new provider's behavior.

### Application identity and permissions

Keep principal UUIDs, workspace memberships, recipient configuration, and audit
in application-owned PostgreSQL. Link exact `(issuer, subject)` identities to a
principal through a trusted operator connection. Never automatically merge users
by email or import provider organization roles as analytics permissions. A second
provider identity can map to the same principal during an explicit migration;
disabling/removing the old link revokes its local access. The runtime cannot
create principals, identity links, workspaces, or membership grants.

Workspace admins can change configuration; readers can inspect it. Recipient
configuration grants no internal access. Invites, named-recipient report grants,
and recipient email changes remain later W2/report work. Requiring a verified
email for the current internal browser login does not implement report access.

Each configuration operation checks membership and sets transaction-local
actor/workspace context. PostgreSQL RLS checks again with a restricted runtime
role. This trusts the backend's verified actor assertion and does not protect
against stolen database credentials. HTTP/MCP input never selects the actor.
Version conflicts and minimal audit records are atomic with each mutation.

### Browser and MCP credentials

Browser login uses one-use, browser-bound state and nonce, PKCE, signature,
issuer/audience/expiry validation, and subject-matched userinfo with verified
email. Store opaque session-token hashes, not provider access/refresh tokens.
Use host-only HttpOnly SameSite=Lax cookies (Secure in HTTPS deployments), a
30-minute absolute session lifetime, and Origin plus CSRF checks for mutations
and logout. Logout removes the local credential; it does not sign out of the
provider or invalidate every provider-issued token. Recheck the identity link
and workspace grants on requests. Expired authentication state is denied even
when cleanup or the provider is unavailable.

MCP uses the official Rust SDK's stateless Streamable HTTP transport. Verify the
bearer on every request: fixed trusted issuer/JWKS, signature, exact resource
audience, expiration/not-before, required `mcp:read`, and optional `mcp:write`.
Writes also require a current local admin membership. Cookies cannot substitute
for an MCP bearer token. Validate canonical Host and any present Origin. Serve
RFC 9728 protected-resource metadata and a discovery challenge, not a local token
endpoint. Pin separate public read/read-write clients with S256 and configured
scopes; leave dynamic registration disabled initially. Tool arguments are bounded
configuration operations, never arbitrary SQL.

Cache discovery/keys for five minutes; coalesce refreshes and impose a ten-second
cooldown including failed requests. Unknown signing keys fail closed during the
cooldown. Expired cache entries require successful discovery; a provider outage
can therefore deny otherwise unexpired credentials after five minutes. This is
a deliberate bounded-key policy, not offline or immediate provider-revocation
support. Keep issued MCP access tokens short-lived; provider account disabling
and external refresh-token revocation need their own operating checks.

### Lifecycle and replacement

Authentication attempts expire after ten minutes and are capped at 1,000 live
attempts. The runtime cleans expired attempts/sessions on startup and every
minute, and denies expired rows independently of cleanup. Backups/restores must
reapply stored deadlines. Identity links remain until operator unlink/account
deletion; they are the minimal access-control association. Set provider audit,
account-deletion, backup, and configuration-audit retention before live release
under [ADR 0007](0007-data-lifecycle-and-evidence.md).

To replace the provider, restore application IDs/grants first, configure and test
the new issuer and clients, and explicitly link the new verified identity to the
existing principal using operator-controlled evidence or dual authentication.
Do not use matching email alone as migration authority. Allow a bounded transition
with separate links, then disable the old link/issuer and revoke old credentials.
Keep rollback mappings until the transition is verified. Rehearse this process
before a provider contract renewal and no later than the third operating year.

## Validation and remaining release gates

Tests exercise PostgreSQL role restrictions, one-use state, session revocation,
HTTP CSRF and tenant isolation, MCP signature/issuer/resource/scope checks, local
role revocation, signing-key rotation/cooldown, and replacement by a second OIDC
issuer without changing principal UUIDs or workspace grants. Protocol fixtures
are local test infrastructure, not an alternative production authorization server.
Actual Rauthy/Codex evidence and commands belong in the [operating guide](../../ops/auth.md).

Rauthy's current resource-indicator documentation excludes upstream-brokered
social login from processing the resource parameter. Do not claim that path works
for MCP without a separate proof, and never relax audience validation to make it
pass. Local password/passkey authentication is the initial proof path.

Before live release, verify the deployed provider and intended client, login and
recovery/verified-email delivery, provider revocation, report-link/recipient
protection, rate limits, backups plus keys and restoration, actual EU placement,
and applicable processor/subprocessor terms. An EU runtime location alone does
not prove that support, email, backups, or every subprocessor stays in the EU.

## References

- [Reporting and access](0008-proactive-reports-and-internal-mcp.md)
- [Implementation plan](../plans/analytics-service.md)
- [Auth integration assessment](../discovery/auth-integration-assessment.md)
- [Rauthy resource indicators](https://sebadob.github.io/rauthy/work/resource_indicators.html)
- [Rauthy persistent cache](https://sebadob.github.io/rauthy/config/ha.html)
- [OIDC subject and standard claims](https://openid.net/specs/openid-connect-core-1_0.html#StandardClaims)
- [OAuth resource indicators](https://www.rfc-editor.org/rfc/rfc8707.html)
