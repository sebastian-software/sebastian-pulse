# Repository instructions

- Use US English for code, documentation, commits, and GitHub content.
- Keep the service portable: standard container HTTP and PostgreSQL boundaries.
- Read the product brief and implementation plan before expanding product scope.
- The accepted backend target is Rust; the checked-in foundation is still
  TypeScript. Do not describe the Rust port as implemented until it is verified.
- For the existing TypeScript foundation, run `pnpm check` and the relevant
  real-database/container checks. Update the documented verification path as
  part of the Rust port; do not substitute empty passing commands.
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
