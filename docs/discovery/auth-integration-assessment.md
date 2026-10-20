# Authentication integration assessment

Assessed October 20, 2026; updated October 21, 2026. The accepted direction is
managed WorkOS AuthKit and Connect under
[ADR 0010](../adr/0010-external-identity-and-workspace-grants.md). WorkOS owns users,
organizations, memberships, roles, and permissions. Pulse owns product data,
stable internal IDs, organization-to-workspace bindings, and enforcement of
verified access. The default hosted login domain avoids a custom-domain dependency.

A staging project, exact local callback, and short-lived server key are configured.
The local service reaches the real hosted AuthKit login. Native SDK and PostgreSQL
tests cover browser sessions, permission changes, tenant isolation, and MCP token
validation. A completed real user login, real MCP client, and production rollout
remain separate evidence; see the [operating guide](../../ops/workos.md).

## Earlier EU-operated direction and standard boundary

Self-hosted Rauthy on Bunny was the earlier direction for EU-operated identity,
with a dedicated provider database/role and persistent cache volume. Its production
rollout is paused. The [earlier operating guide](../../ops/auth.md) preserves that
proof. Explicit legacy configuration still supports standard OIDC/OAuth with local
grants and hashed sessions. Native WorkOS mode is a separate, mutually exclusive
configuration and does not fall back to those grants.

The Rust adapter supports exact issuer paths/trailing slashes, standard audience
strings/arrays, RS256, PKCE S256, subject-bound verified-email userinfo, bounded
same-origin discovery/JWKS, and scoped resource tokens. Tests use two independent
OIDC issuers to preserve a principal's workspace grants through replacement.
This is not a guarantee of credential or passkey migration. The local Rauthy/Codex proof passed browser login, native OAuth, MCP discovery and
workspace access, plus refresh with the resource preserved. Bunny/PS-5 deployment
proof remains separate release evidence.

Rauthy's resource-indicator support excludes upstream-brokered social sign-in
according to its current documentation. Use local provider authentication for
the initial proof and test social/MCP separately before promising it. Never
relax the resource audience check as a workaround. Rauthy also requires persistent
cache storage even when its main database is PostgreSQL. These are actual
operating constraints, not properties solved by the analytics OIDC adapter.

## WorkOS AuthKit and Connect

WorkOS is the selected managed provider. Its [MCP integration](https://workos.com/docs/authkit/mcp)
provides the authorization server, resource indicators, audience-bound tokens, and
CIMD, with DCR available for older clients. Analytics still validates tokens and
serves protected-resource metadata. Its [Rust SDK](https://workos.com/docs/sdks/rust)
allows integration without a TypeScript application backend.

The [token-claims contract](https://workos.com/docs/authkit/connect/token-claims)
distinguishes two registration paths:

- Dashboard-created OAuth applications can receive explicitly assigned permission
  scopes. This may fit deliberately registered internal read/write MCP clients;
  actual client registration and loopback redirect compatibility need a smoke test.
- CIMD/DCR clients cannot receive per-client permission scopes through normal
  configuration. Additional permission scopes need support-configured defaults
  across the environment. Do not silently replace separate client read/write
  scopes with a user's workspace role; those are different restrictions.

The first interoperability proof can use separate dashboard-created
[public OAuth applications with PKCE](https://workos.com/docs/authkit/connect/oauth)
for read-only and read/write access. This requires the intended MCP client to
accept a configured client ID and send the resource parameter; do not assume
all clients support that path.

Configure the exact MCP resource URL and reject other audiences. Resolve the
current active WorkOS membership and effective role permissions for each protected
operation. Intersect MCP client scopes with those permissions. Keep only stable
provider-to-product ID mappings locally; stale local grants cannot widen access
in this mode. Verify email ownership through authenticated provider responses
before granting internal access; accepting an invitation alone is insufficient.

The [public DPA](https://workos.com/legal/data-processing-addendum) covers US
transfers and names US storage/access in its transfer questionnaire. It does not
establish an EU-only AuthKit deployment. Confirm applicable processing terms,
regional options, and export capabilities before production. Provider replacement
also requires organizations, memberships, roles, permissions, and product ID
mapping reconciliation. Standard OAuth support does not prove a lossless exit.

## Earlier Better Auth comparison

[Better Auth](https://better-auth.com/docs/introduction) is a TypeScript auth
framework. Its [Infrastructure offering](https://better-auth.com/docs/infrastructure/introduction)
extends an application-hosted auth service; it does not remove that service's
hosting responsibility. Using it would add a Node/TypeScript identity container
beside analytics, which is allowed by ADR 0010.

Its [MCP plugin](https://better-auth.com/docs/plugins/mcp) is a separate integration
candidate. Validate the selected stable release's OAuth/resource/scope behavior
against the intended MCP clients; do not assume a beta profile is available in
stable. PostgreSQL identity storage permits an operator-owned data path, but adds
identity-service operations, upgrades, key management, and backup verification.

## Next evidence

Keep product configuration operations behind verified internal IDs and permission
context. Before production, validate the registration path and real MCP client, browser sessions,
verified email, exact issuer/audience, read/write scope separation, revocation,
recipient forwarding, identity and grant export, and the applicable data-processing terms.
No custom OAuth authorization server is a fallback for an integration gap.
