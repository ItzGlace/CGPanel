#!/usr/bin/env python3
"""Build an Ubuntu 24.04 amd64 package without build tools on the hosting server."""
import json,os,pathlib,shutil,subprocess,sys,tempfile,tomllib
root=pathlib.Path(__file__).resolve().parents[1];version=tomllib.loads((root/'Cargo.toml').read_text())['package']['version']
target=pathlib.Path(os.environ.get('CARGO_TARGET_DIR',root/'target'))/'release'
output=pathlib.Path(sys.argv[1] if len(sys.argv)>1 else root/'dist').resolve();output.mkdir(parents=True,exist_ok=True)
rclone_archive=root/'target/downloads/rclone-v1.75.1-linux-amd64.zip'
if not rclone_archive.is_file():raise SystemExit('Prepare the pinned rclone archive first: python3 deploy/install-rclone.py --download-only target/downloads/rclone-v1.75.1-linux-amd64.zip')
if __import__('hashlib').sha256(rclone_archive.read_bytes()).hexdigest()!='982b5aa772841168f8e380f139e9e787b2a105403e32b94da8676a0e1c0a13ab':raise SystemExit('Pinned rclone archive checksum mismatch')
certbot=json.loads((root/'deploy/certbot-wheels.json').read_text())
certbot_archive=root/'target/downloads'/certbot['archive']
if not certbot_archive.is_file():raise SystemExit('Prepare the pinned Certbot bundle first: python3 deploy/install-certbot.py --download-only target/downloads/'+certbot['archive'])
if __import__('hashlib').sha256(certbot_archive.read_bytes()).hexdigest()!=certbot['sha256']:raise SystemExit('Pinned Certbot bundle checksum mismatch')
deps=['ca-certificates','curl','gnupg','python3','python3-venv','python3-pymysql','python3-psycopg2','python3-sqlparse','nginx','libnginx-mod-http-modsecurity','podman','uidmap','slirp4netns','fuse-overlayfs','dbus-user-session','mariadb-server','postgresql-16','bind9','bind9-utils','nftables','fail2ban','openssl','openssh-server','vsftpd','bubblewrap','e2fsprogs','util-linux','postfix','dovecot-core','dovecot-imapd','dovecot-lmtpd','rspamd','redis-server','rclone','iptables','libssl3t64','libc6 (>= 2.39)']
with tempfile.TemporaryDirectory(prefix='cgp-deb-') as temp:
 stage=pathlib.Path(temp);payload=stage/'usr/share/cgpanel/payload';payload.mkdir(parents=True);(payload/'bin').mkdir()
 for name in ['cgpanel','cgpanel-agent','cgpanel-workspace','cgpanel-egress','cgpanel-acme-hook']:
  shutil.copy2(target/name,payload/'bin'/name)
  subprocess.run(['strip','--strip-debug',str(payload/'bin'/name)],check=True)
 (payload/'tools').mkdir();shutil.copy2(rclone_archive,payload/'tools'/rclone_archive.name)
 shutil.copy2(certbot_archive,payload/'tools'/certbot_archive.name)
 shutil.copytree(root/'deploy',payload/'deploy',ignore=shutil.ignore_patterns('__pycache__','*.pyc','debian'))
 shutil.copy2(root/'Cargo.toml',payload/'Cargo.toml');(payload/'manifest.json').write_text(json.dumps({'version':version,'platform':'ubuntu-24.04-x86_64'}))
 (payload/'target').mkdir();(payload/'target/release').symlink_to('../bin')
 admin=stage/'usr/sbin';admin.mkdir(parents=True);shutil.copy2(root/'deploy/cgpanel-setup',admin/'cgpanel-setup');(admin/'cgpanel-setup').chmod(0o755)
 meta=stage/'DEBIAN';meta.mkdir()
 size=sum(p.stat().st_size for p in stage.rglob('*') if p.is_file())//1024
 (meta/'control').write_text(f'Package: cgpanel\nVersion: {version}\nArchitecture: amd64\nMaintainer: CGPanel maintainers <maintainers@cgpanel.invalid>\nSection: web\nPriority: optional\nInstalled-Size: {size}\nDepends: '+', '.join(deps)+'\nHomepage: https://github.com/ItzGlace/CGPanel\nDescription: CGPanel isolated application hosting panel\n Hosting panel, privileged provisioning broker and database workspace.\n Fresh hosts require cgpanel-setup with a public IP and dedicated-host flag.\n Existing installations are upgraded with snapshot and health-check recovery.\n')
 for name in ['postinst','prerm','postrm']:shutil.copy2(root/'deploy/debian'/name,meta/name);(meta/name).chmod(0o755)
 for path in stage.rglob('*'):
  if path.is_symlink():continue
  path.chmod(0o755 if path.is_dir() or path.stat().st_mode&0o111 else 0o644)
 subprocess.run(['dpkg-deb','--root-owner-group','--build','-Zxz','--threads-max=2',str(stage),str(output/f'cgpanel_{version}_amd64.deb')],check=True)
print(output/f'cgpanel_{version}_amd64.deb')
