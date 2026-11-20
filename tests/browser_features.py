#!/usr/bin/env python3
"""Browser integration against real panel routes and an isolated broker fixture (no host changes)."""
import json,os,secrets,socket,sqlite3,subprocess,tempfile,time,threading
from pathlib import Path
from admin_api import Client,BINARY

def main():
 with tempfile.TemporaryDirectory(prefix='cgp-browser-') as directory:
  folder=Path(directory);database=folder/'panel.db';password=secrets.token_urlsafe(24);broker_path=str(folder/'broker.sock')
  broker=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);broker.bind(broker_path);broker.listen(8)
  def serve():
   while True:
    try:connection,_=broker.accept()
    except OSError:return
    with connection:
     request=json.loads(connection.makefile().readline());action=request['action'];data=request['data'];rid=request['id']
     result={}
     if action=='integration_list':result=[]
     if action in ('protection_stats','monitor_security'):result={'samples':42,'rejected':3,'server_errors':0,'sources':[],'note':'Isolated browser fixture'}
     if action=='workspace_files':result={'content':'line\n'*200+'NEEDLE\n','revision':'fixture'}
     if action=='update_status':result={'installed':'0.6.1','config':{'enabled':True},'status':{'state':'current'}}
     if action=='mail_status':result={'settings':{'enabled':False,'hostname':'mail.example.com'},'domains':[],'mailboxes':[],'server_ip':'192.0.2.10'}
     if action=='database_query':result={'columns':['answer'],'rows':[['1']],'affected':1,'truncated':False}
     if action=='budget_status':result={'total':{'memory_mb':4096,'cpu_millis':4000,'disk_mb':8192},'allocated':{'memory_mb':512,'cpu_millis':1000,'disk_mb':1024},'free':{'memory_mb':3584,'cpu_millis':3000,'disk_mb':7168},'applications':[{'id':'c'*32,'limits':{'memory_mb':512,'cpu_millis':1000,'disk_mb':1024},'disk_enforced':True}]}
     connection.sendall((json.dumps({'ok':True,'result':result})+'\n').encode())
  threading.Thread(target=serve,daemon=True).start()
  with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  env={**os.environ,'CGPANEL_DB':str(database),'CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':broker_path}
  subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True);env.pop('CGPANEL_ADMIN_PASSWORD')
  with sqlite3.connect(database) as db:
   uid=db.execute('SELECT id FROM users').fetchone()[0]
   for rid,kind,name,data in [('a'*32,'domains','example.com',{}),('b'*32,'domains','www.example.com',{}),('c'*32,'apps','sample',{'runtime':'python','mode':'web','memory_mb':512,'cpu_millis':1000,'disk_mb':1024}),('d'*32,'databases','sample_db',{'engine':'mysql','database':'cgp_'+'d'*20,'port':3306})]:db.execute('INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,?,?,?)',(rid,uid,kind,name,json.dumps(data)))
  with (folder/'log').open('w') as log:
   process=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
   try:
    base=f'http://127.0.0.1:{port}';client=Client(base)
    for _ in range(100):
     try:client.call('/healthz');break
     except OSError:time.sleep(.05)
    client.login('admin',password)
    client.call('/api/v2/monitor/'+'a'*32,'POST',{'scheme':'https','path':'/','analytics':True,'clicks':True})
    subprocess.run(['node',str(Path(__file__).with_suffix('.cjs'))],env={**os.environ,'CGPANEL_BROWSER_URL':base,'CGPANEL_BROWSER_PASSWORD':password,'CGPANEL_SCREENSHOTS':os.environ.get('CGPANEL_SCREENSHOTS',str(folder/'screenshots'))},check=True,timeout=180)
   finally:process.terminate();process.wait(timeout=10);broker.close()
if __name__=='__main__':main()
