# Roadmap to a production hosting panel

This list records missing work, not available features.

1. Independent threat modeling, broker review, penetration testing, security update policy, and signed releases.
2. Crash-safe provisioning journal, idempotent reconciliation, resource editing, health supervision, failure rollback, and a tested uninstall/upgrade path.
3. Project and tenant disk/inode quotas, aggregate resource limits, storage alerts, image digest pinning, image management, and image scanning.
4. MFA/WebAuthn, scoped API tokens, session inventory, password recovery, and delegated administrator roles.
5. Container runtime settings, rolling deployments, Git deploy keys/webhooks, environment secret rotation, interactive PTY sessions, and long-running package jobs.
6. Database dumps/restoration, SQL GUI integration, user/grant editing, certificate distribution, local application database proxying, encrypted off-server backups, retention, and scheduled backups.
7. WAF integration, security event dashboards, provider/CDN integrations, and operator-tested incident workflows.
8. Mail (SMTP/IMAP, spam filtering, DKIM/SPF/DMARC), SFTP accounts, registrar integrations, DNSSEC, secondary DNS, statistics, redirect management, and file upload/download archives.
9. cPanel/WHM migration, reseller plans, account exports, billing hooks, WordPress/application installers, and wider distribution support.

CGPanel should not claim cPanel feature parity until these workflows are implemented and verified.
