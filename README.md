# CGPanel

**A free, open source hosting control panel built in Rust.**

CGPanel brings websites, APIs, Telegram bots, databases, and hosting tools into a familiar category-based dashboard. It is an independent project, with an original interface inspired by traditional hosting panels. It is not affiliated with cPanel.

> **Version 0.2 is a development alpha, not a complete cPanel replacement.** Use a dedicated development server. Do not entrust production tenants or irreplaceable data to this release without further security review, operational testing, and off-server backups.

## Working features

| Area | Included |
| --- | --- |
| Accounts | Administrator-created tenants, resource counts, account suspension, password changes, CIDR-based panel login rules |
| Applications | Rootless Podman workloads for Python, PHP, Node.js, Java, Rust, and static sites; web/API and background-worker modes |
| Telegram bots | Run polling bots as workers, or attach a domain and TLS to webhook applications; provide tokens through container environment variables |
| Domains | Administrator domain assignment, tenant subdomains, Nginx application routing, Let's Encrypt certificates |
| DNS | Local authoritative BIND zones; A, AAAA, CNAME, MX, TXT, and NS records |
| Databases | MySQL-compatible MariaDB and PostgreSQL; per-database users, database-specific grants, local access and exact-IP remote access with TLS |
| Files | Container-confined file listing, text reading, and editing |
| Console | Unprivileged commands inside application containers; user-space package installation |
| Schedules | Five-field cron, IANA timezones, next runs and execution history |
| Backups | Full `.cgp` ZIP archives with files, SQL, domain/DNS configuration; file/database restore; scheduled Telegram, S3 and SSH delivery |
| Monitoring | HTTP availability, response times, Telegram outage/recovery alerts |
| Analytics | Optional daily unique-IP estimates, page views, referrers, click heatmaps, technical SEO checks |
| HTTPS / CDN | HTTP or DNS verification, automatic renewal, short-lived panel IP certificates, Cloudflare zone export |
| SOCKS5 | Per-integration proxy routing and enforced application TCP/DNS gateways, with administrator locks |
| Security | Argon2id, HttpOnly/SameSite sessions, CSRF checks, login throttling, ownership checks, audit records, Nginx rate/connection limits, nftables IP blocking, Fail2ban for SSH |

## Architecture

```text
Browser ── HTTPS :2083 ── Nginx ── loopback :2082 ── cgpanel (unprivileged Rust)
                                                      │
                                              permissioned Unix socket
                                                      │
                                               cgpanel-agent (Rust)
                                                /       |       \
                                 rootless Podman      SQL       Nginx / BIND / nftables
                                 per Linux tenant
```

The HTTP service stores account, session, resource, and audit data in SQLite. A separate root-owned registry records the broker's resources. The broker accepts a fixed set of operations from the panel service account, validates identifiers, and checks resource ownership again. It never exposes a general host shell endpoint.

Application shells run as UID 1000 inside rootless containers. Capabilities are dropped, privilege escalation is disabled, the root filesystem is read-only, and the workspace is writable. Each application is limited to 512 MiB RAM, one CPU, and 128 processes. Linux tenant accounts have no login shell and no sudo permission. The panel and agent are trusted infrastructure; container isolation is not a substitute for kernel patching or an independent security audit.

## Install on a fresh Ubuntu 24.04 server

The installer configures host services and firewall rules. **Use a dedicated host.** It is not designed to coexist with another hosting panel. It stores copies of several original configuration files with a `.pre-cgpanel` suffix, but it is not a transactional installer or an automatic uninstaller.

1. Install the current stable Rust toolchain using the official instructions at [rustup.rs](https://rustup.rs).
2. Clone this repository on the server.
3. Run as root from the repository directory:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
export CGPANEL_PUBLIC_IP="YOUR_SERVER_IPV4"
bash deploy/install.sh --dedicated-host
```

Open `https://YOUR_SERVER_IPV4:2083`. The initial administrator is `admin`; generated credentials are saved to `/root/cgpanel-initial-admin.txt` with root-only permissions. You may supply `CGPANEL_ADMIN_PASSWORD` through a secure environment instead. The initial certificate is self-signed. Replace the panel certificate with a trusted certificate before ordinary use. Domain certificates requested from the panel are separate from the panel's own certificate.

The installer starts Nginx, MariaDB, PostgreSQL, authoritative BIND, nftables, Fail2ban, and both Rust services. It preserves SSH on port 22, opens 80/443/2083 and DNS 53, and permits remote database ports only for configured client IPs. Customize SSH rules before installation if your host uses a nonstandard port. Existing unrelated firewall rules are not supported by this dedicated-host installer.

Images default to Docker Hub. If that registry is unavailable, set `CGPANEL_IMAGE_REGISTRY=public.ecr.aws/docker` before installation, or in `/etc/cgpanel/agent.env` and restart the agent. This uses the public ECR Docker Official Images catalog. These are the two supported registries; tenants cannot choose arbitrary images.

```bash
systemctl status cgpanel cgpanel-agent
journalctl -u cgpanel -u cgpanel-agent -f
```

## Upgrade and v0.2 features

See [the v0.2 guide](docs/V0.2-GUIDE.md) for upgrade instructions, setup, archive format, privacy behavior, certificate renewal, and proxy protocol limits. The [release jobs and charts](docs/RELEASE-0.2.md) record the implementation and verification scope.

## First application

1. Create a tenant in **User manager**.
2. Create an application for that account. The default start command produces a working sample website. Your own HTTP server must bind `0.0.0.0:8080`.
3. Edit files with **File manager** or install application packages through **Terminal**. Python packages can use `pip install --user`; npm and Cargo can write into `/workspace`. For Java, use the JDK and a project-local Maven or Gradle wrapper. System package installation is an administrator/image-maintenance task.
4. Assign a domain to the same account and select the application. Tenants may create subdomains only beneath assigned domains.
5. Point DNS at the host, then request HTTPS. Domain certificate requests require accepting the Let's Encrypt subscriber agreement.

For a Telegram polling bot, select **Background worker** and supply a start command and `BOT_TOKEN` environment variable. For webhooks, use a web application, a routed domain, and a trusted certificate. CGPanel does not create Telegram accounts or register webhooks automatically.

## Database connectivity

Each database has a generated name, a matching isolated username, and a random password shown once on creation. Secrets are excluded from panel resource responses. The root-owned broker registry retains database credentials for grant management; protect and back up this file as a secret.

Local host applications can use the MariaDB Unix socket or PostgreSQL with TLS on loopback. A container's `127.0.0.1` is its own namespace. To connect from a container, use the host's reachable address and explicitly allow the source IP seen by the database, or arrange a trusted database connection proxy. Do not expose an unrestricted database listener to work around networking.

Remote clients require an exact source-IP allowlist. MySQL-compatible remote accounts require SSL; PostgreSQL remote grants use `hostssl`, SCRAM, and database-specific rules. Use certificate verification in production. Initial development certificates are self-signed. SSH tunneling remains useful when public database access is unnecessary.

## DNS

The zone editor updates this server's authoritative BIND service. Registrar delegation and glue records must be configured separately. The default zone contains an `ns1` record and an apex A record for the server IPv4 address. DNS recursion and zone transfers are disabled. Redundant authoritative DNS is not provided by a single host.

## Security and current limits

See [SECURITY.md](SECURITY.md) and [docs/ROADMAP.md](docs/ROADMAP.md). Specific limits:

- This is not complete cPanel/WHM feature parity. Mail hosting, FTP/SFTP account management, resellers, migration, WordPress tooling, database GUI/query tools, MFA, API tokens, and ModSecurity/Coraza WAF integration are future work.
- Nginx and firewall controls reduce some abusive traffic; they cannot prevent upstream bandwidth saturation. Arrange provider/CDN DDoS protection separately.
- Workspaces have resource-count quotas but **no disk or inode quotas**. Container images and backup archives also consume shared disk. Trusted development tenants only.
- The terminal is a bounded command console, not an interactive PTY. Commands have a 25-second limit and captured output is bounded.
- Full backups contain secrets and are not encrypted at rest. Each database dump has its own consistency boundary. Restore overlays files and SQL into existing owned resources; domain reassignment, portable archive import and automatic application configuration restoration require administrator recovery. See the v0.2 guide for exact scope.
- Suspending a tenant disables panel login and revokes sessions; it does not stop running workloads. Application deletion retains workspace files for administrator recovery.
- Audit presentation shows the latest 20 events. Audit records are stored locally and are not tamper-proof against a host administrator.
- Resource mutation spans host services and two registries. A crash or partial provisioning failure may require administrator reconciliation. Back up both registries together.
- Runtime image tags are upstream-maintained tags. Pin reviewed digests and establish an update process before production deployment.

## Development and tests

The interface uses **Tailwind CSS only for design**, with utility classes in the HTML and JavaScript templates. `web/input.css` contains Tailwind directives, theme tokens, and font registration imports; `web/style.css` is generated output. IBM Plex Sans, Lucide SVGs, and generated CGPanel brand artwork are self-hosted and embedded in the Rust executable. There is no runtime font, icon, or CSS CDN dependency. Asset origins and licenses are recorded in [licenses/ASSETS.md](licenses/ASSETS.md), and ImageGen prompts in [docs/BRAND-PROMPTS.md](docs/BRAND-PROMPTS.md).

Use Node.js 22 or newer to rebuild the UI before compiling Rust:

```bash
npm ci --ignore-scripts
npm run build:css
npm run check:ui
```

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
bash -n deploy/install.sh
```

For the interface and read-only API without provisioning, run `cgpanel` locally after bootstrapping an administrator. `CGPANEL_INSECURE_LOCAL=1` is for loopback HTTP development only. Host operations require the Linux broker and its configured services.

The live test creates actual tenants, containers, DNS records, databases, schedules, and a backup on a dedicated test host. It verifies cross-tenant denial, database ACLs, file confinement, non-root commands, CSRF, IP filtering, and backup restoration. It removes created resources and suspends its test accounts; application workspaces are retained.

```bash
export CGPANEL_TEST_PASSWORD='your-test-admin-password'
python3 tests/smoke.py
unset CGPANEL_TEST_PASSWORD
```

Never commit real credentials, registry files, `.env` files, certificates with private keys, or production SQLite databases.

## License

GNU Affero General Public License, version 3 or later (AGPL-3.0-or-later). See [LICENSE](LICENSE). Contributions are welcome under the same license.
