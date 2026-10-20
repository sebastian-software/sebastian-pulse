# Public test keys

The RSA private keys in this directory are intentionally public, disposable
protocol-fixture material. They sign synthetic OIDC/MCP tokens in integration
tests only. Never use these keys, users, issuers, or credentials in a deployed
identity provider. The final application image contains no test fixtures.

`rauthy-app.env.example` configures the optional local real-provider proof described
in [the identity integration guide](../../ops/auth.md). The provider fixture and
operating configuration live in the separate sebastian-auth repository. Automated
OIDC/MCP tests here use the synthetic protocol fixtures without that repository.
