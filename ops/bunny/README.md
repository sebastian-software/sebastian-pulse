# Bunny deployment target

This is the deployment contract for the initial live setup. The PlanetScale
cluster `sebastian-pulse`, branch `main`, was provisioned on October 20, 2026.
The production foundation is running at https://pulse.sebastian-software.de as of
October 21, 2026. Public readiness, the pinned release, TLS, EU routing, cache
bypass, and runtime database restrictions passed live checks. AuthKit production
enrollment, verified admin browser access, scoped MCP operations/refresh, and
Codex login/tool discovery also passed. See
[`production.workos.json`](production.workos.json) for the non-secret deployment receipt.

| Setting | Initial target |
| --- | --- |
| Image | Tested GHCR image digest, `linux/amd64`. |
| Region | One intended EU region, initially Frankfurt; verify routing/base-region settings. |
| Instances | One continuously running instance; no request-dependent wakeup for future jobs. |
| Port | HTTP `3000` or the explicitly configured `PORT`. |
| Health | `/health/live` for liveness, `/health/ready` for traffic readiness; no cached responses. |
| Shutdown | SIGTERM, platform grace at least 25 seconds; app deadline 20 seconds. |
| Storage | No persistent app volume; authoritative state belongs in PostgreSQL. |
| Secrets | Runtime database URL/optional CA plus native WorkOS credentials and session-sealing key; migration credentials in release runner only. |
| Database | PlanetScale Postgres PS-5 ARM HA in Frankfurt, PostgreSQL 18.6; one primary and two replicas, 10 GB, $15/month. |

Release sequence: verify CI → run the same image's migrator with a separate
credential → start/update the app image → check readiness and release ID. The
foundation rejects unexpected migration history, so plan a maintenance window
for schema changes until compatible schema ranges are implemented. Keep the
prior tested image for compatible application rollback.

Before marking the live W1 target proof complete:

1. Establish the actual PS-5 roles/grants and verify certificate/hostname trust
   over the real Bunny-to-PlanetScale connection. Keep TLS verification enabled.
2. Run a synthetic write/read and restricted-role negative checks there. Confirm
   the pool budget and the actual Postgres major, not only the SKU name.
3. Configure the public GHCR digest pull path, owned domain, probes, region,
   minimum instances, secret references, and observed shutdown grace.
4. Record the deployed digest and verify health, reconnect, and release identity.

The CI token publishes images only. A future Bunny deployment workflow needs a
separately protected deployment credential. Bunny's documented deploy action uses
an account API key; keep that key out of the runtime and do not claim narrower
scope without verifying it.

References: [images](https://bunny.net/docs/magic-containers/deploy),
[health](https://bunny.net/docs/magic-containers/health-checks),
[shutdown](https://bunny.net/docs/magic-containers/graceful-shutdown),
[release action](https://bunny.net/docs/magic-containers/deploy-with-github-actions).

Identity and authorization are managed by WorkOS AuthKit/Connect under
[ADR 0010](../../docs/adr/0010-external-identity-and-workspace-grants.md).
The app needs no persistent volume and no separately deployed Auth database.
See the [WorkOS guide](../workos.md) for permissions, mapping, and live proof gates.

## Native WorkOS deployment template

[`application.workos.template.json`](application.workos.template.json) preserves the
Frankfurt/container/probe contract and uses native WorkOS settings. It is a
**template, not a deployable pinned artifact**: supply the checked post-merge
commit tag and exact GHCR digest through `PULSE_IMAGE_TAG` and `PULSE_IMAGE_DIGEST`.
Do not send unresolved `$env:` values or the earlier Rauthy-era image to Bunny.

[`release.workos.json`](release.workos.json) records the published native WorkOS
candidate from merged PR #6. Its main-branch CI run passed, including the image
lifecycle checks. The public registry manifest was checked independently for the
same digest and `linux/amd64` platform on October 21, 2026. Use its `imageTag` and
`imageDigest` as the two release inputs. Live health responses and the Bunny
container inventory independently confirmed the same release and digest. WorkOS
Production and the restricted PostgreSQL credentials were provisioned on
October 21, 2026.

[`env.workos.production.template`](env.workos.production.template) lists the intended
production secret references. The referenced production items are provisioned
in the company vault. Keep environment keys and session-sealing keys separate.
Resolve references and substitute all template inputs in memory; do not save rendered
credential files. Validate a full commit SHA and `sha256:` plus 64 hexadecimal
digest characters before upload. Use the WorkOS production environment and its
exact registered production callback; a staging key is not a production secret.
Keep `BUNNY_API_KEY` in operator tooling, outside the container environment.
Run all four migrations before starting the matching new image.

### Provisioning tool

`scripts/provision-bunny.py` validates the native template, resolves `$env:NAME`
inputs from its process environment in memory, and creates the initial app. It
refuses duplicate application names, mutable image tags, missing digest pins,
known staging WorkOS keys/domains, a different production origin, non-Frankfurt
regions, and operator/migration credentials in the runtime. Provider error bodies are
suppressed because they can echo submitted secrets. WorkOS documents the `sk_`
prefix; a key prefix alone cannot establish its environment. Resolve the verified
production key, client ID, and hosted issuer from the same production item.

```sh
python3 scripts/provision-bunny.py --validate-template
python3 -m unittest discover -s scripts -p 'test_bunny_provision.py'
# Inject the release inputs and credentials through the operator's secret store.
python3 scripts/provision-bunny.py --check
python3 scripts/provision-bunny.py --create
```

The tool does not resolve `op://` references itself. Supply the values named in
`env.workos.production.template` through a secret-manager runner; keep rendered
credentials out of files and shell arguments. `--check` makes no network calls.
`--create` returns only the new application ID and does not issue an explicit
deploy request. Creation can provision application resources; treat the endpoint
as public during setup. Configure the endpoint/pull zone and verify the owned domain, TLS, cache bypass, and Host
forwarding before deploying through Bunny's application deploy endpoint.
Inspect an existing app before updating it; the tool never overwrites one.

### DNS and CDN boundary

Public DNS for `sebastian-software.de` is authoritative at Cloudflare, confirmed
on October 21, 2026. The same-named Bunny DNS zone is not authoritative. Create
only the `pulse` CNAME in the existing Cloudflare zone, using
[`pulse-cname.template.json`](../cloudflare/pulse-cname.template.json) and the
actual `publicHost` returned by the new Magic Containers endpoint. Keep the
Cloudflare proxy disabled; the application and HTTP edge run on Bunny. Refuse an
existing conflicting record rather than overwriting it. Do not change nameservers.

Apply [`pull-zone.settings.json`](pull-zone.settings.json) to that endpoint's
assigned pull zone. It preserves the requested Host header, selects EU-only CDN
routing, and disables logging, origin shielding, and cache features. Add
[`no-cache.edge-rule.json`](no-cache.edge-rule.json) without replacing existing
rules or changing the Magic Containers origin configuration. Omit `Guid` on
creation; Bunny generates it. Find the unique description on subsequent runs and
use its returned `Guid` for updates. Refuse duplicate matching descriptions.
The rule bypasses caching for every URL and sets `Cache-Control: no-store`. The
Rust service also sets that header. Bunny normalizes the EU filter to `eu`
and named edge actions/triggers to numeric enum values in API responses; compare
their meaning when verifying the read-back.
For any reused zone, purge previously cached responses before enabling traffic.

Add the owned hostname to the same pull zone, wait for authoritative DNS, request
the free certificate, and enable forced HTTPS. Verify all settings by reading
them back before deployment, then exercise both the public origin and repeated
authenticated responses. A configuration template alone is not live evidence.

References: [Core API specification](https://bunny.net/docs/api-reference/core/openapi.json),
[routing filters](https://bunny.net/docs/cdn/performance/routing-filters),
[custom hostnames](https://bunny.net/docs/cdn/custom-hostname).

## Previous OIDC application manifest

The retained legacy [`application.json`](application.json) pins the previous
OIDC/Rauthy-era public GHCR image from
`b4189a7d089b951c75b2556dbe5284c1656b235f` by digest. It restricts deployment to
Frankfurt (`DE`), one running instance, and a 30-second shutdown grace. Readiness
uses `/health/ready`; liveness uses `/health/live`. It does not contain the native WorkOS implementation. Do not deploy this legacy
manifest for the selected WorkOS direction.

[`env.production.template`](env.production.template) contains only 1Password
references. Resolve the two `$env:NAME` runtime inputs in memory before sending
the manifest to the Bunny API. `BUNNY_API_KEY` is an operator credential and must
never enter the container environment. Migration credentials are deliberately
absent. Run the pinned image's `migrate` command separately with the migration
credential before deploying the app.

Creating the app does not finish the rollout. Configure the assigned pull zone
with caching disabled for every current route, EU-only routing, the owned
`pulse.sebastian-software.de` hostname, forwarded Host, and forced HTTPS before
starting it. Prove readiness and the real login/session path over the owned
origin; a valid manifest does not establish those behaviors.
