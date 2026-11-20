This patch fixes fresh-install version metadata: the installed version is read from Cargo.toml rather than hard-coded to 0.5.2.

CGPanel 0.6.1 adds account-scoped MCP/OAuth connections, persistent login protection, Database Studio, improved protection/CAPTCHA forms, editor find controls, SPA analytics and sampled mouse hotspots, grouped subdomain DNS, configurable application ports/private services, disk admission safeguards, public-page caching and security/email alert queues.

CGPanel remains a development alpha. The release workflow requires the automated checks to pass before publishing Ubuntu 24.04 x86_64 binaries. Local validation includes Rust/API/security tests, desktop/mobile browser checks, real MariaDB/PostgreSQL privilege tests, BIND zone validation and Nginx cache privacy tests. See docs/VERIFICATION-v0.6.md for the scope and limitations. Dedicated-host provisioning, external mail delivery, actual model-client sessions, full update rollback and sub-minute interruption are not certified by these checks. Database Studio is a simplified SQL workspace, not complete phpMyAdmin parity.

Assets:
- `CGPanel-v0.6.1-linux-x86_64.tar.gz`: optimized binaries and deployment helpers for an existing dedicated Ubuntu 24.04 installation.
- `SHA256SUMS`: verify the downloaded bundle before extracting or executing it.

Existing installation: use **Panel updates → Check for updates → Install available update**, or run the installed `/usr/local/lib/cgpanel/updater.py --apply` as root. The updater verifies the checksum and makes a recovery snapshot. Back up both panel and broker registries off-server first.

Fresh installation: use the source at the `v0.6.1` tag and follow `README.md`'s dedicated-host installation instructions. The binary update bundle does not bootstrap a fresh hosting server. Do not run the dedicated-host installer over an existing installation.

Read `docs/V0.6-GUIDE.md` for HTTPS/OAuth client setup, credential scopes, private services, caching, resource limits and monitoring/email requirements. Private services require recreated workloads to use the account bridge; existing containers retain their current networks and restart policies until recreated. Database/mail/image/backup/log growth is outside workspace quotas. Local arithmetic challenges are basic bot checks, not advanced CAPTCHA protection.
