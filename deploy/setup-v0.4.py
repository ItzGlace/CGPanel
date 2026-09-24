#!/usr/bin/env python3
import json, pathlib, re, shutil, subprocess, sys
root=pathlib.Path(sys.argv[1]).resolve()
lib=pathlib.Path('/usr/local/lib/cgpanel');lib.mkdir(parents=True,exist_ok=True)
shutil.copytree(root/'deploy/php-runtime',lib/'php-runtime',dirs_exist_ok=True)
shutil.copy2(root/'deploy/updater.py',lib/'updater.py')
(lib/'updater.py').chmod(0o700)
config=pathlib.Path('/etc/cgpanel/updates.json')
if not config.exists():config.write_text(json.dumps({'enabled':True}));config.chmod(0o600)
state=pathlib.Path('/var/lib/cgpanel-updates');state.mkdir(mode=0o700,exist_ok=True)
pathlib.Path('/etc/nginx/cgpanel-ide').mkdir(exist_ok=True)
pathlib.Path('/etc/nginx/conf.d/03-cgpanel-ide.conf').write_text('include /etc/nginx/cgpanel-ide/*.conf;\n')
firewall=pathlib.Path('/etc/cgpanel/firewall.nft')
text=firewall.read_text()
if 'ide_ports' not in text:
    text=text.replace(' chain input {',' set ide_ports { type inet_service; }\n chain input {').replace('  tcp dport 22 accept','  tcp dport @ide_ports ct state new limit rate 50/second burst 100 packets accept\n  tcp dport 22 accept')
    firewall.write_text(text)
    subprocess.run(['nft','add','set','inet','cgpanel','ide_ports','{','type','inet_service',';','}'],check=True)
    subprocess.run(['nft','add','rule','inet','cgpanel','input','tcp','dport','@ide_ports','ct','state','new','limit','rate','50/second','burst','100','packets','accept'],check=True)
for unit,mode in [('cgpanel-update-check','--check'),('cgpanel-update','--apply'),('cgpanel-auto-update','--auto')]:
    pathlib.Path('/etc/systemd/system/'+unit+'.service').write_text(f'''[Unit]
Description=CGPanel stable release updater ({mode})
After=network-online.target
[Service]
Type=oneshot
ExecStart=/usr/bin/python3 /usr/local/lib/cgpanel/updater.py {mode}
TimeoutStartSec=20min
UMask=0077
''')
pathlib.Path('/etc/systemd/system/cgpanel-auto-update.timer').write_text('''[Unit]
Description=Check for stable CGPanel updates every six hours
[Timer]
OnBootSec=15min
OnUnitActiveSec=6h
RandomizedDelaySec=15min
Persistent=true
[Install]
WantedBy=timers.target
''')
subprocess.run(['nginx','-t'],check=True)
subprocess.run(['systemctl','daemon-reload'],check=True)
subprocess.run(['systemctl','enable','--now','cgpanel-auto-update.timer'],check=True)
subprocess.run(['systemctl','reload','nginx'],check=True)
