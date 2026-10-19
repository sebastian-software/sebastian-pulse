# Repository instructions

- Use US English for code, documentation, commits, and GitHub content.
- Keep the service portable: standard container HTTP and PostgreSQL boundaries.
- Read the product brief and implementation plan before expanding product scope.
- Run `pnpm check` and the real-database/container checks relevant to a change.
- Never provide migration credentials to the application or log secret values.
- Migration files are immutable after application; add a numbered SQL file.
