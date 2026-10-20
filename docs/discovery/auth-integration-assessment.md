# Authentication integration assessment

Assessed October 20, 2026. The accepted direction is a separate maintained identity
provider, including managed identity, under [ADR 0010](../adr/0010-external-identity-and-workspace-grants.md).
No provider account, production configuration, or subscription has been created.
This assessment does not select a provider or change the required access model.

## WorkOS AuthKit and Connect

WorkOS is the managed candidate to validate first. Its [MCP integration](https://workos.com/docs/authkit/mcp)
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
