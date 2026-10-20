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
| Secrets | Runtime database URL/optional CA and, when enabled, the four AUTH_* settings; migration credentials in release runner only. |
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

Identity runs as a separate maintained provider under [ADR 0010](../../docs/adr/0010-external-identity-and-workspace-grants.md).
See the [Rauthy contract](https://github.com/sebastian-software/sebastian-auth/blob/main/ops/bunny/README.md) for its separate persistent cache
volume, dedicated database, EU-processing checks, and proven local MCP flow.
The analytics application itself still needs no persistent volume.
