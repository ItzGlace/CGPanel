#!/usr/bin/env bash
# Download over HTTPS, verify the pinned archive key, then let APT verify packages.
set -euo pipefail
cgpanel_install() {
 [[ $EUID == 0 ]] || { echo 'Run this installer with sudo bash.' >&2; return 1; }
 . /etc/os-release
 [[ $ID == ubuntu && $VERSION_ID == 24.04 && $(dpkg --print-architecture) == amd64 ]] || { echo 'Supported system: Ubuntu 24.04 amd64.' >&2; return 1; }
 local cg_install_ip='' cg_dedicated=0
 while (($#)); do
  case $1 in
   --dedicated-host) cg_dedicated=1; shift;;
   --public-ip) [[ $# -ge 2 ]] || return 1; cg_install_ip=$2; shift 2;;
   *) echo "Unknown option: $1" >&2; return 1;;
  esac
 done
 if [[ ! -f /var/lib/cgpanel/panel.db ]]; then
  [[ $cg_dedicated == 1 && -n $cg_install_ip ]] || { echo 'Fresh install: pass --dedicated-host --public-ip YOUR_SERVER_IPV4. CGPanel configures firewall, DNS, databases, mail and Nginx.' >&2; return 1; }
  python3 - "$cg_install_ip" <<'PY'
import ipaddress,sys
ipaddress.IPv4Address(sys.argv[1])
PY
 fi
 export DEBIAN_FRONTEND=noninteractive
 local cg_apt_options=(-o APT::Update::Error-Mode=any -o DPkg::Lock::Timeout=120 -o Acquire::Retries=2 -o Acquire::http::Timeout=30 -o Acquire::https::Timeout=30)
 timeout 1200 apt-get "${cg_apt_options[@]}" update
 timeout 1200 apt-get "${cg_apt_options[@]}" install -y ca-certificates curl gnupg
 local cg_stage
 cg_stage=$(mktemp -d /tmp/cgpanel-install.XXXXXXXX)
 chmod 0700 "$cg_stage"
 trap 'rm -rf -- "${cg_stage:-}"' EXIT
 curl --proto '=https' --tlsv1.2 --fail --show-error --location --retry 2 --max-time 120 https://raw.githubusercontent.com/ItzGlace/CGPanel/apt/cgpanel-archive-keyring.gpg -o "$cg_stage/key.gpg"
 local cg_fingerprints
 cg_fingerprints=$(gpg --homedir "$cg_stage" --batch --show-keys --with-colons "$cg_stage/key.gpg" | awk -F: '$1=="fpr" {print $10}')
 [[ $cg_fingerprints == A1CFE2796A8924B3D02562BA81BA83CC10B12E2F ]] || { echo 'Archive key fingerprint mismatch; installation refused.' >&2; return 1; }
 install -m 0644 "$cg_stage/key.gpg" /usr/share/keyrings/cgpanel-archive-keyring.gpg
 printf '%s\n' 'deb [arch=amd64 signed-by=/usr/share/keyrings/cgpanel-archive-keyring.gpg] https://raw.githubusercontent.com/ItzGlace/CGPanel/apt ./' > /etc/apt/sources.list.d/cgpanel.list
 printf '%s\n' 'postfix postfix/main_mailer_type select Local only' 'postfix postfix/mailname string localhost' | debconf-set-selections
 timeout 1200 apt-get "${cg_apt_options[@]}" update
 timeout 2400 apt-get "${cg_apt_options[@]}" install -y cgpanel
 if [[ ! -f /var/lib/cgpanel/panel.db ]]; then
  cgpanel-setup --dedicated-host --public-ip "$cg_install_ip"
 fi
 echo 'CGPanel installation complete. Future updates: apt update && apt install --only-upgrade cgpanel'
 rm -rf -- "$cg_stage"
 trap - EXIT
}
cgpanel_install "$@"
