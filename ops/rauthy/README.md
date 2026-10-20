# Rauthy: local proof and EU deployment contract

The local stack runs Rauthy **0.37.2**, PostgreSQL **18.6**, and Mailpit, pinned by
digest. It uses separate PostgreSQL/cache volumes and does not share the analytics
database. All example accounts, passwords, client secrets, and encryption keys
are deliberately public test fixtures. Never deploy them.

## Start locally

From this directory:

```sh
cp .env.example .env
cp config.toml.example config.toml
cp secrets.toml.example secrets.toml
cp bootstrap/clients.json.example bootstrap/clients.json
cp bootstrap/scopes.json.example bootstrap/scopes.json
docker compose -p analytics-auth-proof up -d --wait
curl -fsS http://localhost:8180/auth/v1/.well-known/openid-configuration
```

Ports: Rauthy `8180`, Mailpit UI `8025`, and separate PostgreSQL `55433`, all bound
to loopback. The issuer is exactly `http://localhost:8180/auth/v1/`, including the
trailing slash. The synthetic admin is `admin@example.invalid`; its password is
in the local example configuration. Open signup is disabled. Recovery mail stays
in Mailpit. The main database uses two connections; `/app/data` persists the
provider cache. No `LOCAL_TEST=true` demo bypass is used.

The released configuration schema requires `bootstrap_dir` inside `[bootstrap]`,
explicit encryption-key references inside `[encryption]`, and plain bootstrap
account settings or their documented environment overrides. Bootstrap account
fields do not resolve `$SECRETS`; database/cluster/encryption fields do. The
browser client fixture has a secret containing exactly 64 characters. This setup
was checked against the running released image, rather than only current docs.

## Register the resource and provision a local principal

Bootstrap creates confidential `analytics-browser`, public `analytics-mcp`
(read/write), and public `analytics-mcp-read` (read only). Native clients use
S256 and standard refresh grants. Their fixture access lifetime is 60 seconds
so refresh can be tested immediately; use 300 seconds for the production profile.
The browser client only uses authorization code; the analytics app stores no
provider refresh/access tokens in its browser-session table.

Rauthy reads bootstrap JSON only for an empty database. Its released bootstrap
client schema omits `allowed_resources`, so set the exact resource after first
boot through the management API/UI. For **this disposable local database only**,
the equivalent setup is:

```sh
docker compose -p analytics-auth-proof exec -T postgres psql -U rauthy -d rauthy \
  -v ON_ERROR_STOP=1 -c "UPDATE clients SET allowed_resources='http://127.0.0.1:3001/mcp', default_aud=NULL WHERE id IN ('analytics-mcp','analytics-mcp-read');"
docker compose -p analytics-auth-proof restart rauthy
```

The restart clears cached client configuration after the fixture-only SQL edit.
Use the provider management interface for production changes. Do not grant its
database role access to the analytics database.

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
. ops/rauthy/app.env.example
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

## Bunny production profile and release gates

Run identity as a separate app so an analytics rollout does not restart login.
Use one continuously running replica in Frankfurt, disable other/base deployment
regions, and verify those settings on the actual platform. Keep `/app/data` on
an encrypted persistent volume; each Bunny replica receives independent storage,
so do not treat autoscaling as replication. This is a small single-instance
starting point with an explicit recovery plan, not high availability.

Use an owned stable identity domain, for example `https://auth.example.com/auth/v1/`,
and independent owned analytics/MCP/report URLs. Configure HTTPS termination,
Rauthy's public URL, exact trusted proxies/client-IP extraction, and passkey RP
origin/domain consistently. Validate MCP Host/Origin against `AUTH_PUBLIC_URL`.
Keep first-party login secrets out of browser bundles. Before exposure, establish
ingress throttling for `/auth/login` and provider endpoints; the app's 1,000 live
attempt cap bounds storage but an attacker could fill it for up to ten minutes.

Give Rauthy a dedicated PostgreSQL database/role on the EU cluster; its own schema
migrations must not confer privileges over analytics data. Require PostgreSQL TLS
with certificate verification (`pg_tls='require'`, `pg_tls_no_verify=false`) and
verify the real target path. Start with two provider connections plus the
analytics pool of five, accounting separately for migration/maintenance usage.
Keep one provider instance until its cache/cluster setup has been explicitly tested.

Generate fresh production encryption/cluster keys, browser-client secret, and
operator credentials. Use provider-admin MFA and disable open signup, DCR, and
CIMD unless explicitly needed. Register separate read and read/write native clients
with exact allowed resources and observed callbacks. Never deploy the example
passwords/keys, Mailpit, local plaintext settings, or the 60-second test lifetime.
Use an EU-approved mail provider and test verification, reset, delivery failure,
and recovery before recipient-facing login.

Back up the provider database together with encryption/signing keys, configuration,
and persistent cache state; a database dump without decryption keys is not a
recovery plan. Restore into an isolated environment, verify login and grants, and
revoke/reconcile credentials as required. Record account, authentication-log,
backup, and administrative-audit retention. The analytics app's stored deadlines
must still deny expired state after a restore. Exercise the provider-exit procedure
in [ADR 0010](../../docs/adr/0010-external-identity-and-workspace-grants.md).

Complete Bunny's applicable DPA and check actual locations/access for identity
records, logs, volumes/backups, support, control plane, and mail. EU app placement
is useful evidence, not a blanket statement about every subprocessor. Bunny's
public list includes US providers for some services; constrain the actual service
and subprocessor path before claiming EU-only identity processing. No production
account, contract, DNS change, subscription, or live deployment was created here.

## Stop or reset the local proof

`docker compose -p analytics-auth-proof down` retains the proof's volumes.
Adding `--volumes` removes only this disposable provider's data. Preserve any
identity data you need before resetting. Do not reset the main analytics volumes.

## Sources

- [Rauthy release](https://github.com/sebadob/rauthy/releases/tag/v0.37.2)
- [Released configuration](https://github.com/sebadob/rauthy/blob/v0.37.2/config.toml)
- [Bootstrap schema](https://github.com/sebadob/rauthy/blob/v0.37.2/src/data/src/migration/bootstrap/types.rs)
- [Resource indicators](https://sebadob.github.io/rauthy/work/resource_indicators.html)
- [Persistent cache](https://sebadob.github.io/rauthy/config/ha.html)
- [Codex MCP OAuth](https://learn.chatgpt.com/docs/extend/mcp#oauth-client-registration-and-callbacks)
- [Bunny persistent volumes](https://bunny.net/blog/magic-containers-now-supports-persistent-volumes-storage-that-survives-restarts-and-redeployments/)
- [Bunny GDPR/DPA guidance](https://bunny.net/gdpr/)
- [Bunny subprocessors](https://bunny.net/gdpr/sub-processors/)
