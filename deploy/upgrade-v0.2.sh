#!/usr/bin/env bash
# Upgrade an existing CGPanel host without replacing firewall/database configuration.
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'Run as root.'; exit 1; }
CG_SOURCE=$(cd -- "$(dirname -- "$0")/.." && pwd)
cd "$CG_SOURCE"
[[ -f /var/lib/cgpanel/panel.db && -f /etc/cgpanel/agent.env ]] || { echo 'Use install.sh for a fresh host.'; exit 1; }
cargo build --release --locked
bash deploy/install-features.sh
CG_SNAPSHOT="/var/backups/cgpanel-upgrade-$(date -u +%Y%m%d-%H%M%S)"
install -d -m 0700 "$CG_SNAPSHOT"
export CG_SNAPSHOT
python3 - <<'PY'
import os,sqlite3
with sqlite3.connect('/var/lib/cgpanel/panel.db') as source, sqlite3.connect(os.environ['CG_SNAPSHOT']+'/panel.db') as target:
    source.backup(target)
PY
install -d -m 0700 "$CG_SNAPSHOT/bin" "$CG_SNAPSHOT/etc"
cp -a /etc/cgpanel "$CG_SNAPSHOT/etc/"
cp -a /var/lib/cgpanel-agent/registry.json "$CG_SNAPSHOT/"
cp -a /usr/local/bin/cgpanel /usr/local/bin/cgpanel-agent "$CG_SNAPSHOT/bin/"
systemctl stop cgpanel cgpanel-agent
install -m 0755 target/release/cgpanel target/release/cgpanel-agent /usr/local/bin/
systemctl start cgpanel-agent cgpanel
systemctl is-active --quiet cgpanel-agent cgpanel
echo "CGPanel v0.2 installed. Recovery snapshot: $CG_SNAPSHOT"
