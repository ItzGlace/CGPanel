# CGPanel documentation

CGPanel is a Rust hosting panel for a dedicated development server. Start with the guide matching your role. Bundled copies are also available in the panel's **Documentation** page; administrators additionally have **Admin API**.

| Guide | Contents |
| --- | --- |
| [Getting started](GETTING-STARTED.md) | Applications, domains, databases, bots and routine tenant tasks |
| [Administrator operations](OPERATIONS.md) | Installation/upgrades, accounts, service checks, recovery and troubleshooting |
| [Monitoring, backups, HTTPS and proxies](V0.2-GUIDE.md) | Detailed setup and feature limits |
| [Administrator API guide](API.md) | Tokens, session/CSRF authentication, curl/Python examples and common workflows |
| [Endpoint reference](API-REFERENCE.md) | Methods, paths, ownership, payloads and response shapes |
| [OpenAPI 3.0 JSON](openapi.json) | Machine-readable contract for importing into API tools |
| [Security policy](../SECURITY.md) | Isolation, credential handling and current security boundaries |
| [Roadmap](ROADMAP.md) | Work still required beyond this alpha |
| [Changelog](../CHANGELOG.md) | Version history |
| [v0.2 jobs and diagrams](RELEASE-0.2.md) | Implementation plan and feature verification evidence |

The OpenAPI document and endpoint reference are generated together by `python3 scripts/build_api_docs.py`. CI runs `python3 scripts/build_api_docs.py --check` to catch stale generated documents. Update the generator alongside endpoint changes, and run the isolated `tests/admin_api.py` suite against a built binary when changing authentication or token behavior. It uses a temporary database and loopback listener; no live credentials or host broker are required.

- [Mail hosting](MAIL.md): mailbox setup, TLS, DNS, delivery security and API operations.

- [Version 0.5 workspace guide](V0.5-GUIDE.md): services, budgets, recovery, console, protections and MFA.
