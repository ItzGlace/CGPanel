#!/usr/bin/env python3
"""Real monitor/job queue with fixture broker; verifies both alert channels and cooldown.
This does not verify SMTP delivery or system journal coverage.
"""
import json,os,secrets,socket,sqlite3,subprocess,tempfile,time,threading
from pathlib import Path
from admin_api import Client,BINARY
with tempfile.TemporaryDirectory(prefix='cgp-alerts-') as folder:
 folder=Path(folder);dbfile=folder/'db';password=secrets.token_urlsafe(24);broker=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);broker.bind(str(folder/'broker'));broker.listen(8)
 def serve():
  while True:
   try:connection,_=broker.accept()
   except OSError:return
   with connection:
    request=json.loads(connection.makefile().readline())
    value={'samples':1500,'rejected':120,'database_auth_failures':7} if request['action']=='monitor_security' else {'queued':True}
    connection.sendall((json.dumps({'ok':True,'result':value})+'\n').encode())
 threading.Thread(target=serve,daemon=True).start()
 with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
 env={**os.environ,'CGPANEL_DB':str(dbfile),'CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_INSECURE_LOCAL':'1','CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_AGENT_SOCKET':str(folder/'broker')}
 subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True);env.pop('CGPANEL_ADMIN_PASSWORD')
 with sqlite3.connect(dbfile) as db:
  owner=db.execute('SELECT id FROM users').fetchone()[0];rid='d'*32
  db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'domains','example.invalid','{}')",(rid,owner))
  config={'scheme':'https','path':'/','interval':60,'security':True,'request_threshold':1000,'email_from':'alerts@example.invalid','email_to':'recipient@example.net','telegram_id':'fixture'}
  db.execute('INSERT INTO monitor_settings(domain_id,owner,enabled,config,analytics_key,salt) VALUES(?,?,1,?,?,?)',(rid,owner,json.dumps(config),'unused','salt'))
 with (folder/'log').open('w') as log:
  process=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
  try:
   def wait_for(predicate):
    for _ in range(100):
     with sqlite3.connect(dbfile) as db:
      if predicate(db):return
     time.sleep(.5)
    raise AssertionError('Monitor/job queue did not complete')
   wait_for(lambda db:db.execute("SELECT count(*) FROM jobs WHERE kind='monitor_email' AND status='succeeded'").fetchone()[0]>=1)
   with sqlite3.connect(dbfile) as db:db.execute('UPDATE monitor_settings SET next_check=0')
   wait_for(lambda db:db.execute('SELECT count(*) FROM monitor_checks').fetchone()[0]>=2)
   with sqlite3.connect(dbfile) as db:
    messages=[(kind,json.loads(payload),status) for kind,payload,status in db.execute("SELECT kind,payload,status FROM jobs WHERE payload LIKE '%suspected abuse%'")]
    assert len(messages)==2,messages
    assert {kind for kind,_,_ in messages}=={'monitor_email','monitor_alert'}
    email=next(payload for kind,payload,_ in messages if kind=='monitor_email')
    assert email['from']=='alerts@example.invalid' and email['to']=='recipient@example.net'
    assert db.execute('SELECT count(*) FROM security_alerts').fetchone()[0]==1
   print('PASS: monitoring queues Telegram + email with account targets; repeat signals respect one-hour cooldown')
  finally:process.terminate();process.wait(timeout=10);broker.close()
