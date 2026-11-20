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
 assert query(engine,'SELECT * FROM items')['rows']==[['1','hello']]
 query(engine,"UPDATE items SET label='changed' WHERE id=1")
 assert query(engine,'SELECT label FROM items')['rows']==[['changed']]
 query(engine,'SELECT 1; SELECT 2;',False)
 query(engine,f'SELECT * FROM {other}.items',False)
 if engine=='mysql':query(engine,"SELECT 1 INTO OUTFILE '/tmp/forbidden-sql-file'",False)
 else:query(engine,"SELECT pg_read_file('/etc/passwd')",False)
 query(engine,'DELETE FROM items WHERE id=1')
 assert query(engine,'SELECT * FROM items')['rows']==[]
 query(engine,'DROP TABLE items')
print('PASS: MariaDB/PostgreSQL Studio DDL, CRUD, single-statement checks, other-database denial and host-file denial')
zone=pathlib.Path('/tmp/example.zone');key='a'*360
zone.write_text('$ORIGIN example.com.\n$TTL 300\n@ IN SOA ns1.example.com. hostmaster.example.com. (2026112001 3600 900 1209600 300)\n@ IN NS ns1.example.com.\nns1 IN A 192.0.2.10\n@ IN A 192.0.2.10\nwww IN A 192.0.2.10\napi IN A 192.0.2.10\n@ IN MX 10 mail.example.com.\nmail IN A 192.0.2.10\n@ IN TXT "v=spf1 mx -all"\n_dmarc IN TXT "v=DMARC1; p=none"\ncgp._domainkey IN TXT "v=DKIM1; k=rsa; p='+key[:200]+'" "'+key[200:]+'"\n_acme-challenge.api IN TXT "safe_token"\n')
run('named-checkzone','example.com',str(zone))
print('PASS: BIND accepts grouped subdomains, MX, SPF, split DKIM, DMARC and child ACME record syntax')
