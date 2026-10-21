# WorkOS integration proof

## Status

The selected target is managed WorkOS AuthKit and Connect, with the default hosted
login domain and WorkOS-managed users, organizations, memberships, roles, and
permissions. See [ADR 0010](../docs/adr/0010-external-identity-and-workspace-grants.md).
A WorkOS staging project, default hosted AuthKit domain, and exact local callback
`http://127.0.0.1:3002/auth/callback` are configured. A staging key with a 24-hour
lifetime is stored in 1Password and has passed an HTTP 200 API check. A local
isolated PostgreSQL database is migrated for the real browser/MCP proof.
Real AuthKit registration, verified email, PKCE callback, and the Pulse browser
session passed locally on October 21, 2026. An authenticated user without an
organization receives no product IDs or internal write access.
The staging pilot organization, reader/admin roles, and two public Connect
clients are configured. Real scoped MCP operations and token refresh passed
against isolated local PostgreSQL data; see the evidence below.
The native WorkOS release is running on Bunny in Frankfurt as of October 21,
2026. Production uses `https://wealthy-aspiration-91.authkit.app`, the exact
`https://pulse.sebastian-software.de/auth/callback` callback, and a separate
production API key/session-sealing key in the company 1Password vault. Email +
Password is enabled with WorkOS's Strong policy: minimum 10 characters, zxcvbn
score at least 3, and breached-password rejection. The production organization,
reader/admin roles, and product workspace binding are configured. Initial
production enrollment and authenticated browser/MCP proof remain pending; public
health and unauthenticated denial checks passed. See the
[Bunny deployment receipt](bunny/production.workos.json).

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

The live provider issued the browser session with
`https://api.workos.com/user_management/<WORKOS_CLIENT_ID>` as its issuer.
Validation accepts that exact client-specific issuer, the documented legacy API
issuer, and the explicitly configured AuthKit issuer. Another application's
client-specific issuer is rejected. Session tokens still originate only from
authenticated code/refresh responses and locally authenticated encryption;
this rule does not turn browser session JWTs into accepted MCP bearers.

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
`<Pulse origin>/mcp`, and that URI must be registered under Connect's MCP resource
indicators. Send `resource` in the authorization request, authorization-code token
exchange, and refresh request. The live staging provider returned a non-resource
audience when the code-exchange request omitted it. Keep Pulse's exact audience
validation in place.
Setting a default resource did not replace explicit resource binding for these
manually registered clients. Never relax server audience validation to compensate.

Request `offline_access` together with `pulse:read` (and `pulse:write` for the
write client) to receive a refresh token. Keep authorization-code and refresh
responses in client memory or the operating-system credential store, never logs.
Do not enable unrestricted callbacks or skip resource validation to accommodate
a client. Dynamic registration is a separate proof:
WorkOS documents restricted scope assignment for dynamically registered clients.

### Local MCP client setup

The local proof uses Pulse at `http://127.0.0.1:3002`, a public S256 Connect client,
and the observed callback `http://127.0.0.1:34627/callback/gNcopO_wGpkQ`. The callback
path is client-generated and depends on the MCP URL; inspect the actual login
request before registering a different URL or client version. The Connect client
ID is public and differs from the AuthKit environment client ID used by Pulse.

Codex CLI supports a pre-registered client with these settings. Replace the
placeholder with the appropriate public Connect client ID:

```toml
mcp_oauth_credentials_store = "keyring"

[mcp_servers.pulse]
url = "http://127.0.0.1:3002/mcp"

[mcp_servers.pulse.oauth]
client_id = "<public Connect client ID>"
callback_port = 34627
```

```sh
codex mcp login pulse --scopes pulse:read,offline_access
```

Use the separately registered write client and request
`pulse:read,pulse:write,offline_access` when writes are needed. A successful login
alone does not prove usable MCP access: verify initialization, tool discovery,
an allowed read, a denied write for the read client, and access after refresh.
Codex completed a real read-client login, MCP initialization, and discovery of all
13 tools through its app-server client. The operations and refresh evidence below
comes from a separate minimal OAuth/HTTP MCP client; Codex tool invocation and
automatic refresh remain separate checks. The temporary proof credential was removed from the keyring
afterward; the example does not change existing client configuration. DCR and CIMD remain disabled in staging.

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

Live evidence on October 21, 2026, using the checked application revision
`866f759aef5ea1a240abd5e1888648321dfd6e0f`:

| Check | Result |
| --- | --- |
| AuthKit registration, verified email, callback, sealed browser session | Passed; organization-free sign-in has no internal access. |
| Pilot admin membership and exact organization/workspace mapping | Passed; browser session resolves managed write access. |
| Two public clients, S256 PKCE, no client secret | Passed for read and read/write scopes. |
| Exact resource audience and selected pilot organization | Passed on code exchange and resource audience rechecked after refresh. |
| MCP initialization and tool discovery | Passed for both clients. |
| Codex app-server MCP client | OAuth login and authenticated discovery of all 13 tools passed. |
| Own workspace read / other workspace denial | Passed for both clients. |
| Read-only client acting for an admin user | Read succeeded; project creation denied. |
| Read/write client acting for an admin user | Synthetic project created, updated, and deleted successfully. |
| Refresh grant and subsequent MCP read | Passed for both clients with `offline_access` and explicit `resource`. |
| Altered access-token signature | HTTP 401 for both clients. |

Tokens stayed in memory during the OAuth/HTTP smoke checks. The isolated database
contains synthetic workspaces; no live analytics data was read or written. These
results establish this staging configuration, not production availability or
compatibility with every MCP client.

Remaining checks:

- Real AuthKit sign-in and verified email passed locally. Check recovery delivery,
  session renewal, logout, account disabling, and concurrent refresh resilience.
- Exercise the intended MCP client's tool execution and automatic refresh in
  addition to its login and discovery. Keep the observed callback, exact resource, and assigned
  permission scopes; do not infer compatibility from a successful redirect.
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
- [Connect token and refresh grants](https://workos.com/docs/reference/workos-connect/token)
- [Codex MCP configuration](https://learn.chatgpt.com/docs/extend/mcp)
