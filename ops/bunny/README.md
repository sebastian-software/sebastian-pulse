# Bunny deployment target

This is the deployment contract for the initial live setup. The PlanetScale
cluster `sebastian-pulse`, branch `main`, was provisioned on October 20, 2026.
Bunny deployment and the real database/authentication path remain in progress;
the local and CI checks alone do not establish production readiness.

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

[`env.workos.production.template`](env.workos.production.template) lists the intended
production secret references; its WorkOS production item is not provisioned.
Create production credentials only as part of the authorized rollout. Resolve
references and substitute all template inputs in memory; do not save rendered
credential files. Validate a full commit SHA and `sha256:` plus 64 hexadecimal
digest characters before upload. Use the WorkOS production environment and its
exact registered production callback; a staging key is not a production secret.
Keep `BUNNY_API_KEY` in operator tooling, outside the container environment.
Run all four migrations before starting the matching new image.

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
