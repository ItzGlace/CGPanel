# CGPanel 0.6 verification record

This is a release candidate, not a claim that every hosting-server workflow has been validated. Work was performed against the existing checkout; repository logic and tests were updated as a coding task.

## Executed

- Node 24.22.1, Rust 1.100.0; frozen npm and Cargo dependency installation.
- Rust formatting and Clippy with warnings denied.
- 17 Rust tests: 12 library tests, one panel test, four broker tests (DNS grouping and disk admission).
- Existing administrator API, analytics, MFA, website protection, file-helper, SFTP-policy and firewall-migration tests.
- New MCP credential scope/IP/revocation/hash tests; ordinary-tenant access boundaries; OAuth S256 PKCE, exact redirects and authorization-code replay checks; pointer collection and persistent login challenge/limits.
- New monitoring tests exercise actual monitor/job queues with a fixture broker: Telegram + email targeting and one-hour abuse-alert cooldown. They do not send messages to external recipients.
- Playwright with headless Chromium: 10 views at 1440px and 390px widths (20 screenshots), analytics controls, domain hierarchy and editor find scrolling/wrapping/Ctrl+F. No browser exceptions. Broker-dependent screens use an isolated broker fixture.
- Disposable Ubuntu 24.04 container: real MariaDB/PostgreSQL DDL and CRUD, rejection of multiple statements, other-database denial and host-file denial.
- BIND `named-checkzone` accepts the broker-generated parent/subdomain, MX, SPF, multi-string DKIM and DMARC fixture. A separate syntax fixture includes child ACME challenges.
- Real Nginx public-page cache hits; cookie/Authorization bypasses; private/Set-Cookie responses excluded. Tests read the shipped cache directives.
- Generated OpenAPI reference: 78 documented management operations. Python compilation and shell syntax checks.

## Request-by-request scope

| Area | Implementation and validation | Outstanding |
| --- | --- | --- |
| MCP for owner/user clients | HTTP transport, separate hashed credentials, read/manage scope, OAuth PKCE, docs; isolated auth tests | Connecting actual Claude/ChatGPT accounts; public HTTPS origin must be configured |
| Panel login CAPTCHA/brute force | One-use local arithmetic, persistent IP/account limits, admin controls, recovery override | Local arithmetic is basic; provider CAPTCHA remains available for hosted websites |
| Editor find | Keyboard controls, visible selection/scroll, wrap feedback; real browser test | None observed in exercised flows |
| All-page hotspots/analytics | Shared-layout snippet, SPA paths, click labels, sampled movement and heatmap selector; collection/browser tests | Site owner must install snippet and manage consent; production site not accessed |
| Protection/CAPTCHA layout | Shared form controls fixed; desktop/mobile screenshots inspected | Fixtures do not prove production deployment has rebuilt CSS |
| Mail hosting/DNS | Preserve configured Postfix listener on upgrade; DKIM/DMARC TXT and grouped-zone fixes; BIND validation | Live SMTP/IMAP authentication, DKIM signing, external delivery, PTR and provider port checks |
| Database interface | Database Studio tables/schema/SQL/CSV; real MariaDB/PostgreSQL privilege tests | Full phpMyAdmin parity, full SQL export/import and advanced administration are not implemented |
| Subdomains | Same-owner ancestor grouping, parent DNS zone and child record/ACME labels, account boundary checks | Migration on a real BIND server and existing customer zones |
| Application resource changes | Merge partial limits, live CPU/RAM update, preserve stopped state, early shrink rejection | Actual rootless Podman/cgroup and volume-growth test on dedicated host |
| Ports 80/443 | Configurable container HTTP port, PORT-aware starters and PHP launcher | Actual rootless low-port binding/runtime tests; upstream TLS is not automatic |
| Private workloads | Account bridge, no host publication for private mode, peer names/private addresses | Cross-tenant network tests on real Podman; existing apps move networks when recreated |
| Disk overselling/pressure | Account promise admission, preallocated volumes, growth headroom, server pressure status; 30 GiB/50 GiB regression | Database/mail/image/backup/log growth is not covered by app workspace quotas; test actual loop volumes |
| Updates/keepalive | Existing manual/automatic updater retained; dependencies before interruption, app restart policy, measured interruption record | Foreground broker operations, full rollback and under-load downtime not validated; under-one-minute target is not guaranteed |
| Multicore/performance | Account CPU quotas above one core; application performance controls and worker guidance; safe public-response cache | Framework worker/thread tuning remains application-specific |
| Security/email monitoring | Five-minute sampled traffic and named-role login-failure signals; mailbox ownership on sending; both queues/cooldown tested | Journal coverage, actual attack traffic and external alert delivery; not proof of DDoS/data access |

## External blockers

SSH to the user-supplied test server on port 22 returned `Connection refused`, including a retry outside the command sandbox. No remote installer or host-changing test was run. The repository's HTTPS Git proxy supports reads; GitHub API requests were denied by the cloud proxy (`CONNECT ... 403`). Additional destinations were saved in the environment configuration draft but the running allowlist remained unchanged.

No stable GitHub release, deployment to the user's existing installation, actual model-client session, live mail-delivery success, complete phpMyAdmin parity or sub-minute update guarantee is established by this record.

## Repeat checks

Use the CI workflow for pinned installation, Rust/Python suites and headless browser dependencies. The disposable image is built from `tests/host-tools.Dockerfile`; `tests/host_tools.py` must only run inside a fresh test container with `CGPANEL_DISPOSABLE_TEST=1`. It initializes local SQL fixtures and must never run on an existing hosting server.

See [the 0.6 guide](V0.6-GUIDE.md) for client credentials, HTTPS/OAuth setup, limits and operational instructions.
