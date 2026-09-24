#!/usr/bin/env bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
[[ $EUID == 0 ]] || { echo 'Run as root on a dedicated Ubuntu 24.04 host.'; exit 1; }
[[ ${1:-} == --dedicated-host ]] || { echo 'This installer configures Nginx, DNS, database access, and a dedicated firewall. Use --dedicated-host on a fresh dedicated Ubuntu 24.04 server.'; exit 1; }
. /etc/os-release
[[ $ID == ubuntu && $VERSION_ID == 24.04 ]] || { echo 'Supported host: Ubuntu 24.04'; exit 1; }
CG_SOURCE=$(cd -- "$(dirname -- "$0")/.." && pwd)
: "${CGPANEL_PUBLIC_IP:?Set CGPANEL_PUBLIC_IP to this server IPv4 address}"
python3 -c 'import ipaddress,os; ipaddress.IPv4Address(os.environ["CGPANEL_PUBLIC_IP"])'
if [[ ${CGPANEL_SKIP_PACKAGES:-0} != 1 ]]; then
 apt-get update
 apt-get install -y build-essential pkg-config libssl-dev curl git nginx podman uidmap slirp4netns fuse-overlayfs dbus-user-session mariadb-server postgresql bind9 bind9-utils nftables fail2ban certbot python3-certbot-nginx openssl
fi
command -v cargo >/dev/null || { echo 'Install a current stable Rust toolchain, then rerun. See https://rustup.rs'; exit 1; }
cd "$CG_SOURCE"
cargo build --release --locked
id cgpanel >/dev/null 2>&1 || useradd --system --home /var/lib/cgpanel --shell /usr/sbin/nologin cgpanel
install -d -m 0750 -o cgpanel -g cgpanel /var/lib/cgpanel
install -d -m 0755 /usr/local/lib/cgpanel /srv/cgpanel /srv/cgpanel/public /etc/cgpanel /etc/bind/cgpanel
install -d -m 0711 /srv/cgpanel/tenants
install -d -m 0700 /var/lib/cgpanel-agent /var/lib/cgpanel-agent/backups
install -m 0755 target/release/cgpanel target/release/cgpanel-agent target/release/cgpanel-workspace /usr/local/bin/
for f in /etc/nftables.conf /etc/bind/named.conf.local /etc/bind/named.conf.options /etc/postgresql/16/main/pg_hba.conf; do
 [[ -f "$f.pre-cgpanel" ]] || cp -a "$f" "$f.pre-cgpanel"
done
if [[ ! -f /etc/cgpanel/panel.key ]]; then
 openssl req -x509 -newkey rsa:3072 -sha256 -days 365 -nodes -keyout /etc/cgpanel/panel.key -out /etc/cgpanel/panel.crt -subj '/CN=CGPanel development host' -addext "subjectAltName=IP:${CGPANEL_PUBLIC_IP},DNS:localhost"
fi
chmod 0600 /etc/cgpanel/panel.key
install -d -m 0750 -o mysql -g mysql /etc/mysql/cgpanel
install -m 0600 -o mysql -g mysql /etc/cgpanel/panel.key /etc/mysql/cgpanel/server.key
install -m 0644 -o mysql -g mysql /etc/cgpanel/panel.crt /etc/mysql/cgpanel/server.crt
cat > /etc/mysql/mariadb.conf.d/90-cgpanel.cnf <<'EOF'
[mysqld]
bind-address=0.0.0.0
ssl-cert=/etc/mysql/cgpanel/server.crt
ssl-key=/etc/mysql/cgpanel/server.key
local-infile=0
max_connections=100
EOF
cat > /etc/postgresql/16/main/conf.d/cgpanel.conf <<'EOF'
listen_addresses = '*'
ssl = on
password_encryption = 'scram-sha-256'
max_connections = 100
EOF
cat > /etc/nginx/conf.d/00-cgpanel-limits.conf <<'EOF'
limit_req_zone $binary_remote_addr zone=cgp_web:10m rate=15r/s;
limit_req_zone $binary_remote_addr zone=cgp_panel:10m rate=10r/s;
limit_conn_zone $binary_remote_addr zone=cgp_conn:10m;
map $http_upgrade $cg_connection { default upgrade; '' close; }
server_tokens off;
client_header_timeout 15s;
client_body_timeout 30s;
send_timeout 30s;
EOF
cat > /etc/nginx/conf.d/01-cgpanel.conf <<'EOF'
server {
 listen 2083 ssl; listen [::]:2083 ssl;
 server_name _;
 ssl_certificate /etc/cgpanel/panel.crt;
 ssl_certificate_key /etc/cgpanel/panel.key;
 ssl_protocols TLSv1.2 TLSv1.3;
 ssl_session_tickets off;
 client_max_body_size 512k;
 limit_req zone=cgp_panel burst=30 nodelay;
 limit_conn cgp_conn 30;
 add_header Strict-Transport-Security 'max-age=31536000' always;
 location / {
  proxy_pass http://127.0.0.1:2082;
  proxy_set_header Host $host;
  proxy_set_header X-CGPanel-Client-IP $remote_addr;
  proxy_set_header X-Forwarded-Proto https;
  proxy_read_timeout 370s;
 }
}
EOF
cat > /etc/bind/named.conf.options <<'EOF'
options {
 directory "/var/cache/bind";
 listen-on { any; }; listen-on-v6 { any; };
 recursion no;
 allow-query { any; };
 allow-transfer { none; };
 minimal-responses yes;
 version "CGPanel DNS";
 rate-limit { responses-per-second 10; window 5; };
 dnssec-validation auto;
};
EOF
touch /etc/bind/cgpanel/zones.conf
grep -q 'cgpanel/zones.conf' /etc/bind/named.conf.local || printf '\ninclude "/etc/bind/cgpanel/zones.conf";\n' >> /etc/bind/named.conf.local
cp deploy/cgpanel.nft /etc/cgpanel/firewall.nft
if nft list table inet cgpanel >/dev/null 2>&1; then nft delete table inet cgpanel; fi
nft -f /etc/cgpanel/firewall.nft
cat > /etc/nftables.conf <<'EOF'
#!/usr/sbin/nft -f
flush ruleset
include "/etc/cgpanel/firewall.nft"
EOF
cat > /etc/fail2ban/jail.d/cgpanel.local <<'EOF'
[sshd]
enabled = true
backend = systemd
maxretry = 5
findtime = 600
bantime = 3600
banaction = nftables-multiport
EOF
cat > /etc/sysctl.d/90-cgpanel.conf <<'EOF'
net.ipv4.tcp_syncookies=1
net.ipv4.conf.all.accept_redirects=0
net.ipv4.conf.default.accept_redirects=0
net.ipv6.conf.all.accept_redirects=0
net.ipv4.icmp_echo_ignore_broadcasts=1
net.ipv4.conf.all.send_redirects=0
EOF
sysctl --system >/dev/null
printf 'CGPANEL_PUBLIC_IP=%s\n' "$CGPANEL_PUBLIC_IP" > /etc/cgpanel/agent.env
printf 'CGPANEL_IMAGE_REGISTRY=%s\n' "${CGPANEL_IMAGE_REGISTRY:-docker.io}" >> /etc/cgpanel/agent.env
chmod 0600 /etc/cgpanel/agent.env
install -m 0644 deploy/cgpanel.service deploy/cgpanel-agent.service /etc/systemd/system/
bash deploy/install-features.sh
systemctl daemon-reload
nginx -t
named-checkconf
systemctl restart mariadb postgresql named nginx fail2ban
systemctl enable nginx named mariadb postgresql nftables fail2ban cgpanel-agent cgpanel
if [[ ! -f /var/lib/cgpanel/panel.db ]]; then
 if [[ -z ${CGPANEL_ADMIN_PASSWORD:-} ]]; then
  CGPANEL_ADMIN_PASSWORD=$(openssl rand -base64 30)
  umask 077
  printf 'Username: admin\nPassword: %s\nPanel: https://%s:2083\n' "$CGPANEL_ADMIN_PASSWORD" "$CGPANEL_PUBLIC_IP" > /root/cgpanel-initial-admin.txt
 fi
 export CGPANEL_ADMIN_PASSWORD
 runuser -u cgpanel -- env CGPANEL_DB=/var/lib/cgpanel/panel.db CGPANEL_ADMIN_PASSWORD="$CGPANEL_ADMIN_PASSWORD" /usr/local/bin/cgpanel bootstrap
 unset CGPANEL_ADMIN_PASSWORD
fi
python3 deploy/setup-v0.4.py "$CG_SOURCE"
printf '0.5.0\n' > /etc/cgpanel/version
systemctl restart cgpanel-agent cgpanel
systemctl is-active --quiet cgpanel-agent cgpanel
echo "CGPanel installed: https://${CGPANEL_PUBLIC_IP}:2083"
echo 'Initial TLS uses a self-signed certificate. Configure a trusted panel certificate before general use.'
echo 'If generated, administrator credentials are in /root/cgpanel-initial-admin.txt (root-only).'
