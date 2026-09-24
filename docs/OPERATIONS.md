# Administrator operations

## Install and upgrade

Use a dedicated Ubuntu 24.04 host and follow the [repository installation instructions](https://github.com/ItzGlace/CGPanel#install-on-a-fresh-ubuntu-2404-server). Build CSS before Rust so the embedded interface matches the source. For an existing v0.2 installation, pull the release, rebuild the frontend and Rust binaries, and restart the panel services after taking a recovery snapshot. `deploy/upgrade-v0.2.sh` also works for the additive v0.3 schema; its filename reflects when the upgrade path was introduced.

```bash
npm ci --ignore-scripts
npm run build:css
cargo build --release --locked
sudo bash deploy/upgrade-v0.2.sh
```

Ensure Cargo and the chosen Node.js runtime are available on the PATH used by the upgrade command. The script snapshots the SQLite database, broker registry, configuration and previous binaries under `/var/backups/cgpanel-upgrade-*`. It installs required feature tools and adds schema tables without replacing existing database grants or the host firewall.

## Accounts and domains

Create tenants in **User manager**. The resource quota is a count per resource type, not disk/inode storage enforcement. Assign domains explicitly to their owners, then attach only applications belonging to the same owner. Tenants may create subdomains under their assignments. Suspending an account revokes panel sessions; it does not stop running applications. Stop those separately when required.

Panel login rules accept CIDRs. Database remote access accepts exact IP addresses. Keep host SSH access limited to server administrators; panel terminals cannot grant tenant root access.

## Service checks

```bash
systemctl status cgpanel cgpanel-agent nginx bind9
systemctl status cgpanel-certbot.timer
journalctl -u cgpanel -u cgpanel-agent --since '1 hour ago'
systemctl list-timers cgpanel-certbot.timer
```

`GET /healthz` reports the HTTP process is serving; it is not a complete dependency check. **Overview** and `GET /api/admin/system` report resource counts and broker host health. An `agent: unavailable` result means the broker check failed even if HTTP health is OK.

The unprivileged HTTP service and privileged Rust broker communicate through `/run/cgpanel/agent.sock`. Application commands run in rootless Podman containers. Host operations are fixed broker operations; there is no general root shell API.

## Administrator API access

Open **Admin API → Create token** in a browser session. Choose read-only for reporting or full administrator access for provisioning. Set a 1–90 day lifetime and, when possible, the script's source CIDRs. Save the one-time secret; CGPanel stores only its SHA-256 hash and a display prefix. At most 20 active tokens are allowed per administrator.

Revoke unused tokens from the same page. Changing the administrator password revokes all that account's tokens and sessions. Account enablement and panel IP rules are rechecked on every bearer request. Token management requires a browser/login session with CSRF protection; tokens cannot create more tokens or change the administrator password. Full-admin tokens can provision, execute tenant commands, restore and delete resources. Read-only tokens still expose sensitive readable data and backup downloads; they are not public monitoring keys.

The [API guide](https://github.com/ItzGlace/CGPanel/blob/main/docs/API.md) includes curl and Python examples. The bundled OpenAPI file can be imported into API tools using the real panel HTTPS origin as the server URL.

## Backups and restore boundaries

Keep off-server backups of both `/var/lib/cgpanel/panel.db` and `/var/lib/cgpanel-agent/registry.json`, plus `/etc/cgpanel`, tenant workspaces, database data/dumps and required service configuration. Use SQLite's backup API instead of copying a live WAL database file alone. A per-website `.cgp` does not capture the complete host, panel accounts or certificate private keys.

Before rollback, stop both panel services and preserve post-upgrade tenant changes. Restore matching SQLite, registry and binaries from one recovery snapshot. Restoring only a binary does not undo later provisioning changes. Validate database and domain ownership before any manual recovery.

## TLS, DNS and CDN

Public HTTP verification needs DNS pointing to the server and incoming port 80. Local DNS verification needs registrar delegation and public UDP/TCP 53. Cloudflare verification uses a scoped DNS token. The administrator can request a short-lived panel-IP certificate; IP validation uses HTTP, not DNS. The renewal timer runs every six hours. Review certificate expiry and renewal results in **Certificates & CDN** and `/var/log/letsencrypt/letsencrypt.log`.

A zone export is a point-in-time BIND file for provider import. Registrar changes and CDN activation happen at the provider; later DNS edits do not automatically synchronize. Cloudflare visitor headers are trusted only from the configured Cloudflare networks. Review those ranges as the provider updates them.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| API 401 | Token spelling, expiry or revocation; account enablement; eight-hour session expiry. |
| API 403 | Account/token IP filters, role, read-only token attempting POST, or missing session CSRF header. |
| Job queued for a long time | Broker/panel service health, pending jobs and whether its owner is enabled. |
| Job interrupted | The panel restarted while it ran. Inspect the target before retrying; restore/TLS/egress need a fresh reviewed request. |
| Domain returns 502 | Application health, startup logs and binding to `0.0.0.0:8080`. |
| Certificate validation fails | Public DNS, firewall, challenge path or delegated authoritative DNS, provider token scope. |
| SOCKS app cannot resolve DNS | Proxy availability and permission for TCP to `1.1.1.1:53`. Other UDP and IPv6 are intentionally blocked. |
| Backup delivery fails | Destination credentials, SSH host key, remote permissions, proxy connectivity and local disk space. The local archive remains available. |

Monitor disk capacity: disk quotas are not implemented. Nginx/firewall controls cannot absorb upstream bandwidth saturation. Review [security boundaries](https://github.com/ItzGlace/CGPanel/blob/main/SECURITY.md) before production use.
