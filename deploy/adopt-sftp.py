#!/usr/bin/env python3
"""Adopt an existing single-user chroot config only when its bind and group
unambiguously identify a CGPanel workspace. Passwords and files stay unchanged.
Run while the provisioning broker is stopped.
"""
import json,os,pathlib,pwd,re,subprocess,ipaddress
assert os.geteuid()==0
if subprocess.run(["systemctl","is-active","--quiet","cgpanel-agent"]).returncode==0:
    raise SystemExit("Stop cgpanel-agent before adopting transfer accounts")
file=pathlib.Path('/var/lib/cgpanel-agent/registry.json')
if not file.exists():raise SystemExit(0)
registry=json.loads(file.read_text());changed=False
mounts=[line.split() for line in pathlib.Path('/etc/fstab').read_text().splitlines() if line.strip() and not line.lstrip().startswith('#')]
for app,item in registry['items'].items():
    if item['kind']!='apps' or item['data'].get('transfer_user'):continue
    tenant=item['tenant'];workspace=f'/srv/cgpanel/tenants/{tenant}/apps/{app}'
    try:group=pwd.getpwnam('cg_'+tenant[:20]).pw_gid
    except KeyError:continue
    for config in pathlib.Path('/etc/ssh/sshd_config.d').glob('*.conf'):
        if config.is_symlink():continue
        text=config.read_text();users=re.findall(r'^Match User ([a-z_][a-z0-9_-]{0,31})\s*$',text,re.M)
        root=re.findall(r'^\s*ChrootDirectory (/srv/sftp/[a-zA-Z0-9_-]+)\s*$',text,re.M)
        directory=re.findall(r'^\s*ForceCommand internal-sftp -d (/\w+) -u 0007\s*$',text,re.M)
        if len(users)!=1 or len(root)!=1 or len(directory)!=1:continue
        matches=re.findall(r'^\s*Match (.+)$',text,re.M)
        if any(m.strip() not in ('all','User '+users[0]) for m in matches):continue
        if re.search(r'^\s*DenyUsers\s+',text,re.M):continue
        ips=[];safe=True
        for rule in re.findall(r'^\s*AllowUsers\s+(.+)$',text,re.M):
            for token in rule.split():
                if token==users[0]:ips=[];break
                prefix=users[0]+'@'
                if not token.startswith(prefix):safe=False;break
                try:ips.append(str(ipaddress.ip_network(token[len(prefix):],strict=False)))
                except ValueError:safe=False;break
        if not safe:continue

        try:account=pwd.getpwnam(users[0])
        except KeyError:continue
        if account.pw_uid<1000 or account.pw_gid!=group:continue
        target=root[0]+directory[0]
        if not any(len(m)>=4 and m[0]==workspace and m[1]==target and 'bind' in m[3].split(',') for m in mounts):continue
        item['data'].update(transfer_user=users[0],transfer_root=root[0],transfer_directory=directory[0],transfer_config=str(config),sftp_enabled=True,ftps_enabled=False,transfer_ips=ips)
        changed=True;break
if changed:
    temp=file.with_suffix('.adopt.tmp');temp.write_text(json.dumps(registry));temp.chmod(0o600);temp.replace(file)
    print('Adopted existing workspace SFTP access; credentials preserved.')
