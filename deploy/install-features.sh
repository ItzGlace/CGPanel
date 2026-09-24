#!/usr/bin/env bash
# Additive prerequisites shared by fresh installs and upgrades.
set -euo pipefail
[[ $EUID == 0 ]] || exit 1
CG_SOURCE=$(cd -- "$(dirname -- "$0")/.." && pwd)
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y rclone python3-venv
python3 "$CG_SOURCE/deploy/install-rclone.py"
install -d -m 0755 /opt/cgpanel-tools /usr/local/lib/cgpanel /srv/cgpanel/acme
python3 -m venv /opt/cgpanel-tools/certbot
/opt/cgpanel-tools/certbot/bin/pip install --upgrade 'certbot>=5.4,<6' 'certbot-dns-cloudflare>=5.4,<6'
install -m 0755 "$CG_SOURCE/target/release/cgpanel-acme-hook" /usr/local/bin/
install -m 0755 "$CG_SOURCE/deploy/reload-nginx" /usr/local/lib/cgpanel/
install -m 0644 "$CG_SOURCE/deploy/cloudflare-realip.conf" /etc/cgpanel/
install -m 0644 "$CG_SOURCE/deploy/cgpanel-certbot.service" "$CG_SOURCE/deploy/cgpanel-certbot.timer" /etc/systemd/system/
CG_BUILD=$(mktemp -d /var/tmp/cgpanel-egress-build.XXXXXX)
trap 'rm -rf -- "$CG_BUILD"' EXIT
install -m 0755 "$CG_SOURCE/target/release/cgpanel-egress" "$CG_BUILD/cgpanel-egress"
install -m 0644 "$CG_SOURCE/deploy/Containerfile.egress" "$CG_BUILD/Containerfile"
podman build -t localhost/cgpanel-egress:0.2.0 "$CG_BUILD"
podman save -o /usr/local/lib/cgpanel/egress-image.tar.new localhost/cgpanel-egress:0.2.0
chmod 0644 /usr/local/lib/cgpanel/egress-image.tar.new
mv /usr/local/lib/cgpanel/egress-image.tar.new /usr/local/lib/cgpanel/egress-image.tar
systemctl daemon-reload
systemctl enable --now cgpanel-certbot.timer
