# Changelog

## 0.3.0 — 2026-09-24

- Bundled Documentation page with tenant setup, administrator operations and API guides; GitHub documentation index and troubleshooting instructions.
- Administrator API center with 41 searchable operations, request examples, response schemas, curl snippets and an OpenAPI 3.0 download.
- Administrator bearer tokens with read/full scope, 1–90 day expiry, optional CIDR restrictions, hash-only storage, usage timestamps and revocation. Session-only credential management and password-change revocation protect the token lifecycle.
- Administrator system overview and cursor-paginated audit endpoints. Existing management endpoints accept authorized bearer requests.
- A single generator maintains the endpoint reference and OpenAPI contract; CI checks generated documentation for drift. Dedicated-host tests verify scope, IP restrictions, expiry, revocation, secret handling, sessions and tenant denial.

## 0.2.0 — 2026-09-24

This development alpha adds website observability, portable backups and per-application proxy routing to the Rust hosting panel.

- Website availability checks, response history, outage/recovery state and Telegram alerts.
- Opt-in page views, daily unique-IP estimates, referrers, click-density heatmaps and technical SEO inspection, with hashed identifiers and configurable retention.
- Five-field cron with IANA timezones, container-only execution and run history.
- Full `.cgp` ZIP archives containing website files, selected SQL databases, application settings, domains and DNS metadata. Authenticated downloads and workspace/SQL restore are included.
- Scheduled backups to Telegram, S3 or pinned SSH/SFTP destinations, with retries, local retention, job history and optional SOCKS5 transport.
- Let's Encrypt HTTP/local DNS/Cloudflare DNS validation, renewal timer, administrator panel-IP certificate requests, BIND zone export and Cloudflare trusted-proxy support.
- Enforced SOCKS5 application TCP/DNS routing, including ordinary PHP sockets, with administrator locks and no direct fallback.
- Persistent background jobs, protected integration secrets, responsive Tailwind pages and reduced-motion-aware animations.
- Additive upgrade tooling, recovery snapshots and integration tests.

See [the setup and recovery guide](docs/V0.2-GUIDE.md) and [release verification](docs/RELEASE-0.2.md). This remains an alpha: it is not complete cPanel parity, globally distributed monitoring, full server disaster recovery or protection against upstream bandwidth saturation. Real provider delivery and certificate issuance require configuration. The restore UI overlays files and SQL into existing resources; archived domain/DNS/environment settings require administrator recovery. Archives are not encrypted at rest.

## 0.1.0

Initial Rust hosting alpha with tenant management, assigned domains and DNS, rootless application runtimes, restricted terminals, files, MariaDB/PostgreSQL access controls, basic backups, security controls and a Tailwind interface.
