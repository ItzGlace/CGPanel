#!/usr/bin/env python3
"""Disposable Ubuntu container only: real database-role isolation and DNS syntax.
Run with docker run --rm -v "$PWD:/project:ro" cgpanel-integration python3 tests/host_tools.py.
Never run against an installed hosting server: this test initializes local SQL fixtures.
"""
import json,os,pathlib,secrets,subprocess,time
ROOT=pathlib.Path(__file__).resolve().parents[1]
assert os.environ.get('CGPANEL_DISPOSABLE_TEST')=='1','Set CGPANEL_DISPOSABLE_TEST=1 only in a disposable container'
def run(*args,input=None,ok=True):
 p=subprocess.run(args,input=input,text=True,capture_output=True,timeout=40)
 assert (p.returncode==0)==ok,(args,p.stdout[-500:],p.stderr[-500:])
 return p
subprocess.Popen(['mariadbd','--user=mysql','--socket=/run/mysqld/mysqld.sock','--skip-networking'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
for _ in range(100):
 if subprocess.run(['mariadb-admin','ping'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0:break
 time.sleep(.1)
else:raise AssertionError('MariaDB did not start')
hba=pathlib.Path('/etc/postgresql/16/main/pg_hba.conf');hba.write_text('local all postgres peer\nlocal all all scram-sha-256\n')
run('pg_ctlcluster','16','main','start')
name='cgp_'+'a'*20;other='cgp_'+'b'*20;password=secrets.token_hex(24)
run('mariadb','-uroot',input=f"CREATE DATABASE {name}; CREATE USER '{name}'@'localhost' IDENTIFIED BY '{password}'; GRANT ALL ON {name}.* TO '{name}'@'localhost'; CREATE DATABASE {other};")
run('runuser','-u','postgres','--','psql','-v','ON_ERROR_STOP=1',input=f"CREATE ROLE {name} LOGIN PASSWORD '{password}'; CREATE DATABASE {name} OWNER {name}; CREATE DATABASE {other};")
def query(engine,sql,ok=True):
 p=run('python3',str(ROOT/'deploy/database-query.py'),input=json.dumps({'engine':engine,'database':name,'password':password,'sql':sql}),ok=ok)
 return json.loads(p.stdout) if ok else p
for engine in ['mysql','postgresql']:
 query(engine,'CREATE TABLE items(id INTEGER PRIMARY KEY, label VARCHAR(100));')
 query(engine,"INSERT INTO items VALUES(1,'hello');")
 selected=query(engine,'SELECT * FROM items');assert selected['rows']==[['1','hello']] and selected['affected']==1
 query(engine,"UPDATE items SET label='changed' WHERE id=1")
 assert query(engine,'SELECT label FROM items')['rows']==[['changed']]
 query(engine,'SELECT 1; SELECT 2;',False)
 query(engine,f'SELECT * FROM {other}.items',False)
 if engine=='mysql':query(engine,"SELECT 1 INTO OUTFILE '/tmp/forbidden-sql-file'",False)
 else:query(engine,"SELECT pg_read_file('/etc/passwd')",False)
 query(engine,'DELETE FROM items WHERE id=1')
 assert query(engine,'SELECT * FROM items')['rows']==[]
 query(engine,'DROP TABLE items')
 def studio(action,ok=True,**fields):
  p=run('python3',str(ROOT/'deploy/database-query.py'),input=json.dumps({'engine':engine,'database':name,'password':password,'action':action,**fields}),ok=ok)
  return json.loads(p.stdout) if ok else p
 schema=name if engine=='mysql' else 'public';table='workspace_items'
 studio('create_table',schema=schema,table=table,columns=[{'name':'id','type':'integer','primary':True,'auto':True},{'name':'label','type':'text','nullable':True}])
 assert any(t['name']==table for t in studio('catalog')['tables'])
 studio('insert',schema=schema,table=table,values={'label':'hello'})
 page=studio('table',schema=schema,table=table);assert page['editable'] and page['primary_key']==['id'];row=page['rows'][0];assert row['values']==['1','hello']
 studio('update',schema=schema,table=table,key={'id':'1'},token=row['token'],values={'label':'updated'})
 stale=studio('update',ok=False,schema=schema,table=table,key={'id':'1'},token=row['token'],values={'label':'overwrite'});assert 'changed since' in stale.stderr
 studio('delete',ok=False,schema=schema,table=table,key={},token=row['token'])
 current=studio('table',schema=schema,table=table)['rows'][0];assert current['values'][1]=='updated'
 studio('alter_table',schema=schema,table=table,change='add',column='note',type='varchar')
 studio('alter_table',schema=schema,table=table,change='rename',column='note',new_name='notes')
 studio('alter_table',ok=False,schema=schema,table=table,change='drop',column='notes',confirm='wrong')
 studio('alter_table',schema=schema,table=table,change='drop',column='notes',confirm='notes')
 current=studio('table',schema=schema,table=table)['rows'][0]
 studio('delete',schema=schema,table=table,key={'id':'1'},token=current['token'])
 assert studio('table',schema=schema,table=table)['rows']==[]
 for i in range(55):query(engine,"INSERT INTO workspace_items(label) VALUES('page');")
 first=studio('table',schema=schema,table=table,limit=50);second=studio('table',schema=schema,table=table,limit=50,offset=50)
 assert len(first['rows'])==50 and first['has_more'] and len(second['rows'])==5 and not second['has_more']
 studio('table',ok=False,schema=other if engine=='mysql' else 'pg_catalog',table=table)
 studio('drop_table',ok=False,schema=schema,table=table,confirm='wrong')
 size=studio('size');assert isinstance(size['bytes'],int) and size['bytes']>0 and size['table_count']>=1,size
 catalog=studio('catalog');assert catalog['bytes']>0 and all(isinstance(t['bytes'],int) for t in catalog['tables'])
 studio('truncate',ok=False,schema=schema,table=table,confirm='wrong')
 assert len(studio('table',schema=schema,table=table)['rows'])==50
 query(engine,'CREATE TABLE dependent(id INTEGER PRIMARY KEY, parent_id INTEGER, FOREIGN KEY(parent_id) REFERENCES workspace_items(id))')
 query(engine,'INSERT INTO dependent VALUES(1,2)')
 studio('truncate',ok=False,schema=schema,table=table,confirm=table)
 assert query(engine,'SELECT count(*) FROM dependent')['rows']==[['1']]
 query(engine,'DROP TABLE dependent')
 studio('truncate',schema=schema,table=table,confirm=table)
 assert studio('table',schema=schema,table=table)['rows']==[]
 assert len(studio('table',schema=schema,table=table)['columns'])==2
 studio('drop_table',schema=schema,table=table,confirm=table)
 odd='quoted`"table';studio('create_table',schema=schema,table=odd,columns=[{'name':'a','type':'integer','primary':True},{'name':'b','type':'integer','primary':True}])
 studio('insert',schema=schema,table=odd,values={'a':1,'b':1});studio('insert',schema=schema,table=odd,values={'a':1,'b':2})
 one=studio('table',schema=schema,table=odd)['rows'][0]
 studio('delete',ok=False,schema=schema,table=odd,key={'a':1},token=one['token'])
 studio('delete',schema=schema,table=odd,key={'a':1,'b':1},token=one['token']);assert len(studio('table',schema=schema,table=odd)['rows'])==1
 studio('drop_table',schema=schema,table=odd,confirm=odd)
 query(engine,'CREATE TABLE big_rows(id INTEGER PRIMARY KEY,label '+('MEDIUMTEXT' if engine=='mysql' else 'TEXT')+')')
 query(engine,"INSERT INTO big_rows VALUES "+','.join("("+str(i)+",REPEAT('x',70000))" for i in range(1,31)))
 first=studio('table',schema=schema,table='big_rows');assert first['bytes_limited'] and 0<len(first['rows'])<30
 second=studio('table',schema=schema,table='big_rows',offset=len(first['rows']))
 assert int(second['rows'][0]['values'][0])==len(first['rows'])+1
 studio('drop_table',schema=schema,table='big_rows',confirm='big_rows')
 if engine=='postgresql':
  query(engine,'CREATE MATERIALIZED VIEW materialized_items AS SELECT 1 AS id')
  assert any(t['type']=='MATERIALIZED VIEW' for t in studio('catalog')['tables'])
  studio('drop_table',schema=schema,table='materialized_items',confirm='materialized_items')

print('PASS: MariaDB/PostgreSQL Studio DDL, CRUD, single-statement checks, other-database denial and host-file denial')
print('PASS: navigable catalog/schema/pages, structured row editing, stale-edit rejection, composite keys, quoted identifiers and column controls')
zone=pathlib.Path('/tmp/example.zone');key='a'*360
zone.write_text('$ORIGIN example.com.\n$TTL 300\n@ IN SOA ns1.example.com. hostmaster.example.com. (2026112001 3600 900 1209600 300)\n@ IN NS ns1.example.com.\nns1 IN A 192.0.2.10\n@ IN A 192.0.2.10\nwww IN A 192.0.2.10\napi IN A 192.0.2.10\n@ IN MX 10 mail.example.com.\nmail IN A 192.0.2.10\n@ IN TXT "v=spf1 mx -all"\n_dmarc IN TXT "v=DMARC1; p=none"\ncgp._domainkey IN TXT "v=DKIM1; k=rsa; p='+key[:200]+'" "'+key[200:]+'"\n_acme-challenge.api IN TXT "safe_token"\n')
run('named-checkzone','example.com',str(zone))
print('PASS: BIND accepts grouped subdomains, MX, SPF, split DKIM, DMARC and child ACME record syntax')
