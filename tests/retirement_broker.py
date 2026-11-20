#!/usr/bin/env python3
"""Real privileged broker retirement across files, SQL, DNS, Nginx and registry.
Systemd, Podman, rndc and nft operations use adapters inside a disposable container.
"""
import json,os,pathlib,shutil,socket,sqlite3,subprocess,time
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1'
source=pathlib.Path('/project');tenant='a'*32;other='b'*32;admin='e'*32
app_ids=['3'*32,'4'*32];domains=['c'*32,'d'*32];db_ids=['1'*32,'2'*32,'5'*32,'6'*32]
def run(*args,input=None):
 p=subprocess.run(args,input=input,text=True,capture_output=True,timeout=30);assert p.returncode==0,(args,p.stdout[-1500:],p.stderr[-1500:]);return p.stdout
for folder in ['/etc/cgpanel','/var/lib/cgpanel','/var/lib/cgpanel-agent','/usr/local/lib/cgpanel','/run/cgpanel','/run/sshd','/etc/bind/cgpanel','/run/mysqld','/etc/nginx/cgpanel-ide']:pathlib.Path(folder).mkdir(parents=True,exist_ok=True)
run('chown','mysql:mysql','/run/mysqld')
mysql=subprocess.Popen(['mariadbd','--user=mysql','--socket=/run/mysqld/mysqld.sock','--skip-networking'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
for _ in range(100):
 if subprocess.run(['mariadb-admin','ping'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0:break
 time.sleep(.1)
run('pg_ctlcluster','16','main','start');run('useradd','--system','--home','/var/lib/cgpanel','cgpanel');run('ssh-keygen','-A')
for name in ['systemctl','rndc','nft','loginctl']:
 p=pathlib.Path('/usr/local/bin')/name;p.write_text('#!/bin/sh\n[ "$1" != "-f" ] || cat >/dev/null\nexit 0\n');p.chmod(0o755)
p=pathlib.Path('/usr/local/bin/podman');p.write_text('#!/bin/sh\nif [ "$1" = info ]; then printf "%s/.local/share/containers\\n" "$HOME"; fi\nexit 0\n');p.chmod(0o755)
for name in ['account-purge.py']:shutil.copy2(source/'deploy'/name,pathlib.Path('/usr/local/lib/cgpanel')/name)
pathlib.Path('/etc/nginx/conf.d/00-fixture.conf').write_text('limit_req_zone $binary_remote_addr zone=cgp_web:10m rate=20r/s; limit_conn_zone $binary_remote_addr zone=cgp_conn:10m; map $http_upgrade $cg_connection { default upgrade; "" close; }')
pathlib.Path('/etc/bind/cgpanel/zones.conf').touch()
with pathlib.Path('/etc/bind/named.conf.local').open('a') as f:f.write('\ninclude "/etc/bind/cgpanel/zones.conf";\n')
items={}
with sqlite3.connect('/var/lib/cgpanel/panel.db') as db:
 db.executescript((source/'src/schema.sql').read_text())
 for uid,name,role in [(tenant,'retired','user'),(other,'preserved','user'),(admin,'admin','admin')]:db.execute('INSERT INTO users(id,username,password,role,enabled) VALUES(?,?,?,?,?)',(uid,name,'dummy',role,int(uid!=tenant)))
 for uid,app,domain,name in zip([tenant,other],app_ids,domains,['retired.example','preserved.example']):
  home=pathlib.Path('/srv/cgpanel/tenants')/uid;home.mkdir(parents=True);run('useradd','-M','-U','-d',str(home),'cg_'+uid[:20])
  (home/'apps'/app).mkdir(parents=True);(home/'apps'/app/'site.txt').write_text(name)
  mail=pathlib.Path('/var/lib/cgpanel-mail')/uid;mail.mkdir(parents=True);(mail/'message').write_text(name)
  for rid,kind,data in [(app,'apps',{'name':'site','mode':'worker','memory_mb':512,'cpu_millis':1000,'disk_mb':1024}),(domain,'domains',{'name':name})]:
   items[rid]={'tenant':uid,'kind':kind,'data':data};db.execute('INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,?,?,?)',(rid,uid,kind,data['name'],json.dumps(data)))
 for uid,rid,engine in [(tenant,db_ids[0],'mysql'),(tenant,db_ids[1],'postgresql'),(other,db_ids[2],'mysql'),(other,db_ids[3],'postgresql')]:
  name='cgp_'+rid[:20]
  if engine=='mysql':run('mariadb',input=f"CREATE DATABASE {name}; CREATE USER '{name}'@'localhost' IDENTIFIED BY 'fixture'; GRANT ALL ON {name}.* TO '{name}'@'localhost';")
  else:run('runuser','-u','postgres','--','psql','-X','-v','ON_ERROR_STOP=1',input=f'CREATE ROLE {name} LOGIN; CREATE DATABASE {name} OWNER {name};')
  data={'name':name,'engine':engine,'allowed_ips':[],'password':'fixture'};items[rid]={'tenant':uid,'kind':'databases','data':data};db.execute('INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,\'databases\',?,?)',(rid,uid,name,json.dumps(data)))
registry=pathlib.Path('/var/lib/cgpanel-agent/registry.json');registry.write_text(json.dumps({'items':items,'ports':20000}))
sock='/run/cgpanel/agent.sock';log=pathlib.Path('/tmp/broker.log').open('w')
def start():
 process=subprocess.Popen([os.environ.get('CGPANEL_BROKER_BINARY','/project/target/ubuntu-release/release/cgpanel-agent')],env={**os.environ,'CGPANEL_PUBLIC_IP':'192.0.2.10','CGPANEL_AGENT_SOCKET':sock},stdout=log,stderr=log)
 for _ in range(150):
  if process.poll() is not None:raise AssertionError(pathlib.Path('/tmp/broker.log').read_text())
  if pathlib.Path(sock).exists():return process
  time.sleep(.1)
 raise AssertionError('Broker did not become ready')
def call(action,uid=tenant,**data):
 with socket.socket(socket.AF_UNIX) as s:
  s.settimeout(90);s.connect(sock);s.sendall((json.dumps({'action':action,'tenant':uid,'id':uid,'data':data})+'\n').encode());return json.loads(s.makefile().readline())
process=start()
try:
 assert not call('tenant_delete_preview',administrator=False)['ok']
 preview=call('tenant_delete_preview',administrator=True);assert preview['ok'],preview
 assert not preview['result']['blockers'] and preview['result']['counts']['databases']==2,preview
 result=call('tenant_delete',administrator=True,fingerprint=preview['result']['fingerprint']);assert result=={'ok':True,'result':{'purged':True}},(result,pathlib.Path('/tmp/broker.log').read_text())
 state=json.loads(registry.read_text());marker=state['items']['retire_'+tenant];assert marker['data']['state']=='completed' and 'plan' not in marker['data']
 assert all(i['tenant']!=tenant or i['kind']=='retiring_tenant' for i in state['items'].values())
 assert not pathlib.Path('/srv/cgpanel/tenants',tenant).exists() and pathlib.Path('/srv/cgpanel/tenants',other,'apps',app_ids[1],'site.txt').read_text()=='preserved.example'
 assert not pathlib.Path('/var/lib/cgpanel-mail',tenant).exists() and pathlib.Path('/var/lib/cgpanel-mail',other,'message').exists()
 assert not pathlib.Path('/etc/nginx/conf.d/cgp_'+domains[0]+'.conf').exists() and pathlib.Path('/etc/nginx/conf.d/cgp_'+domains[1]+'.conf').exists()
 assert 'retired.example' not in pathlib.Path('/etc/bind/cgpanel/zones.conf').read_text();run('named-checkconf');run('nginx','-t')
 names=run('mariadb','--batch','--skip-column-names',input='SHOW DATABASES;');assert 'cgp_'+db_ids[0][:20] not in names and 'cgp_'+db_ids[2][:20] in names
 names=run('runuser','-u','postgres','--','psql','-X','-t','-A',input='SELECT datname FROM pg_database;');assert 'cgp_'+db_ids[1][:20] not in names and 'cgp_'+db_ids[3][:20] in names
 process.terminate();process.wait(timeout=5);pathlib.Path(sock).unlink(missing_ok=True);process=start()
 assert call('tenant_delete_preview',administrator=True)['result']['completed']
 assert call('tenant_delete',administrator=True,fingerprint=preview['result']['fingerprint'])['result']['purged']
 assert call('budget_status',uid=other)['ok']
 print('PASS: actual broker retirement RPC, native SQL drops and foreign DB preservation, account/files/mail removal, valid remaining Nginx/DNS, restart recovery and idempotent completion; kernel/service operations adapted')
finally:process.terminate();process.wait(timeout=5);mysql.terminate();mysql.wait(timeout=5);log.close()
