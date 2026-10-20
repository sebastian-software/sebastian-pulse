# Public test keys

The RSA private keys in this directory are intentionally public, disposable
protocol-fixture material. They sign synthetic OIDC/MCP tokens in integration
tests only. Never use these keys, users, issuers, or credentials in a deployed
identity provider. The final application image contains no test fixtures.
