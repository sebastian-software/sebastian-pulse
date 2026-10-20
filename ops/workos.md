# WorkOS integration proof

## Status

The selected target is managed WorkOS AuthKit and Connect, with the default hosted
login domain and WorkOS-managed users, organizations, memberships, roles, and
permissions. See [ADR 0010](../docs/adr/0010-external-identity-and-workspace-grants.md).
A WorkOS staging project, default hosted AuthKit domain, and exact local callback
`http://127.0.0.1:3002/auth/callback` are configured. A staging key with a 24-hour
lifetime is stored in 1Password and has passed an HTTP 200 API check. A local
isolated PostgreSQL database is migrated for the real browser/MCP proof.
No production login or Bunny application is deployed.

`src/auth/workos.rs` contains the native Rust SDK boundary. Its synthetic tests
cover AuthKit S256/state generation, code exchange, sealed-session tampering and
refresh, transient-versus-terminal refresh failures, provider revocation, active membership/role resolution, permission
changes, and Connect issuer/resource/expiry/delegation checks. It uses the official
`workos` 3.4.0 SDK without its default HTTP transport, so the service retains its
existing reqwest version, fixed API origin, no redirects, four-second request
timeout, response-size cap, and no automatic mutation retries.

Startup selects native WorkOS mode when all its settings are present and rejects
mixed legacy OIDC configuration. Browser login/session/logout, configuration
routes, and MCP use this native boundary. Existing OIDC/local-membership behavior
remains available only through the explicit legacy settings.

Migration 000004 adds private provider-ID bindings and a managed RLS context.
Organization-to-workspace bindings are operator-managed; a principal mapping is
created only after verified WorkOS access. These are identifiers, not local grants.
Managed authorization is checked live and scoped to one workspace for each
request/operation. Stale local memberships cannot widen it. Real PostgreSQL/HTTP
tests exercise login state/CSRF, stable concurrent mapping, tenant/actor isolation,
permissions, stale local admin grants, provider outage, and MCP/browser agreement.

## Intended setup

Use an individual company account for each dashboard operator. Keep credentials
in 1Password and runtime secret injection; never add keys, refresh tokens,
personal account identifiers, or verification codes to the repository. Start in
a non-production WorkOS environment and use synthetic analytics data.

The native configuration loader requires these values together:

| Setting | Purpose |
| --- | --- |
| `WORKOS_API_KEY` | Server-only environment API key. |
| `WORKOS_CLIENT_ID` | AuthKit environment client ID. |
| `WORKOS_AUTHKIT_URL` | Exact HTTPS origin on the default `*.authkit.app` domain, used as the Connect issuer. |
| `WORKOS_COOKIE_PASSWORD` | Independently generated 32-byte key encoded as 64 hexadecimal characters, for SDK session sealing. |
| `AUTH_PUBLIC_URL` | Canonical Pulse origin; HTTP is allowed only for loopback in local/test mode. |

Legacy OIDC issuer/client settings and WorkOS mode must be mutually exclusive.
No setting may silently fall back to a different provider. Keep production and
development environment keys and session-sealing keys separate.

Register the exact browser callback `<Pulse origin>/auth/callback` and sign-out
destination. Configure hosted verification/recovery and the needed authentication
methods using AuthKit. Revisit domain-bound passkeys before production enrollment.

Create `pulse:read` and `pulse:write` in WorkOS, assign the intended permissions to
WorkOS roles, create a pilot organization, and grant pilot users active memberships.
Keep report-only recipients separate from internal analytics roles. The adapter
requires `pulse:read` for any internal access, including a write-capable caller.

For MCP, register explicit public read and read/write Connect applications using
S256 and the actual client's loopback callback. Assign the corresponding permission
scopes to each application. The requested resource must exactly match
`<Pulse origin>/mcp`. Do not enable unrestricted callbacks or skip resource
validation to accommodate a client. Dynamic registration is a separate proof:
WorkOS documents restricted scope assignment for dynamically registered clients.

## Local proof and remaining rollout checks

Set all native settings through a secret manager and start the normal service
binary against a migrated, restricted-role PostgreSQL database. Register its
exact callback before visiting `/auth/login`. New vendor and end-user accounts
are created by their human owners. `/auth/session` returns verified sign-in and
CSRF information, plus product IDs only when current WorkOS permissions and a
workspace binding permit internal access. A signed-in user without organization
access receives no internal grant.

To bind an existing pilot organization to a product workspace, use the trusted
operator connection. Use actual verified IDs and parameterized tooling or psql
variables rather than concatenating caller-supplied text:

```sql
BEGIN;
INSERT INTO analytics.workspaces(id, name)
VALUES (:'workspace_id'::uuid, :'workspace_name');
INSERT INTO analytics.workos_workspaces(client_id, organization_id, workspace_id)
VALUES (:'authkit_client_id', :'organization_id', :'workspace_id'::uuid);
COMMIT;
```

WorkOS remains the only membership/role authority in this mode. The runtime
cannot write organization bindings. A provider change requires an explicit mapping
migration; email matches do not create links. Only the selected organization can
be accessed by the current request; an organization-switch UI is not implemented.

- Complete the real AuthKit sign-in and verified email/recovery delivery. Check
  session renewal, logout, account disabling, and concurrent refresh resilience.
- Configure explicit Connect public read/read-write clients with the intended
  client's observed callback, exact resource, and assigned permission scopes.
  Verify a real internal MCP login and scoped read/write operations.
- Exercise inactive membership, permission removal, cross-organization requests,
  provider outage, and real signing-key rollover. Current automated checks use
  synthetic vendor contracts and do not establish these live outcomes. The staging
  public Connect JWKS was inspected and uses RSA/RS256, matching validation.
- The SDK session cookie is authenticated encrypted data from the WorkOS API.
  Browser tokens are not accepted as MCP bearers. A transient refresh failure
  returns 503 and keeps the cookie; a terminal failure clears it and requires login. Access-token revocation is
  bounded by the configured token lifetime, not an immediate global guarantee.
  Permission checks are live on every protected operation. Set and record the
  actual session/token lifetimes before production.
- Cookie values are capped at 3,800 bytes. Keep auth profiles/claims minimal;
  oversized sealed sessions fail closed. The production cookie is host-only,
  HttpOnly, Secure, and SameSite=Lax. Mutations require Origin plus CSRF; logout
  revokes the WorkOS session and clears local cookies.
- Publish the checked image and render the native WorkOS Bunny template using
  its exact commit tag/digest and production-only secrets. Validate TLS, canonical
  Host/Origin, no caching, readiness, and the real login on the owned URL. The
  earlier Rauthy image/profile is not the native WorkOS deployment artifact.
- Record and rehearse backup/restore of product ID mappings, provider grant
  export, replacement, revocation, and rollback before production release.

Keep WorkOS API calls and SDK models inside the auth boundary. Product operations
should receive verified internal IDs and permission context. Provider replacement
must export and reconcile organizations, memberships, roles, permissions, and ID
mappings in addition to identities; see the
[privacy and exit assessment](../docs/discovery/workos-privacy-and-exit.md).

## Sources

- [Official Rust SDK](https://workos.com/docs/sdks/rust)
- [Hosted AuthKit](https://workos.com/docs/authkit/hosted-ui)
- [Sessions](https://workos.com/docs/authkit/sessions)
- [Session resilience](https://workos.com/docs/authkit/session-resilience)
- [Connect token claims and server-side permissions](https://workos.com/docs/authkit/connect/token-claims)
- [Roles and permissions](https://workos.com/docs/authkit/roles-and-permissions)
- [MCP authentication](https://workos.com/docs/authkit/mcp)
