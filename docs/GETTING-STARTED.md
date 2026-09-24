# Getting started with CGPanel

CGPanel is a free Rust hosting panel for websites, APIs and background workers such as Telegram bots. This community alpha runs on a dedicated Ubuntu 24.04 server. It is not complete cPanel parity.

## Your first website

1. Sign in with the account provided by the server administrator.
2. Open **Applications → Create**. Choose Python, PHP, Node.js, Java, Rust or static, then choose Web/API mode. The starter application listens on port 8080 inside its container. Custom web commands must bind to `0.0.0.0:8080`.
3. Use **File manager** to edit workspace text files, or **Terminal** to install user-space packages and run commands. Commands execute as UID 1000, with a 25-second limit. This is a bounded command console, not an interactive SSH shell.
4. Ask the administrator to assign your domain to the application. You can create subdomains beneath an assigned domain. Point DNS to this server, or arrange nameserver delegation for CGPanel's authoritative DNS.
5. Open **Certificates & CDN** and request a certificate after DNS and validation are ready. HTTP verification needs public port 80; DNS verification needs the selected authoritative DNS provider.
6. Visit your domain and verify the application before enabling monitoring or automated backups.

The application workspace is `/workspace`. Runtime packages installed there persist; arbitrary system package installation and root access are not available to tenants. Application deletion retains workspace files for administrator recovery, but removes the managed workload. Do not use deletion as a backup strategy.

## Databases

Create a MariaDB (MySQL-compatible) or PostgreSQL database in **Databases**. Save the credentials returned at creation; the normal list does not reveal passwords. Each database has its own account and grants. Add exact source IP addresses for external access; `0.0.0.0/0` is not accepted.

Inside a container, `127.0.0.1` belongs to that container. Use the host's reachable database address and the explicitly allowed source IP, or a trusted database proxy. Use TLS for remote database connections. A SOCKS-routed app must reach the database through its configured route.

## Telegram bots and APIs

Choose Worker mode for polling bots and long-running background programs. Supply tokens as application environment variables. Choose Web/API mode for HTTP APIs or Telegram webhooks, bind to `0.0.0.0:8080`, then attach a domain and HTTPS. Worker mode does not publish an HTTP port.

## Routine tasks

| Page | What to do |
| --- | --- |
| Scheduled tasks | Add a five-field cron expression, IANA timezone and container command. Check run history for errors. |
| Website monitoring | Enable HTTP/HTTPS checks and optionally select a Telegram alert integration. |
| Website analytics | Enable tracking and copy the supplied script into your website. Add stable click labels where useful. |
| Integrations | Save Telegram, S3, SSH, Cloudflare or SOCKS5 settings. Secrets are omitted from normal reads. |
| Full backups | Select the application and databases, create a `.cgp`, then download it or send it to your storage. |
| Background jobs | Inspect pending, succeeded, failed or interrupted operations. A queued job has not finished yet. |
| Outgoing traffic | Apply SOCKS5 routing to an application. An administrator can lock this setting. |

## Understanding the numbers

Unique IPs are daily estimates, not distinct people. Shared IPs combine visitors; changing IPs can count one person twice. Heatmaps aggregate normalized click positions and do not replay sessions. SEO inspection reports fetched HTML observations and does not promise search rankings. Monitoring runs from this server, so it cannot alert when the entire monitoring host is offline.

## Backups and recovery

A `.cgp` is a ZIP archive with website files, explicitly selected logical SQL dumps and application/domain/DNS metadata. It can contain credentials and is not encrypted at rest. Store copies only in destinations you control. The restore UI overlays files and SQL into existing resources; an administrator must recover archived routing, domain and environment settings separately. Files added since a backup are not removed by overlay restore.

Read the [monitoring, backup and HTTPS guide](https://github.com/ItzGlace/CGPanel/blob/main/docs/V0.2-GUIDE.md) for setup details and limits.

## Where to find help

- Administrators: use **Admin API** for tokens, endpoint search, sample requests and OpenAPI download.
- Use **Documentation** in the panel to read the bundled guides without an external documentation service.
- Read the [documentation index](https://github.com/ItzGlace/CGPanel/blob/main/docs/README.md) on GitHub for installation, operations and API examples.
- Report reproducible bugs through [GitHub Issues](https://github.com/ItzGlace/CGPanel/issues). Remove credentials, cookies, tokens and private website data before sharing logs.
