# Authentication integration assessment

Assessed October 20, 2026. The accepted direction is a separate maintained identity
provider, including managed identity, under [ADR 0010](../adr/0010-external-identity-and-workspace-grants.md).
No provider account, production configuration, or subscription has been created.
The current deployment direction prioritizes EU-operated identity and validates
self-hosted Rauthy. This comparison preserves the WorkOS findings without
authorizing its provisioning or changing the local access model.

## EU-operated direction and standard boundary

US identity processing is a material adoption barrier. Validate self-hosted
Rauthy on Bunny, one EU region, with a dedicated provider database/role and
persistent cache volume. See the [operating guide](../../ops/auth.md).
The analytics app uses standard OIDC/OAuth without vendor user/organization APIs;
local principal UUIDs, grants, and hashed sessions remain in PostgreSQL.

The Rust adapter supports exact issuer paths/trailing slashes, standard audience
strings/arrays, RS256, PKCE S256, subject-bound verified-email userinfo, bounded
same-origin discovery/JWKS, and scoped resource tokens. Tests use two independent
OIDC issuers to preserve a principal's workspace grants through replacement.
This is not a guarantee of credential or passkey migration. The local Rauthy/Codex proof passed browser login, native OAuth, MCP discovery and
workspace access, plus refresh with the resource preserved. Bunny/PS-10 deployment
proof remains separate release evidence.

Rauthy's resource-indicator support excludes upstream-brokered social sign-in
according to its current documentation. Use local provider authentication for
the initial proof and test social/MCP separately before promising it. Never
relax the resource audience check as a workaround. Rauthy also requires persistent
cache storage even when its main database is PostgreSQL. These are actual
operating constraints, not properties solved by the analytics OIDC adapter.

## WorkOS AuthKit and Connect

WorkOS was the managed candidate assessed before the EU-operated identity requirement. Its [MCP integration](https://workos.com/docs/authkit/mcp)
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

Configure the exact MCP resource URL and reject other audiences. Preserve local
workspace/recipient grants rather than importing the provider's organization model.
Verify email ownership through validated provider claims/API before binding report
access; accepting an invitation alone is insufficient.

The [public DPA](https://workos.com/legal/data-processing-addendum) covers US
transfers and names US storage/access in its transfer questionnaire. It does not
establish an EU-only AuthKit deployment. Confirm applicable processing terms,
regional options, custom-domain costs, and credential/identity export before
provisioning. Standard OAuth support does not prove a lossless provider exit.

## Better Auth

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

Keep W2 configuration operations provider-independent. Before enabling login,
validate the preferred registration path and real MCP client, browser sessions,
verified email, exact issuer/audience, read/write scope separation, revocation,
recipient forwarding, identity export, and the applicable data-processing terms.
No custom OAuth authorization server is a fallback for an integration gap.
