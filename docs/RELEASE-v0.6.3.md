Fixes recovery when an update is interrupted after stopping CGPanel, and preserves the reason for a startup health-check failure.

- SIGINT, SIGTERM and SIGHUP during installation trigger rollback. Before services stop, cancellation waits for the current package-preparation command to finish, then exits without stopping the panel.
- Binary copying and complete atomic binary backups happen while the existing panel is online. After services stop, activation uses renames instead of copying large executables during the interruption.
- Before staging and again before shutdown, check each filesystem for the required binary, snapshot and recovery writes plus 1 GiB headroom. Archive estimates include sparse-file logical sizes. A failed snapshot releases partial/staged files and restarts the unchanged panel; incomplete backups are never restored.
- Configuration commands and their children stop before files are restored. An installer lock prevents a second installer from changing the same installation, including when the old updater wrapper has exited.
- Recovery continues if its output pipe is closed. A failed installer clears its matching stale `installing` status, and retains root-only logs.
- Readiness logs distinguish an HTTP failure, a version mismatch, and an unavailable or empty agent socket. Loopback probes bypass external HTTP proxies. Recent panel/agent service journals are captured before rollback; agent startup logs identify its current configuration stage.
- Regression coverage includes actual local HTTP/Unix sockets, three cancellation signals, surviving child processes, real 0.5.2 upgrade/rollback, low space, failed snapshots, failed health checks after the new panel starts, and cancellation before/after services stop. Hosting-server service controls still use test adapters in these container fixtures.

For an existing Ubuntu 24.04 x86_64 installation, use **Panel updates**, or start the detached update service as root:

```bash
systemctl start --no-block cgpanel-update.service
```

Read `/var/lib/cgpanel-updates/last-install.log` for the underlying installer error. Do not run the fresh-host installer over an existing panel. Power loss and SIGKILL cannot execute an in-process rollback; host-level recovery and production downtime remain deployment-dependent.
