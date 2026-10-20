# Delegate identity, organizations, and permissions to WorkOS

- Status: accepted
- Date: 2026-10-20
- Updated: 2026-10-21
- Change note: Choose managed WorkOS AuthKit and Connect, including its organization and permission model; pause self-hosted identity deployment.

## Context

Pulse needs browser login, internal MCP authorization, organization membership,
and access management. Operating an authorization server adds security, recovery,
mail, upgrades, and availability work that does not distinguish the analytics
product. Rauthy was validated locally, but its deployment is no longer the chosen
path. Prefer the intended WorkOS capabilities over rebuilding user management or
maintaining a parallel source of permission grants.

WorkOS includes US processing. Its public terms do not establish EU-only identity
residency. The [privacy and exit assessment](../discovery/workos-privacy-and-exit.md)
records the evidence and remaining production inputs. This choice does not imply
that the vendor has been approved for every data class or customer deployment.

## Decision

Use managed **WorkOS AuthKit** for hosted login, verification, authentication
methods, recovery, and session issuance/refresh/revocation. Start with the default
WorkOS login domain, without a custom login domain. Use **WorkOS Connect** as the
OAuth authorization server for internal MCP. Do not build or deploy a separate
identity server or an OAuth completion broker for this integration.

WorkOS is the source of truth for users, organizations, memberships, roles, and
permission mappings. Use fitting WorkOS features directly, through its official
Rust SDK where available. SSO, MFA, invitations, Admin Portal, and later capabilities
can use the vendor's supported flows when required; this decision does not enable
paid add-ons or claim that every capability is already integrated.

The separate [sebastian-auth repository](https://github.com/sebastian-software/sebastian-auth)
retains the previous Rauthy operating work as a reference. Pause its production
rollout. Pulse's PostgreSQL and Bunny deployment remain independent of WorkOS;
there is no reason to create a second authentication database for this direction.

### Application data and authorization

Pulse retains stable internal principal and workspace UUIDs for product data,
references, and audit. Map exact provider/environment user and organization IDs to
those UUIDs at the integration boundary. Do not use an email address to merge
accounts or inherit workspace access. An organization-to-workspace mapping binds
product data; it does not create an independent membership or role authority.

Use `pulse:read` and `pulse:write` permission slugs, with the read permission
required for any internal access. Roles and their permission assignments live in
WorkOS, not in hardcoded role-to-permission tables in Pulse. Pending and inactive
memberships grant no access. Report-only access remains a separate permission
boundary: adding a report recipient does not grant internal analytics access.

Enforce verified caller, selected organization, and effective permissions in
application operations and transaction-local PostgreSQL RLS. The existing local
membership tables are the previous implementation, not the accepted authority for
the new WorkOS path. Do not silently reuse them as a second source of grants.
Do not bypass RLS to complete provider integration. The database still trusts the
backend's verified actor/context assertion; it cannot protect against stolen
runtime credentials that permit those assertions.

### Browser and MCP credentials

Use AuthKit authorization code with S256 PKCE and one-use, browser-bound state.
The official SDK exchanges the code and manages sealed provider sessions and
refresh. Keep tokens out of URLs, logs, product responses, and Git. Use host-only
HttpOnly Secure cookies in production, Origin plus CSRF checks for mutations,
and provider session revocation on logout. A local cookie wrapper must not claim
to revoke all already-issued access tokens immediately.

MCP accepts user Connect tokens only: verify the fixed hosted issuer, trusted
JWKS, signature, exact resource audience, expiration, and client scopes. Do not
accept AuthKit browser session tokens or machine tokens as user MCP credentials.
Connect scopes describe client delegation; they do not establish the user's
permissions. After token validation, resolve the selected user's active WorkOS
membership and union of WorkOS role permissions. Read/write access is the
intersection of delegation and effective user permissions.

The initial adapter checks permissions directly against WorkOS on each request;
errors deny access rather than falling back to local or stale grants. A bounded
cache may be added only with an explicit revocation window and outage behavior.
Keep Connect keys for five minutes with a ten-second refresh cooldown, including
failed refreshes. Require refresh when the key cache expires; unknown keys fail
closed. Use the existing stateless MCP transport and protected-resource metadata.
Start with explicitly registered public read and read/write clients and S256;
dynamic registration requires its own compatibility and scope proof.

### Portability and replacement

Contain WorkOS SDK models, API calls, session handling, and permission resolution
in the auth integration module. Product operations consume internal identifiers
and an authenticated access context. Do not impose a generic identity-provider
interface that hides WorkOS features or recreates user/organization management.

A provider change includes **authorization migration**: export users,
organizations, memberships, role definitions, permission assignments, and their
internal ID mappings. Import equivalent grants into the new provider and verify
that previously denied access stays denied. Preserve product UUIDs and audit
references. Never migrate access based on a matching email alone. Test dual
identity linking or a controlled recovery path while both providers are available.

Passwords, MFA factors, passkeys, sessions, and Connect authorizations have
separate export limits. Assume fresh authentication and potentially credential
re-enrollment. A default vendor login domain makes passkeys domain-bound; revisit
that tradeoff before production passkey enrollment. Rehearse an export/replacement
before the third operating year and before a material contract renewal.

## Implementation evidence and remaining work

The Rust WorkOS boundary implements SDK-based AuthKit PKCE, code exchange,
sealed sessions, refresh/revocation, active membership and role-permission
resolution, and resource-bound Connect validation. Synthetic tests exercise
native API contracts, denied memberships, permission changes, scope intersection,
session tampering, issuer/audience failures, and expiry.

Startup selects WorkOS when its complete configuration is present; legacy OIDC
settings and WorkOS settings are mutually exclusive. Native browser and MCP routes
share configuration operations using a verified, request-scoped access context.
Migration 000004 binds provider IDs to stable product IDs. Organization bindings
remain operator-managed; principal bindings are created only after managed access
verification. These bindings contain no persistent membership grants. RLS consumes
the transaction-local managed permission; the old local membership table cannot
broaden managed requests. The previous OIDC/local-grant path remains available
only as an explicitly selected legacy mode.

Real PostgreSQL/HTTP tests exercise stable concurrent identity mapping, private
binding tables, no implicit local grant, tenant and actor mismatch, stale local
admin grants, pooled context isolation, native login state, CSRF, permission
removal, provider outage, MCP scope intersection, and provider logout.

A WorkOS staging project and the local callback are configured. Real registration,
verified email, PKCE callback, and browser session passed locally on October 21,
2026. Real MCP client proof, concurrent refresh resilience, signing-key rollover against the real
provider, production cookie/ingress configuration, deployment, and recovery remain
completion gates. See the [WorkOS integration guide](../../ops/workos.md).

## References

- [WorkOS Rust SDK](https://workos.com/docs/sdks/rust)
- [AuthKit hosted UI](https://workos.com/docs/authkit/hosted-ui)
- [AuthKit sessions](https://workos.com/docs/authkit/sessions)
- [Roles and permissions](https://workos.com/docs/authkit/roles-and-permissions)
- [Connect access token claims and server-side permission checks](https://workos.com/docs/authkit/connect/token-claims)
- [Reporting and access](0008-proactive-reports-and-internal-mcp.md)
- [Implementation plan](../plans/analytics-service.md)
