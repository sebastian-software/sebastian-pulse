# Repository instructions

- Use US English for code, documentation, commits, and GitHub content.
- Keep the service portable: standard container HTTP and PostgreSQL boundaries.
- Read the product brief and implementation plan before expanding product scope.
- The backend foundation is Rust (Axum/Tokio/SQLx); browser SDK/UI code will
  remain TypeScript. Workspace configuration persistence, RLS, optional standard
  OIDC login, protected configuration HTTP routes, and scoped MCP exist.
  WorkOS AuthKit/Connect is the selected managed identity and authorization
  target. Native AuthKit/Connect routes and request-scoped PostgreSQL RLS are
  implemented and fixture-tested; a real provider/client proof is in progress.
  WorkOS owns users, organizations, memberships, roles, and permission mappings; preserve stable product IDs and transaction-local
  RLS. The previous Rauthy rollout is paused. Live deployment, invitations,
  collection, report access, and browser UI remain pending.
- Run `scripts/check.sh` and the relevant real-PostgreSQL/container checks.
  Keep SQLx offline metadata current; `scripts/prepare-sqlx.sh` uses an explicit
  migrated test database. Normal builds must not need production database access.
- Never provide migration credentials to the application or log secret values.
- Migration files are immutable after application; add a numbered SQL file.

## Product and architecture decisions

- Read [the ADR index](docs/adr/README.md) and relevant accepted records before
  product-scope, policy, or architecture work. Follow
  [the living ADR convention](docs/adr/0002-living-architecture-decisions.md).
- Update an existing topic in place when the task changes its decision; keep
  its filename stable, preserve `Date`, and update `Updated`, `Change note`,
  and the index as required.
- Use [ADR 0004](docs/adr/0004-product-scope-and-priorities.md) for feature scope
  and priorities; read the linked product decisions for the affected behavior.
- Keep requirements, the implementation plan, and implementation-status notes
  consistent with the decision. Keep unresolved choices explicit.
