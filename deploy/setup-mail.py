#!/usr/bin/env python3
"""Install the Ubuntu 24.04 mail stack. Public ports stay closed until configured."""
import os,pathlib,subprocess
assert os.geteuid()==0
def run(*args,**kw):return subprocess.run(args,check=True,**kw)
packages=['postfix','dovecot-core','dovecot-imapd','dovecot-lmtpd','rspamd','redis-server']
missing=[p for p in packages if subprocess.run(['dpkg-query','-W','-f=${Status}',p],capture_output=True,text=True).stdout!='install ok installed']
if missing:
    run('debconf-set-selections',input='postfix postfix/main_mailer_type select Local only\npostfix postfix/mailname string localhost\n',text=True)
    run('apt-get','-o','DPkg::Lock::Timeout=60','install','-y',*missing,env={**os.environ,'DEBIAN_FRONTEND':'noninteractive'})
if subprocess.run(['id','cgpvmail'],capture_output=True).returncode:
    run('useradd','--system','--home-dir','/var/lib/cgpanel-mail','--create-home','--shell','/usr/sbin/nologin','cgpvmail')
folder=pathlib.Path('/etc/postfix/cgpanel');folder.mkdir(mode=0o755,exist_ok=True);folder.chmod(0o755)
run('chown','root:dovecot',str(folder))
for name in ['passwd','domains','mailboxes','senders']:
    path=folder/name
    if not path.exists():path.write_text('');path.chmod(0o640);run('chown','root:dovecot',str(path))
run('postconf','-e','inet_interfaces = loopback-only')
run('systemctl','enable','--now','redis-server','rspamd')
print('Mail packages installed. Configure a mail hostname in CGPanel to activate mail services.')
