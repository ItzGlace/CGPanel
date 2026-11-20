#!/usr/bin/env python3
"""Account deletion authentication, preview matching, revocation, queue and metadata isolation."""
import json,os,secrets,socket,sqlite3,subprocess,tempfile,threading,time
from pathlib import Path
from admin_api import Client,BINARY
with tempfile.TemporaryDirectory(prefix='cgp-accounts-') as tmp:
 folder=Path(tmp);dbfile=folder/'panel.db';path=str(folder/'broker.sock');password=secrets.token_urlsafe(24);failure={'enabled':True};calls=[]
 broker=socket.socket(socket.AF_UNIX);broker.bind(path);broker.listen()
 def serve():
  while True:
   try:connection,_=broker.accept()
   except OSError:return
   with connection:
    op=json.loads(connection.makefile().readline());calls.append(op)
    if op['action']=='tenant_delete_preview':result={'ok':True,'result':{'fingerprint':'f'*64,'blockers':[],'counts':{'apps':1},'domains':[],'scope':'Local data'}}
    elif op['action']=='tenant_delete' and failure['enabled']:result={'ok':False,'error':'simulated storage busy'}
    else:result={'ok':True,'result':{'purged':True}}
    connection.sendall((json.dumps(result)+'\n').encode())
 threading.Thread(target=serve,daemon=True).start()
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 env={**os.environ,'CGPANEL_DB':str(dbfile),'CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':path}
 subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True);env.pop('CGPANEL_ADMIN_PASSWORD')
 with (folder/'log').open('w') as log:
  panel=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
  try:
   admin=Client(f'http://127.0.0.1:{port}')
   for _ in range(100):
    try:admin.call('/healthz');break
    except OSError:time.sleep(.05)
   admin.login('admin',password);uid=admin.call('/api/me')['id'];tenant='a'*32;other='b'*32;resource='c'*32
   with sqlite3.connect(dbfile) as db:
    for identity,name in [(tenant,'retired'),(other,'preserved')]:db.execute("INSERT INTO users(id,username,password,role) SELECT ?,?,password,'user' FROM users WHERE id=?",(identity,name,uid))
    db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'domains','retired.example','{}')",(resource,tenant))
    db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'apps','keep','{}')",('d'*32,other))
    db.execute("INSERT INTO analytics_events(domain_id,day,at,visitor,path,kind) VALUES(?,'2026-11-20',1,'hash','/','view')",(resource,))
   user=Client(admin.base);user.login('retired',password)
   user.call('/api/users/'+other+'/deletion',status=403);admin.call('/api/users/'+uid+'/deletion',status=403);admin.call('/api/users/'+tenant[:12]+'/deletion',status=400)
   plan=admin.call('/api/users/'+tenant+'/deletion');assert plan['username']=='retired' and 'inventory' not in plan
   body={'confirm_username':'retired','fingerprint':plan['fingerprint'],'purge_data':True}
   admin.call('/api/users/'+tenant,'DELETE',{**body,'confirm_username':'wrong'},status=400)
   admin.call('/api/users/'+tenant,'DELETE',{**body,'purge_data':False},status=400)
   admin.call('/api/users/'+tenant,'DELETE',{**body,'fingerprint':'0'*64},status=400)
   job=admin.call('/api/users/'+tenant,'DELETE',body)['id'];user.call('/api/me',status=401)
   admin.call('/api/users/'+tenant,'POST',{'enabled':True},status=400)
   for _ in range(100):
    state=next(j for j in admin.call('/api/v2/jobs') if j['id']==job)['status']
    if state=='failed':break
    time.sleep(.05)
   assert state=='failed'
   with sqlite3.connect(dbfile) as db:assert db.execute('SELECT enabled FROM users WHERE id=?',(tenant,)).fetchone()==(0,)
   admin.call('/api/v2/jobs/'+job+'/retry','POST',{},status=400)
   # Recreate an interrupted dispatcher and verify startup makes deletion reviewable.
   panel.terminate();panel.wait(timeout=10)
   with sqlite3.connect(dbfile) as db:
    db.execute("UPDATE jobs SET status='running' WHERE id=?",(job,));db.execute("UPDATE account_deletions SET state='running' WHERE user_id=?",(tenant,))
   panel=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
   for _ in range(100):
    try:admin.call('/healthz');break
    except OSError:time.sleep(.05)
   assert admin.call('/api/users/'+tenant+'/deletion')['deletion']['state']=='failed'
   failure['enabled']=False;job=admin.call('/api/users/'+tenant,'DELETE',body)['id']
   for _ in range(100):
    state=next(j for j in admin.call('/api/v2/jobs') if j['id']==job)['status']
    if state=='succeeded':break
    time.sleep(.05)
   assert state=='succeeded',(state,(folder/'log').read_text())
   with sqlite3.connect(dbfile) as db:
    assert db.execute('SELECT id FROM users WHERE id=?',(tenant,)).fetchone() is None
    assert db.execute('SELECT id FROM resources WHERE owner=?',(tenant,)).fetchone() is None
    assert db.execute('SELECT * FROM analytics_events WHERE domain_id=?',(resource,)).fetchone() is None
    assert db.execute('SELECT id FROM users WHERE id=?',(other,)).fetchone() and db.execute('SELECT id FROM resources WHERE owner=?',(other,)).fetchone()
   print('PASS: admin-only deletion, exact ID/name/preview confirmation, immediate session revocation, failed-job recovery and isolated metadata cleanup')
  finally:panel.terminate();panel.wait(timeout=10);broker.close()
