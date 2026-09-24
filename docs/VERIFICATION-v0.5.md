# v0.5 verification record

Development verification, 2026-09-25. This records completed checks, not an independent security audit or proof of public mail deliverability.

| Area | Evidence |
| --- | --- |
| Rust and UI | Rust unit tests and Clippy with warnings denied; all JavaScript entry modules pass syntax checks |
| Authentication/API | Isolated administrator scopes, IP restrictions, expiry/revocation, CSRF, audit pagination, tenant denial, bundled documentation and 68-operation OpenAPI contract |
| MFA | Enrollment, required second factor, TOTP replay rejection, single-use recovery, password-protected disable |
| Analytics | Text/plain beacons, distinct-IP estimates, click cells, origin/key rejection, Do Not Track, query/IP privacy and CIDR range previews |
| Protection | Nginx serves the default page; known AI user agent rejected; robots/sitemap served; OWASP CRS rejects an XSS test request; traffic summary records rejections |
| CAPTCHA | Local challenge origin/domain/IP checks, single use, secure cookie, replay rejection; automatic sitemap includes observed public paths |
| Resource limits | Filesystem ENOSPC, migration preserving files, growth, shrink rejection, CPU/RAM changes and over-budget rejection |
| Transfer services | External SFTP binary round trip, rename, permissions, confinement and shell/forwarding denial; certificate-pinned encrypted FTPS round trip |
| Recovery | MySQL and PostgreSQL restore, mixed SFTP/tenant file ownership, pre-restore recovery archive, resumed writers, chunked import and offset rejection |
| Restore confinement | SQL shell metacommand and archive symlink tests could not alter a host canary; failed staging cleanup and subsequent recovery restore succeeded |
| Console | Incremental stdout, separate stderr, nonzero exit status and enforced execution timeout; browser Tab completion and clear verified |
| File editor | Separate browser tab, successful optional ten-second autosave, manual save; helper tests cover revision conflicts, upload integrity, Trash and traversal/symlink/hardlink denial |
| Mail | Broker mailbox provisioning and domain-deletion guard; TLS IMAP, authenticated SMTP, local delivery, DKIM header, sender/relay rejection, and actual mailbox quota rejection |
| Design | Browser sign-in, overview, first-login guide, mail page and file browser inspected; distinct pinned CC0 sidebar assets and generated logo documented |

The final deployment patch is v0.5.1. Lunarblush passed website isolation headers, Unity MIME/encoding, PHP-FPM with a real MySQL TLS query, existing password SFTP upload/confinement, and application privilege checks. An older nftables set migration was corrected and regression-tested. The domain IDE login was verified through Cloudflare after fixing default TLS handshake handling. Lunarblush SMTP/IMAP has a trusted mail certificate; public mail DNS/PTR and external delivery remain separate acceptance steps. Third-party CAPTCHA credentials have not been supplied; provider integrations are implemented but live provider verification is not asserted. Mailbox messages are outside application `.cgp` backups. Existing domain/DNS assignments are preserved on restore.
