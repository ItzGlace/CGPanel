# Install with the bootstrap script or APT

Supported system: a **dedicated Ubuntu 24.04 amd64 server**. Initial setup configures Nginx, DNS, database servers, mail, SSH policies and the host firewall. Use the server's IPv4 address below, including the public address when behind NAT.

```bash
curl -fsSL https://raw.githubusercontent.com/ItzGlace/CGPanel/main/install.sh -o /tmp/cgpanel-install.sh && sudo bash /tmp/cgpanel-install.sh --dedicated-host --public-ip YOUR_SERVER_IPV4
```

This downloads prebuilt binaries; Rust/Node build tools are not required on the host. The bootstrap verifies the archive key fingerprint and APT verifies repository signatures and package hashes. It never uses `trusted=yes` or disables signature checking. Initial administrator credentials are saved in `/root/cgpanel-initial-admin.txt` if generated. Configure a trusted panel certificate before general use.

To register the signed repository manually:

```bash
curl -fsSL https://raw.githubusercontent.com/ItzGlace/CGPanel/apt/cgpanel-archive-keyring.gpg -o /tmp/cgpanel-key.gpg
gpg --show-keys --with-fingerprint /tmp/cgpanel-key.gpg
# Check fingerprint: A1CF E279 6A89 24B3 D025 62BA 81BA 83CC 10B1 2E2F
sudo install -m 0644 /tmp/cgpanel-key.gpg /usr/share/keyrings/cgpanel-archive-keyring.gpg
echo 'deb [arch=amd64 signed-by=/usr/share/keyrings/cgpanel-archive-keyring.gpg] https://raw.githubusercontent.com/ItzGlace/CGPanel/apt ./' | sudo tee /etc/apt/sources.list.d/cgpanel.list
sudo apt update
sudo apt install cgpanel
# Only for a fresh, dedicated host:
sudo cgpanel-setup --dedicated-host --public-ip YOUR_SERVER_IPV4
```

A package install on an existing configured CGPanel host runs the snapshot/health-check upgrade automatically. A fresh package install stages files; `cgpanel-setup` initializes hosting services. APT packages include checksum-verified rclone and Certbot bundles. Certbot and its Cloudflare DNS plugin install from pinned wheels without contacting PyPI or resolving newer versions on the host. Setup validates native dependencies and the plugin before activating the new environment; the previous environment is retained. Repeating an interrupted setup reuses a verified installed environment. Source installs fetch these pinned bundles from the CGPanel mirror. Fresh setup still needs outbound network access for Ubuntu packages and the application egress image.

For future updates:

```bash
sudo apt update
sudo apt install --only-upgrade cgpanel
```

`apt update` refreshes repository metadata; it does not accept a package name. `apt upgrade cgpanel` or `apt install --only-upgrade cgpanel` upgrades the package. The panel's Updates page and automatic update timer use this APT channel on package-managed installations. Existing source/bundle installations keep the GitHub release channel until migrated by APT. Running background jobs or insufficient disk can defer/reject an update while retaining the previous installation.

`apt remove cgpanel` stops panel services and removes packaged files. Hosted applications, databases, mail and host configuration are retained even on package purge. Use the explicit account-deletion interface when project data should be removed.

If fresh setup stopped while downloading rclone or resolving Certbot packages, upgrade CGPanel through APT and rerun the same `cgpanel-setup` command. The bundled dependencies need no pip timeout overrides. Do not delete the panel database or generate another administrator when recovering an existing configured panel.

The Certbot package versions, wheel names and SHA-256 pins are recorded in `deploy/certbot-wheels.json`. Maintainers can reproduce its archive from those wheels with `python3 scripts/build_certbot_bundle.py WHEEL_DIRECTORY OUTPUT_ARCHIVE`; the script rejects different wheel or archive hashes.

## Maintaining the signed repository

The `apt` Git branch contains a flat signed repository hosted over GitHub HTTPS. Public keys are committed; private signing keys must never be committed. The initial signing key is held in the private environment directory `/workspace/cgpanel-dev/apt-signing` (mode 0700). The repository owner should securely back up/control that key and add its armored secret-key export to the repository Actions secret **APT_SIGNING_KEY** to enable future automatic signed APT publication. The workflow checks the pinned fingerprint. Without that secret, GitHub binary releases still publish and the APT repository retains its last signed package.

The `Publish signed APT repository` workflow runs after a successful release or manually. It downloads the published `.deb`, verifies its release checksum, signs the package index and pushes the `apt` branch. Initial key expiration is November 19, 2028; rotate the key and pinned fingerprint before it expires. Keep the bootstrap, public key and signing workflow in sync during rotation.
