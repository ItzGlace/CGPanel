# Changelog

## 0.6.3

- Roll back when an installer receives SIGINT, SIGTERM or SIGHUP after stopping services. Finish dependency preparation before honoring an earlier cancellation.
- Stage replacement binaries and complete atomic binary backups while the existing panel is online; activate them with renames after services stop.
- Check per-filesystem staging, snapshot and recovery space plus 1 GiB safety headroom before stopping services. Recover from a failed snapshot without restoring incomplete or unchanged data.
- Stop configuration child processes before restoration, prevent overlapping installers, and keep recovery logging working after an SSH/output pipe disconnects.
- Mark a failed install as failed even if its old updater wrapper has exited, instead of leaving a stale installing status.
- Preserve the exact HTTP/version or agent-readiness error and recent service journals before rollback. Bypass external proxies for local health checks and log agent startup stages.
- Add real HTTP/Unix-socket, process-group cancellation, interrupted-update and post-start health-failure regressions.

## 0.6.2

- Wait for APT index/dpkg locks during dependency preparation, before stopping the panel. Retry lock races within a bounded preparation window; preserve repository/signature errors.
- Retain the actual installer failure in update status and root-only persistent logs.
- Explicitly close SQLite connections and restore database/binaries atomically during rollback, including compatibility with the legacy updater's open connection.
- Add real 0.5.2 panel upgrade, held-lock, preparation-failure and rollback regressions with website keepalive and data checks.


## 0.6.1

- Read the fresh installation version from Cargo.toml so updater metadata matches the binaries.
- Publish checked, installable Ubuntu packages automatically from version tags.


## 0.6.0 — release candidate

- Account-scoped MCP tools with hashed, expiring credentials and OAuth S256 PKCE consent.
- Configurable persistent panel login throttling and one-use local challenges.
- Database Studio for MariaDB/PostgreSQL; per-database privileges and bounded SQL execution.
- Fixed protection/CAPTCHA forms and editor search; SPA page views and sampled pointer hotspots.
- Grouped subdomain DNS zones and safe multi-string DKIM/DMARC TXT rendering.
- Configurable application ports, account-private service networks and larger resource budgets.
- Live CPU/RAM limit updates for enforced volumes, disk reservation admission and growth headroom.
- Opt-in public-page cache with cookie/auth/private-response exclusions.
- Sampled abuse/database-login signals and mailbox-owned email alerts alongside Telegram.
- Prepare updater dependencies before downtime; preserve configured Postfix listeners during updates.

Host-only Podman, mail-delivery and update downtime validation remains required before a stable production release.


## 0.4.1

- Disable every SSH forwarding type, including Unix-socket forwarding, for existing CGPanel SFTP-only profiles during upgrades.
- Validate SSH configuration before reload and restore it on failure. Existing SFTP credentials and workspace mounts are preserved.
- Add reproducible cross-server SFTP transfer and confinement checks.

## v0.4.0

- Replaced the basic file form with folder browsing, chunked transfers, multi-selection, Trash, permissions and revision-checked editing.
- Added an unprivileged Rust file helper with descriptor-based path confinement; files work when apps are stopped.
- Added optional per-workspace code-server with Git, private authentication and a separate HTTPS origin.
- Added approved language version tracks, PHP MySQL/FPM profiles, and runtime startup rollback.
- Added administrator-controlled stable GitHub release updates, verified binary bundles and recovery snapshots.
- Documented new APIs and deployment/update boundaries.


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
