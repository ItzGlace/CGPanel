# Mail hosting

CGPanel manages real SMTP and IMAP mailboxes using Postfix, Dovecot, and Rspamd. Each VPS has its own mail hostname, TLS certificate, signing keys, and mailboxes. Mailbox administration is available under **Mail hosting**; read and send messages with an IMAP/SMTP client. This page is not a webmail inbox.

## Administrator setup

1. Choose a unique hostname for each VPS, such as `mail.example.com`. Add a **DNS-only** A record pointing to that VPS. Add AAAA only if IPv6 routing and reverse DNS are configured.
2. Ask the VPS provider to set the IP's PTR record to the same hostname and confirm incoming and outgoing TCP port 25 are permitted. Forward and reverse DNS must agree. These provider settings cannot be changed by CGPanel.
3. Add the hostname as an assigned domain, then issue its trusted certificate under **Certificates & CDN**. Select that domain in **Mail hosting → Server configuration**. The development self-signed option is for local testing; normal clients will not trust it.
4. Save the hostname and enable public mail ports. Each server needs this setup separately. Use different hostnames when hosting the same parent domain on two servers; do not publish both as MX unless mailbox routing/replication has been arranged separately.
5. Enable email for each sending/receiving domain, then create its mailboxes. Copy the generated MX, DKIM, SPF, and DMARC records into the authoritative DNS provider. Cloudflare's website proxy does not proxy these SMTP/IMAP ports.
6. Verify inbound and outbound delivery using accounts you control. Check headers for SPF, DKIM and DMARC alignment before tightening DMARC policy. A configured service alone does not establish deliverability or IP reputation.

## Mailbox access

| Setting | Value |
| --- | --- |
| Username | Complete email address |
| Password | Separate mailbox password |
| IMAP | Server mail hostname, port 993, TLS |
| SMTP | Same hostname, port 587 with STARTTLS, or 465 with TLS |
| Authentication | Required for message submission |

Mailbox passwords require 14–128 characters and are stored as salted SHA-512 crypt hashes. The panel never returns existing passwords. Manage a mailbox to reset its password, change quota, or disable it. Disabling preserves stored mail. A domain with active mailboxes cannot be removed.

Quotas are enforced by Dovecot and are separate from application workspace disk allocations. Each account can create up to 30 mailboxes. Administrators must budget physical disk space for mail and databases in addition to application volumes.

## Delivery security and operations

Postfix rejects unauthenticated external relaying. Submission requires encrypted authentication and checks that the authenticated mailbox owns the envelope sender. Rspamd filters mail and signs authenticated outbound mail with a per-domain DKIM key. Message size is limited to 32 MiB. No public IMAP port 143 or plaintext FTP-style authentication is offered.

Use `journalctl -u postfix -u dovecot -u rspamd` and the distribution mail logs for diagnostics. `postqueue -p` shows queued delivery; investigate the reported destination error before retrying. Never publish logs containing private message content, mailbox passwords, or signing keys.

Mail resides under `/var/lib/cgpanel-mail`. Configuration is in `/etc/postfix`, `/etc/dovecot`, and `/etc/rspamd`; private DKIM keys are in `/var/lib/rspamd/dkim`. Application `.cgp` archives currently cover application workspaces and selected databases, not mailbox messages. Back up mail data and signing keys separately with restricted root access.

## Administrator API examples

Use an administrator bearer token as documented in [API.md](API.md). Examples omit secrets. Session-authenticated changes also require the CSRF header.

- `GET /api/v5/mail`: server settings, authorized domains, mailbox metadata, and public DKIM records. Administrators see all managed domains; tenants see their own.
- `POST /api/v5/mail/server`: administrator-only server configuration, for example `{"hostname":"mail.example.com","certificate_domain":"DOMAIN_ID","enabled":true}`.
- `POST /api/v5/mail/domains/DOMAIN_ID`: `{"enabled":true}` enables email for an owned domain and generates its signing key.
- `POST /api/v5/mail/mailboxes`: `{"domain_id":"DOMAIN_ID","local":"hello","password":"REPLACE_WITH_PRIVATE_PASSWORD","quota_mb":512,"enabled":true}` creates a mailbox. To update, include its `id`; an empty password keeps the existing hash. Only an authorized domain owner or administrator may change it.

Public DNS, PTR, certificate trust, provider port restrictions, and actual external delivery require verification for each VPS. Local delivery tests do not prove public delivery.
