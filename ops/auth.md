# Identity integration

Sebastian Analytics owns OIDC login, browser sessions, MCP token validation, and
its PostgreSQL workspace/report grants. The separate private
[sebastian-auth repository](https://github.com/sebastian-software/sebastian-auth)
owns Rauthy configuration, client registration, deployment, upgrades, mail,
backups, and recovery. Its [local fixture guide](https://github.com/sebastian-software/sebastian-auth/blob/main/docs/local-development.md)
starts the provider used below; its [Bunny operating guide](https://github.com/sebastian-software/sebastian-auth/blob/main/ops/bunny/README.md)
owns the provider release gates. An analytics rollout must not redeploy identity.

For the optional real-provider proof, clone both repositories as sibling
checkouts, start the local fixture in sebastian-auth, and register its exact MCP
resource as documented there. It creates `analytics-browser`, `analytics-mcp`,
and `analytics-mcp-read` clients. The normal PostgreSQL/OIDC/MCP integration tests
remain self-contained here and do not require another repository or live provider.
All local example identities and secrets are public test fixtures.

## Provision local analytics access

Provision the analytics principal, workspace, membership, and exact provider link
using a trusted operator connection to the **analytics** database. Obtain the
subject from the verified provider identity, not from an email supplied by a
caller. The following is an operator template, not literal fixture SQL:

```sql
BEGIN;
INSERT INTO analytics.principals (id) VALUES (:'principal_id'::uuid);
INSERT INTO analytics.workspaces (id, name) VALUES (:'workspace_id'::uuid, :'workspace_name');
INSERT INTO analytics.workspace_members (workspace_id, principal_id, role)
VALUES (:'workspace_id'::uuid, :'principal_id'::uuid, 'admin');
INSERT INTO analytics.identity_links (issuer, subject, principal_id)
VALUES (:'exact_issuer', :'verified_subject', :'principal_id'::uuid);
COMMIT;
```

Use parameterized operator tooling or psql `-v` variables; never concatenate
untrusted input. Runtime credentials cannot provision these grants. An unlinked
provider login is denied. Recipient configuration grants no membership.

From the repository root, add OIDC settings to the existing local runtime
configuration, then start the app on port `3001`:

```sh
set -a
. tests/fixtures/rauthy-app.env.example
set +a
APP_ENV=local DATABASE_TLS=disable \
  DATABASE_URL=postgresql://analytics_app:local-runtime-only@127.0.0.1:55432/analytics \
  cargo run --locked -- serve
```

Visit `http://127.0.0.1:3001/auth/login`. The registered browser callback is
`http://127.0.0.1:3001/auth/callback`. A successful verified login ends at
`/auth/session`; inspect workspace data through the authenticated configuration API.

## Codex MCP proof

Use the installed client's callback URL, not an assumed fixed port. Codex CLI
0.163.0 used `http://127.0.0.1:<ephemeral-port>/callback/cQ3lrratGjpe` for this exact
MCP URL. The fixture registers that path and `/callback` for clients using the
issuer-bound stable variant. `access.rfc_8252_enable=true` permits loopback port
selection; no general callback wildcard or dynamic registration is enabled.
A different canonical MCP URL may produce a different callback path.

For an isolated smoke, put Codex state in a fresh private temporary directory,
use file credential storage there, and remove it afterward. Do not alter the
operator's normal Codex configuration. Example command, with that temporary
location supplied through Codex's supported `CODEX_HOME` setting:

```sh
codex \
  -c 'mcp_servers.analytics_auth_proof.url="http://127.0.0.1:3001/mcp"' \
  -c 'mcp_servers.analytics_auth_proof.oauth.client_id="analytics-mcp"' \
  -c 'mcp_servers.analytics_auth_proof.scopes=["mcp:read","mcp:write"]' \
  -c 'mcp_oauth_credentials_store="file"' \
  mcp login analytics_auth_proof --no-browser --scopes mcp:read,mcp:write
```

Keep stdin open for this command. It prints the authorization URL and accepts
either the loopback callback or a manually pasted callback URL. Never publish
that URL with a code, or the resulting token/credential file. For read-only use,
select `analytics-mcp-read` and request only `mcp:read`.

The resource is the exact server URL, so omit Codex `oauth_resource`: the tested
client sends the resource itself. Explicit configuration can duplicate that
parameter in affected Codex releases. PRM intentionally omits optional
`scopes_supported`, allowing an explicitly configured read or read/write profile
to determine the requested scopes. Local roles remain an additional check.

### Observed on October 20, 2026

- Rauthy 0.37.2 with PostgreSQL 18.6 and a persistent cache started successfully.
- Real browser code/PKCE login, ID-token verification, subject-bound verified
  userinfo, local identity mapping, and authenticated workspace HTTP access passed.
- The installed Codex CLI 0.163.0 completed real OAuth login with its public
  client, S256, requested scopes, and exact `resource` parameter. No model-driven
  Codex agent run was required or claimed.
- Its actual access token discovered 13 MCP tools and read the synthetic workspace.
- A standard refresh grant rotated the refresh token; the refreshed access token
  passed the application's resource/scope checks and read the same workspace.
- Automated PostgreSQL/HTTP/OIDC/MCP tests additionally cover reader versus admin,
  read versus write scope, cross-workspace denial, forged signatures, revocation,
  failed key refresh/cooldown, and replacement by a second issuer.

This proves a local provider/client integration. It does not prove Bunny-to-PS-10
networking, production TLS/proxy behavior, mail recovery, MFA/passkeys, backup
restoration, or every client. Rauthy's documented resource processing excludes
upstream-brokered social login; test that path separately before offering it for
MCP. Do not remove audience checks to accommodate it.
