Fixes the update from CGPanel 0.5.2 failing when another APT process holds the package-index lock.

- Dependency preparation waits for APT index and dpkg locks before stopping CGPanel. A lock race is retried within a 20-minute preparation budget; repository/signature failures are reported directly. Existing lock files and other package processes are preserved.
- Installer output is retained in `/var/lib/cgpanel-updates/last-install.log`; updater output is retained in `last-update.log`. Both are root-only. Updated installers expose the underlying failure in the Panel updates status instead of only a subprocess exit code.
- Rollback explicitly closes SQLite connections, restores files atomically, and preserves the previous version and tenant data. This also handles the old updater retaining a database connection while its installer runs.
- New tests exercise a real 0.5.2 panel and its SQLite data through success, blocked APT, preflight failure and forced post-install failure. A separate Nginx-proxied website is checked throughout. These container tests use adapters for systemd/nftables and do not certify every host deployment.

If a 0.5.2 update failed before creating snapshot contents, the panel binaries were not replaced. Let the existing package job finish, then install this update through **Panel updates**, or run the installed updater as root:

```bash
python3 /usr/local/lib/cgpanel/updater.py --apply
```

The first attempt is still launched by the old updater, whose status message may hide an error if preparation cannot complete. The new installer retains the real output in `last-install.log` even in that case. Back up before upgrading. Never run the fresh-host installer over an existing installation.

Assets target dedicated Ubuntu 24.04 x86_64 installations. Verify `SHA256SUMS`. CGPanel remains a development alpha; external mail delivery and production update downtime depend on your host and provider.
