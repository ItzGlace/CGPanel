# Development verification

Verified on 24 September 2026 on a dedicated Ubuntu 24.04 development host.

- Tailwind CSS production build and JavaScript syntax checks passed. All fonts, SVG icons, and generated brand artwork are served locally by the Rust binary.
- Rust formatting, Clippy with warnings denied, and five unit tests passed.
- Installer shell syntax passed; the panel, privileged broker, Nginx, BIND, MariaDB, PostgreSQL, and Fail2ban services were running.
- Actual starter applications for Python, PHP, Node.js, Java, Rust, and static sites returned HTTP 200. Each command console reported an unprivileged container UID of 1000.
- Live integration tests passed for authentication, CSRF rejection, cross-tenant access denial, domain assignment boundaries, DNS records, file path confinement, workspace backup/restore, schedule creation, MySQL/PostgreSQL grants, exact-IP filtering, and forged proxy-header rejection.
- Browser checks covered the dashboard, navigation, application creation form, generated artwork, and desktop/mobile layouts.

The runtime test used the public ECR mirror of Docker Official Images because Docker Hub downloads were unavailable from the host. Runtime images remain mutable upstream tags.

This is functional verification of a development alpha, not an independent security audit, load test, or proof of complete cPanel compatibility. Public DNS delegation, issuance of certificates for a real domain, delivery of a real Telegram bot, upstream DDoS resistance, disaster recovery, and hostile multi-tenant operation have not been verified. The initial panel certificate is self-signed. See SECURITY.md and ROADMAP.md for current limits.

Live tests create disposable resources and suspend their test accounts afterward. They retain application workspace files for inspection.
