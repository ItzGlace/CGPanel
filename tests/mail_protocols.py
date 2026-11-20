#!/usr/bin/env python3
"""Real mail protocol tests. Run ONLY in a fresh disposable Ubuntu test container.
Systemd management is stubbed; configuration validators and protocol daemons are real.
No outbound network or external recipients are used.
"""
import email,imaplib,json,os,pathlib,secrets,smtplib,socket,ssl,subprocess,time
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1', 'Disposable container required'
ROOT=pathlib.Path(__file__).resolve().parents[1];work=pathlib.Path('/tmp/cgpanel-mail-test');work.mkdir()
def run(*args,input=None):
 p=subprocess.run(args,input=input,text=True,capture_output=True,timeout=60)
 if p.returncode:raise RuntimeError(args[0]+': '+p.stderr[-2000:])
 return p.stdout
# Replace only systemd service management in this disposable, non-systemd container.
# Configuration validation and all SMTP/IMAP traffic use the real programs.
shim=pathlib.Path('/usr/local/bin/systemctl');shim.write_text('#!/bin/sh\nexit 0\n');shim.chmod(0o755)
run('python3',str(ROOT/'deploy/setup-mail.py'))
certdir=pathlib.Path('/etc/cgpanel');certdir.mkdir(exist_ok=True)
run('openssl','req','-x509','-newkey','rsa:2048','-sha256','-days','1','-nodes','-keyout',str(certdir/'panel.key'),'-out',str(certdir/'panel.crt'),'-subj','/CN=mail.cgpanel.test','-addext','subjectAltName=DNS:mail.cgpanel.test')
password=secrets.token_urlsafe(24);hashed='{SHA512-CRYPT}'+run('openssl','passwd','-6','-stdin',input=password+'\n').strip()
boxes=[{'address':name+'@example.test','owner':'1'*32,'id':identifier*32,'quota_mb':64,'hash':hashed} for name,identifier in [('alice','a'),('bob','b')]]
config={'hostname':'mail.cgpanel.test','domains':['example.test'],'mailboxes':boxes,'cert':''}
assert json.loads(run('python3',str(ROOT/'deploy/mail-config.py'),input=json.dumps(config)))['configured']
run('redis-server','--daemonize','yes','--bind','127.0.0.1','--protected-mode','yes')
rspamd_dir=pathlib.Path('/run/rspamd');rspamd_dir.mkdir(exist_ok=True);run('chown','_rspamd:_rspamd',str(rspamd_dir))
logs=[];processes=[]
for command,name in [(['rspamd','-f','-u','_rspamd','-g','_rspamd'],'rspamd'),(['dovecot','-F'],'dovecot')]:
 log=(work/(name+'.log')).open('w');logs.append(log);processes.append(subprocess.Popen(command,stdout=log,stderr=log))
try:
 for port in [11332,993]:
  for _ in range(100):
   try:
    with socket.create_connection(('127.0.0.1',port),timeout=.2):break
   except OSError:time.sleep(.1)
  else:raise RuntimeError('Mail service did not start: '+str(port)+'; '+(work/'dovecot.log').read_text()[-1000:]+'; '+(work/'rspamd.log').read_text()[-1000:])
 run('postfix','start')
 for _ in range(100):
  try:
   with socket.create_connection(('127.0.0.1',587),timeout=.2):break
  except OSError:time.sleep(.1)
 else:raise RuntimeError('Submission listener not ready')
 trust=ssl.create_default_context(cafile=str(certdir/'panel.crt'))
 host='mail.cgpanel.test'
 # TLS is required and AUTH is not offered before STARTTLS on submission.
 with smtplib.SMTP(host,587,timeout=20) as smtp:
  smtp.ehlo();assert not smtp.has_extn('auth')
  smtp.starttls(context=trust);smtp.ehlo();smtp.login('alice@example.test',password)
  rejected=smtp.sendmail('alice@example.test',['bob@example.test'],'From: alice@example.test\r\nTo: bob@example.test\r\nSubject: CGPanel local protocol test\r\n\r\nLocal fixture delivery only.\r\n');assert not rejected
  smtp.rset();code,_=smtp.mail('bob@example.test');
  if code<400:code,_=smtp.rcpt('alice@example.test')
  assert code>=400,'Authenticated sender spoofing was accepted';smtp.rset()
 # Implicit TLS submission and IMAP authenticate using full mailbox addresses.
 with smtplib.SMTP_SSL(host,465,timeout=20,context=trust) as smtp:
  try:smtp.login('bob@example.test','incorrect-test-password');raise AssertionError('Wrong SMTP password accepted')
  except smtplib.SMTPAuthenticationError as error:assert error.smtp_code==535
  smtp.login('bob@example.test',password)
 with smtplib.SMTP(host,25,timeout=20) as smtp:
  smtp.ehlo();smtp.mail('alice@example.test');code,_=smtp.rcpt('outside@external.invalid');assert code>=400,'Unauthenticated relay accepted'
 with imaplib.IMAP4_SSL(host,993,ssl_context=trust,timeout=20) as imap:
  try:imap.login('bob@example.test','incorrect-test-password');raise AssertionError('Wrong IMAP password accepted')
  except imaplib.IMAP4.error:pass
  assert imap.login('bob@example.test',password)[0]=='OK'
  for _ in range(60):
   imap.select('INBOX');status,data=imap.search(None,'ALL')
   if data and data[0]:break
   time.sleep(.5)
  else:raise RuntimeError('Local LMTP delivery did not reach mailbox')
  status,data=imap.fetch(data[0].split()[-1],'(RFC822)');message=email.message_from_bytes(next(v[1] for v in data if isinstance(v,tuple)))
  assert message['Subject']=='CGPanel local protocol test'
  assert message['DKIM-Signature'],'Authenticated outgoing message was not DKIM-signed'
 quota=run('doveadm','quota','get','-u','bob@example.test');assert 'STORAGE' in quota and '65536' in quota
 assert pathlib.Path('/var/lib/rspamd/dkim/example.test.key').exists()
 zone=work/'example.test.zone';zone.write_text('$ORIGIN example.test.\n$TTL 300\n@ IN SOA ns1.example.test. hostmaster.example.test. (1 3600 900 1209600 300)\n@ IN NS ns1.example.test.\nns1 IN A 192.0.2.10\n@ IN MX 10 mail.cgpanel.test.\n@ IN TXT "v=spf1 mx -all"\n_dmarc IN TXT "v=DMARC1; p=none"\n'+pathlib.Path('/var/lib/rspamd/dkim/example.test.txt').read_text());run('named-checkzone','example.test',str(zone))
 # An update setup pass must preserve enabled listeners.
 run('python3',str(ROOT/'deploy/setup-mail.py'));assert run('postconf','-h','inet_interfaces').strip()=='all'
 print('PASS: real Postfix/Dovecot/Rspamd TLS submission (587/465), IMAP 993, authenticated local LMTP delivery, DKIM signature, quota, sender-spoof/relay rejection and listener preservation')
finally:
 subprocess.run(['postfix','stop'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
 for process in processes:process.terminate()
 for process in processes:
  try:process.wait(timeout=10)
  except subprocess.TimeoutExpired:process.kill();process.wait()
 for log in logs:log.close()
