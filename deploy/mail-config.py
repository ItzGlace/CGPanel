#!/usr/bin/env python3
"""Apply root-broker validated mail configuration. JSON arrives on stdin."""
import atexit,json,os,pathlib,pwd,re,subprocess,sys
assert os.geteuid()==0
value=json.load(sys.stdin)
# Restore the previous configuration if validation or a service restart fails.
files=['/etc/postfix/main.cf','/etc/postfix/master.cf','/etc/dovecot/dovecot.conf']
files += ['/etc/postfix/cgpanel/'+n for n in ['passwd','domains','mailboxes','senders','domains.db','mailboxes.db','senders.db']]
files += ['/etc/rspamd/local.d/'+n for n in ['worker-proxy.inc','worker-normal.inc','worker-controller.inc','redis.conf','dkim_signing.conf']]
previous={}
for name in files:
    p=pathlib.Path(name)
    if p.exists():
        st=p.stat();previous[name]=(p.read_bytes(),st.st_mode&0o7777,st.st_uid,st.st_gid)
    else:previous[name]=None
success=False
def rollback():
    if success:return
    for name,old in previous.items():
        p=pathlib.Path(name)
        if old is None:p.unlink(missing_ok=True)
        else:p.write_bytes(old[0]);p.chmod(old[1]);os.chown(p,old[2],old[3])
    subprocess.run(['systemctl','restart','postfix','dovecot','rspamd'],capture_output=True)
atexit.register(rollback)

def run(*args,**kwargs):
    result=subprocess.run(args,capture_output=True,text=True,**kwargs)
    if result.returncode:raise RuntimeError(args[0]+': '+result.stderr[-3000:])
    return result.stdout
def write(path,text,mode=0o640,group=None):
    p=pathlib.Path(path);p.parent.mkdir(parents=True,exist_ok=True);temp=p.with_suffix(p.suffix+'.tmp');temp.write_text(text);temp.chmod(mode)
    if group:run('chown','root:'+group,str(temp))
    temp.replace(p)
host=value['hostname'];assert re.fullmatch(r'[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?',host) and '.' in host
uid=pwd.getpwnam('cgpvmail').pw_uid;gid=pwd.getpwnam('cgpvmail').pw_gid
base='/etc/postfix/cgpanel';domains=value['domains'];boxes=value['mailboxes']
assert len(boxes)<=10000 and len(domains)<=10000
passwd=[];recipients=[];senders=[]
for box in boxes:
    address=box['address'];assert re.fullmatch(r'[a-z0-9][a-z0-9._-]{0,63}@[a-z0-9.-]+',address)
    owner=box['owner'];identifier=box['id'];assert re.fullmatch('[a-f0-9]{32}',owner) and re.fullmatch('[a-f0-9]{32}',identifier)
    size=box['quota_mb'];assert isinstance(size,int) and 64<=size<=102400
    password=box['hash'];assert re.fullmatch(r'\{SHA512-CRYPT\}\$6\$[./a-zA-Z0-9$]+',password)
    home=f'/var/lib/cgpanel-mail/{owner}/{identifier}'
    tenant=pathlib.Path(home).parent;tenant.mkdir(parents=True,exist_ok=True);os.chown(tenant,0,gid);tenant.chmod(0o710)
    pathlib.Path(home).mkdir(parents=True,exist_ok=True);os.chown(home,uid,gid);os.chmod(home,0o700)
    passwd.append(f'{address}:{password}:{uid}:{gid}::{home}::userdb_quota_rule=*:storage={size}M')
    recipients.append(f'{address} OK');senders.append(f'{address} {address}')
for name,lines in [('passwd',passwd),('domains',[d+' OK' for d in domains]),('mailboxes',recipients),('senders',senders)]:
    write(f'{base}/{name}','\n'.join(lines)+'\n',group='dovecot')
    if name!='passwd':
        run('postmap',f'hash:{base}/{name}');os.chmod(f'{base}/{name}.db',0o644)
cert=value.get('cert','');assert not cert or re.fullmatch(r'(?:cgp_[a-f0-9]{32}|[a-z0-9.-]+)',cert)
certificate=f'/etc/letsencrypt/live/{cert}/fullchain.pem' if cert else '/etc/cgpanel/panel.crt'
key=f'/etc/letsencrypt/live/{cert}/privkey.pem' if cert else '/etc/cgpanel/panel.key'
assert pathlib.Path(certificate).exists() and pathlib.Path(key).exists()
write('/etc/dovecot/dovecot.conf',f'''protocols = imap lmtp
listen = *, ::
ssl = required
ssl_min_protocol = TLSv1.2
ssl_cert = <{certificate}
ssl_key = <{key}
disable_plaintext_auth = yes
auth_mechanisms = plain login
auth_username_format = %Lu
mail_location = maildir:~/Maildir
mail_uid = {uid}
mail_gid = {gid}
first_valid_uid = {uid}
mail_privileged_group =
mail_plugins = quota
passdb {{
 driver = passwd-file
 args = username_format=%u {base}/passwd
}}
userdb {{
 driver = passwd-file
 args = username_format=%u {base}/passwd
}}
service auth {{
 unix_listener /var/spool/postfix/private/auth {{
  mode = 0660
  user = postfix
  group = postfix
 }}
}}
service lmtp {{
 unix_listener /var/spool/postfix/private/dovecot-lmtp {{
  mode = 0600
  user = postfix
  group = postfix
 }}
}}
service imap-login {{
 inet_listener imap {{
  port = 0
 }}
 inet_listener imaps {{
  port = 993
  ssl = yes
 }}
}}
protocol imap {{
 mail_plugins = $mail_plugins imap_quota
}}
protocol lmtp {{
 postmaster_address = postmaster@{host}
}}
plugin {{
 quota = maildir:User quota
}}
''')
settings={'myhostname':host,'mydestination':'localhost','inet_interfaces':'all','inet_protocols':'all','mynetworks':'127.0.0.0/8 [::1]/128',
 'virtual_mailbox_domains':f'hash:{base}/domains','virtual_mailbox_maps':f'hash:{base}/mailboxes','virtual_transport':'lmtp:unix:private/dovecot-lmtp',
 'smtpd_relay_restrictions':'permit_sasl_authenticated, reject_unauth_destination','smtpd_recipient_restrictions':'reject_non_fqdn_recipient, reject_unknown_recipient_domain, reject_unlisted_recipient',
 'smtpd_sasl_type':'dovecot','smtpd_sasl_path':'private/auth','smtpd_sasl_auth_enable':'no','smtpd_sasl_security_options':'noanonymous',
 'smtpd_sender_login_maps':f'hash:{base}/senders','smtpd_tls_cert_file':certificate,'smtpd_tls_key_file':key,'smtpd_tls_security_level':'may','smtpd_tls_auth_only':'yes',
 'smtpd_tls_mandatory_protocols':'>=TLSv1.2','smtp_tls_security_level':'may','smtp_tls_CApath':'/etc/ssl/certs','smtpd_milters':'inet:127.0.0.1:11332','non_smtpd_milters':'inet:127.0.0.1:11332','milter_default_action':'tempfail','milter_protocol':'6',
 'message_size_limit':'33554432','smtpd_client_message_rate_limit':'60','smtpd_client_connection_rate_limit':'30','smtpd_client_connection_count_limit':'20','disable_vrfy_command':'yes'}
for key,value in settings.items():run('postconf','-e',f'{key} = {value}')
for name,wrapper in [('submission','no'),('submissions','yes')]:
    run('postconf','-M',f'{name}/inet={name} inet n - y - - smtpd')
    for key,value in {'smtpd_tls_security_level':'encrypt','smtpd_tls_wrappermode':wrapper,'smtpd_sasl_auth_enable':'yes','smtpd_relay_restrictions':'permit_sasl_authenticated,reject','smtpd_sender_restrictions':'reject_authenticated_sender_login_mismatch','smtpd_recipient_restrictions':'permit_sasl_authenticated,reject'}.items():run('postconf','-P',f'{name}/inet/{key}={value}')
write('/etc/rspamd/local.d/worker-proxy.inc','bind_socket = "127.0.0.1:11332";\nmilter = yes;\ncount = 1;\nupstream "local" { default = yes; self_scan = yes; }\n',0o644)
write('/etc/rspamd/local.d/worker-normal.inc','bind_socket = "127.0.0.1:11333";\ncount = 1;\n',0o644)
write('/etc/rspamd/local.d/worker-controller.inc','bind_socket = "127.0.0.1:11334";\n',0o644)
write('/etc/rspamd/local.d/redis.conf','servers = "127.0.0.1";\n',0o644)
write('/etc/rspamd/local.d/dkim_signing.conf','selector = "cgpanel";\npath = "/var/lib/rspamd/dkim/$domain.key";\nallow_username_mismatch = false;\nsign_authenticated = true;\nsign_local = false;\n',0o644)
dkim=pathlib.Path('/var/lib/rspamd/dkim');dkim.mkdir(exist_ok=True);run('chown','_rspamd:_rspamd',str(dkim));dkim.chmod(0o750)
for domain in domains:
    assert re.fullmatch(r'[a-z0-9.-]+',domain)
    private=dkim/(domain+'.key')
    if not private.exists():
        public=run('rspamadm','dkim_keygen','-b','2048','-s','cgpanel','-d',domain,'-k',str(private))
        write(str(dkim/(domain+'.txt')),public,0o644)
        run('chown','_rspamd:_rspamd',str(private));private.chmod(0o600)
run('doveconf','-n');run('postfix','check');run('rspamadm','configtest')
run('systemctl','enable','--now','postfix','dovecot','rspamd','redis-server')
run('systemctl','restart','postfix','dovecot','rspamd')
success=True
print(json.dumps({'configured':True,'certificate_trusted':bool(cert)}))
