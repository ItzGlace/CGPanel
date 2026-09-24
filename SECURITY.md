# Security policy

CGPanel 0.1 is development software and has not been independently audited. Use trusted development tenants on a dedicated host.

Do not publish exploit details, credentials, or tenant data in public issues. Use the repository owner's private security advisory facility to report a vulnerability; if that is unavailable, request a private contact without including exploit details. The project's maintainers must establish a supported release and disclosure process before production use.

## Trust boundaries

- The browser is untrusted. Every resource read and mutation checks tenant ownership. Administrators can manage all tenants.
- Nginx replaces `X-CGPanel-Client-IP`. The application accepts that header only from loopback. Keep the backend loopback-only and do not provide tenants with host shell access.
- The web service runs as `cgpanel` with no capabilities and a read-only host filesystem except its state directory.
- The root broker checks Unix peer credentials, receives typed operations, and validates names, domains, DNS data, and resource ownership. Its socket is accessible only to root and the panel account.
- Commands use argument arrays. Tenant-supplied commands are interpreted only inside the tenant's container as an unprivileged UID. Tenant input is never interpolated into a root shell command.
- The root broker and panel account are trusted. Compromise of either is a serious incident. Container and kernel vulnerabilities remain relevant.
- Database secrets live in the root-only broker registry. Container environment secrets are readable to the container owner. They are not a multi-party secrets vault.

## Operational requirements

Keep the kernel, Podman, language images, Nginx, database servers, Rust dependencies, and the panel patched. Establish independent backups, capacity alerts, and a recovery procedure. Pin runtime images to reviewed digests before exposing untrusted workloads. Provision provider-level DDoS protection and redundant DNS for public hosting.

The initial firewall assumes a dedicated server, SSH on port 22, and hosting on ports 80/443/2083. Database filters apply to new network connections; revocation does not forcibly terminate existing SQL sessions. Suspending a panel account does not terminate its applications.

Do not treat rate limiting as comprehensive DDoS prevention. There is no WAF, MFA, disk quota enforcement, malicious-package detection, or immutable remote audit store in this alpha.
