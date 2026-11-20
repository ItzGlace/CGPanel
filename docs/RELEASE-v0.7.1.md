Fix fresh installation stopping with HTTP 403 while fetching rclone, before the administrator account was created.

- APT packages now bundle the pinned rclone archive and validate its SHA-256 before installing it. This part of fresh setup works without network access.
- Source installations use the verified CGPanel mirror with official GitHub and rclone download fallbacks. Checksums remain pinned; a bad checksum never overwrites the installed tool.
- Tests cover forbidden/unavailable download sources, bad checksums, malformed archives, offline bundled installation and execution of the real binary.

For an existing configured panel, use the Updates page or `apt update && apt install --only-upgrade cgpanel`. For an interrupted fresh installation, upgrade the package, then run `cgpanel-setup --dedicated-host --public-ip YOUR_SERVER_IPV4`. No administrator account was created if setup stopped at the rclone download.
