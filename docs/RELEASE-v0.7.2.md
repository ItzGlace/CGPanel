Fix fresh setup stopping during Certbot installation when PyPI downloads time out or the host cannot resolve Cryptography dependencies.

- Ship Certbot 5.10.0, its Cloudflare DNS plugin and their dependencies (29 pinned packages in total) as a verified Ubuntu 24.04 amd64/Python 3.12 wheel bundle. Installation uses no package index, validates archive and individual wheel hashes, and checks native modules and the plugin before activation.
- Preserve the previous Certbot environment if installation fails. Repeated setup reuses an already verified environment instead of creating more copies.
- Cover failed activation, corrupt archives/wheels, forbidden download sources and offline installation in native Ubuntu package tests.

For the interrupted fresh installation, run `apt update && apt install --only-upgrade cgpanel`, then `cgpanel-setup --dedicated-host --public-ip YOUR_SERVER_IPV4`. The administrator credentials file is generated later during setup. Existing configured panels upgrade through APT or their configured release channel.
