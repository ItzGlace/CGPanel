#!/usr/bin/env python3
"""Install v0.5 host dependencies without changing tenant application runtimes."""
import ipaddress,json,os,pathlib,re,shutil,subprocess,sys
root=pathlib.Path(sys.argv[1]).resolve();assert os.geteuid()==0
def run(*args,**kw):return subprocess.run(args,check=True,**kw)
packages=['libnginx-mod-http-modsecurity','vsftpd','bubblewrap','e2fsprogs','util-linux']
missing=[p for p in packages if subprocess.run(['dpkg-query','-W','-f=${Status}',p],capture_output=True,text=True).stdout!='install ok installed']
if missing:
    run('apt-get','update',env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'})
    run('apt-get','-o','DPkg::Lock::Timeout=60','install','-y',*missing,env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'})
lib=pathlib.Path('/usr/local/lib/cgpanel');lib.mkdir(parents=True,exist_ok=True)
shutil.copy2(root/'deploy/workspace-volume.py',lib/'workspace-volume.py');(lib/'workspace-volume.py').chmod(0o700)
shutil.copy2(root/'deploy/reload-nginx',lib/'reload-nginx');(lib/'reload-nginx').chmod(0o755)
shutil.copytree(root/'deploy/waf/crs',lib/'crs-v4.29.0',dirs_exist_ok=True)
for folder in ['/var/log/cgpanel-waf','/var/cache/cgpanel-waf']:
    pathlib.Path(folder).mkdir(mode=0o750,exist_ok=True);shutil.chown(folder,user='www-data',group='adm')
for mode,engine in [('detect','DetectionOnly'),('enforce','On')]:
    pathlib.Path('/etc/cgpanel/waf-'+mode+'.conf').write_text(f'''Include /etc/nginx/modsecurity.conf
SecRuleEngine {engine}
SecRequestBodyLimit 33554432
SecRequestBodyNoFilesLimit 1048576
SecAuditEngine RelevantOnly
SecAuditLogParts AHZ
SecAuditLog /var/log/cgpanel-waf/audit.log
SecDataDir /var/cache/cgpanel-waf
Include /usr/local/lib/cgpanel/crs-v4.29.0/crs-setup.conf.example
Include /usr/local/lib/cgpanel/crs-v4.29.0/rules/*.conf
''')
pathlib.Path('/etc/logrotate.d/cgpanel-waf').write_text('''/var/log/cgpanel-waf/*.log {
 daily
 maxsize 50M
 rotate 4
 missingok
 notifempty
 compress
 delaycompress
 create 0640 www-data adm
 sharedscripts
 postrotate
  /usr/sbin/nginx -s reopen >/dev/null 2>&1 || true
 endscript
}
''')
# Preserve all live firewall rules and elements while upgrading set types atomically.
def firewall_upgrade(text):
    for name in ['blocked4','blocked6','database4','database6']:
        pattern=r'(set '+name+r'\s*\{)([^}]*)(\})'
        def update(match):
            body=match[2]
            if 'flags interval' not in body:body=re.sub(r'(type ipv[46]_addr)[ \t]*;?',r'\1; flags interval;',body)
            return match[1]+body+match[3]
        text=re.sub(pattern,update,text,flags=re.S)
    if 'set ftps_ports' not in text:
        text=text.replace('table inet cgpanel {','table inet cgpanel {\n set ftps_ports { type inet_service; flags interval; }',1)
        text=re.sub(r'(\s+tcp dport 22 accept)',r'\n  tcp dport @ftps_ports ct state new limit rate 50/second burst 100 packets accept\1',text,count=1)
    if 'set mail_ports' not in text:
        text=text.replace('table inet cgpanel {','table inet cgpanel {\n set mail_ports { type inet_service; flags interval; }',1)
        text=re.sub(r'(\s+tcp dport 22 accept)',r'\n  tcp dport @mail_ports ct state new limit rate 50/second burst 100 packets accept\1',text,count=1)
    return text
live=run('nft','list','table','inet','cgpanel',capture_output=True,text=True).stdout
updated=firewall_upgrade(live)
if updated!=live:
    script='delete table inet cgpanel\n'+updated
    run('nft','-c','-f','-',input=script,text=True);run('nft','-f','-',input=script,text=True)
path=pathlib.Path('/etc/cgpanel/firewall.nft');path.write_text(firewall_upgrade(path.read_text()))
pathlib.Path('/etc/nginx/conf.d/03-cgpanel-ide.conf').write_text('server { listen 8443 ssl default_server; listen [::]:8443 ssl default_server; ssl_certificate /etc/cgpanel/panel.crt; ssl_certificate_key /etc/cgpanel/panel.key; return 444; }\ninclude /etc/nginx/cgpanel-ide/*.conf;\n')
pathlib.Path('/etc/cgpanel/ftps-users').mkdir(mode=0o700,exist_ok=True)
for name,value in [('ftps-users.list',''),('ftps-access.conf','-|ALL|ALL\n')]:
    file=pathlib.Path('/etc/cgpanel')/name
    if not file.exists():file.write_text(value);file.chmod(0o600)
values={k:v for k,v in (line.split('=',1) for line in pathlib.Path('/etc/cgpanel/agent.env').read_text().splitlines() if '=' in line and not line.startswith('#'))}
address=str(ipaddress.ip_address(values['CGPANEL_PUBLIC_IP']))
config=pathlib.Path('/etc/vsftpd.conf')
if config.exists() and '# CGPanel managed' not in config.read_text():shutil.copy2(config,'/etc/cgpanel/vsftpd-before-v5.conf')
config.write_text(f'''# CGPanel managed explicit TLS file transfer
listen=YES
listen_ipv6=NO
anonymous_enable=NO
local_enable=YES
write_enable=YES
local_umask=007
chroot_local_user=YES
allow_writeable_chroot=YES
userlist_enable=YES
userlist_deny=NO
userlist_file=/etc/cgpanel/ftps-users.list
user_config_dir=/etc/cgpanel/ftps-users
pam_service_name=cgpanel-ftps
ssl_enable=YES
force_local_logins_ssl=YES
force_local_data_ssl=YES
ssl_sslv2=NO
ssl_sslv3=NO
ssl_ciphers=HIGH:!aNULL:!MD5:!3DES
rsa_cert_file=/etc/cgpanel/panel.crt
rsa_private_key_file=/etc/cgpanel/panel.key
require_ssl_reuse=NO
pasv_enable=YES
pasv_min_port=60000
pasv_max_port=60100
pasv_address={address}
port_enable=NO
max_clients=50
max_per_ip=5
idle_session_timeout=300
data_connection_timeout=120
xferlog_enable=YES
seccomp_sandbox=NO
''')
pathlib.Path('/etc/pam.d/cgpanel-ftps').write_text('auth required pam_unix.so\naccount required pam_unix.so\naccount required pam_access.so accessfile=/etc/cgpanel/ftps-access.conf fieldsep=|\nsession required pam_unix.so\n')
# Ubuntu's vsftpd seccomp filter kills OpenSSL 3 getsockopt during TLS.
# Keep chroot/privilege separation and apply a compatible service-level filter.
unit=pathlib.Path('/etc/systemd/system/vsftpd.service.d');unit.mkdir(exist_ok=True)
(unit/'cgpanel.conf').write_text('[Service]\nProtectSystem=full\nPrivateTmp=true\nProtectKernelTunables=true\nProtectKernelModules=true\nProtectControlGroups=true\nRestrictRealtime=true\nRestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX AF_NETLINK\nSystemCallFilter=~@reboot @swap @raw-io @debug\nSystemCallErrorNumber=EPERM\n')
run('systemctl','daemon-reload')
run('systemctl','enable','--now','vsftpd');run('systemctl','restart','vsftpd')
run('nginx','-t');run('systemctl','reload','nginx')
print('CGPanel v0.5 host support ready; site protections remain opt-in.')

run('python3',str(root/'deploy/setup-mail.py'))
shutil.copy2(root/'deploy/mail-config.py',lib/'mail-config.py');(lib/'mail-config.py').chmod(0o700)

run('python3',str(root/'deploy/adopt-sftp.py'))
