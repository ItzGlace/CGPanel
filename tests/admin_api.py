#!/usr/bin/env python3
"""Black-box auth/documentation tests using an isolated loopback panel and temporary DB.
No installed host services, real credentials, provider accounts or root access required.
Build first, then CGPANEL_TEST_BINARY=target/debug/cgpanel python3 tests/admin_api.py.
"""
import hashlib
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT=Path(__file__).resolve().parents[1]
BINARY=Path(os.getenv('CGPANEL_TEST_BINARY', str(ROOT/'target/debug/cgpanel'))).resolve()

class Client:
    def __init__(self,base,token=None):
        self.base=base;self.token=token;self.csrf=''
        self.open=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    def call(self,path,method='GET',data=None,status=200,csrf=True,headers=None):
        h={'Content-Type':'application/json'}
        if self.csrf and csrf:h['x-csrf-token']=self.csrf
        if self.token:h['Authorization']='Bearer '+self.token
        h.update(headers or {})
        request=urllib.request.Request(self.base+path,headers=h,method=method,data=None if data is None else json.dumps(data).encode())
        try:
            with self.open.open(request,timeout=10) as response:code=response.status;raw=response.read()
        except urllib.error.HTTPError as error:code=error.code;raw=error.read()
        assert code==status,(method,path,'expected',status,'got',code)
        return json.loads(raw) if raw else None
    def login(self,username,password):
        self.csrf=self.call('/api/login','POST',{'username':username,'password':password})['csrf']

def validate_contract(spec):
    assert spec['openapi']=='3.0.3'
    operation_ids=[]
    for path,methods in spec['paths'].items():
        for method,op in methods.items():
            assert method in ('get','post','delete')
            operation_ids.append(op['operationId'])
            for name in __import__('re').findall(r'{([^}]+)}',path):
                assert any(p['name']==name and p['in']=='path' and p['required'] for p in op['parameters'])
            assert '200' in op['responses']
    assert len(operation_ids)==len(set(operation_ids))==68
    def walk(value):
        if isinstance(value,dict):
            if '$ref' in value:
                node=spec
                for part in value['$ref'].removeprefix('#/').split('/'):node=node[part]
            for item in value.values():walk(item)
        elif isinstance(value,list):
            for item in value:walk(item)
    walk(spec)

def main():
    with tempfile.TemporaryDirectory(prefix='cgpanel-api-') as directory:
        temp=Path(directory);database=temp/'panel.db';log=temp/'server.log'
        with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
        password=secrets.token_urlsafe(24)
        env={**os.environ,'CGPANEL_DB':str(database),'CGPANEL_ADMIN':'admin','CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':str(temp/'missing-agent.sock')}
        subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True)
        env.pop('CGPANEL_ADMIN_PASSWORD')
        with log.open('wb') as output:
            process=subprocess.Popen([str(BINARY)],env=env,stdout=output,stderr=output)
            try:
                base=f'http://127.0.0.1:{port}'
                anon=Client(base)
                for attempt in range(100):
                    try:anon.call('/healthz');break
                    except (OSError,urllib.error.URLError):
                        if process.poll() is not None:raise AssertionError('Isolated panel exited')
                        time.sleep(.05)
                else:raise AssertionError('Isolated panel did not start')
                anon.call('/api/admin/system',status=401)
                anon.call('/api/admin/openapi.json',status=401)
                anon.call('/api/docs',status=401)
                admin=Client(base);admin.login('admin',password)
                identity=admin.call('/api/me');uid=identity['id']
                with sqlite3.connect(database) as db:
                    db.execute("INSERT INTO users(id,username,password,role) SELECT 'tenant_fixture','tenant_fixture',password,'user' FROM users WHERE id=?",(uid,))
                tenant=Client(base);tenant.login('tenant_fixture',password)
                tenant.call('/api/admin/system',status=403)
                tenant.call('/api/admin/openapi.json',status=403)
                tenant.call('/api/admin/tokens',status=403)
                tenant.call('/api/admin/tokens','POST',{'name':'denied'},status=403)
                tenant.call('/api/docs/api',status=403)
                assert len(tenant.call('/api/docs'))==5
                assert len(admin.call('/api/docs'))==8
                tenant.call('/api/v4/updates',status=403)
                for article in admin.call('/api/docs'):
                    doc=admin.call('/api/docs/'+article['slug'])
                    assert '<h1>' in doc['html'] and doc['markdown'].startswith('# ')
                    assert '<script' not in doc['html']
                spec=admin.call('/api/admin/openapi.json');validate_contract(spec)
                assert spec==json.loads((ROOT/'docs/openapi.json').read_text())
                admin.call('/api/admin/tokens','POST',{'name':'no-csrf'},status=403,csrf=False)
                for body in ({'name':''},{'name':'invalid','scope':'root'},{'name':'invalid','expires_days':91},{'name':'invalid','allowed_ips':['not-a-cidr']}):
                    admin.call('/api/admin/tokens','POST',body,status=400)
                read=admin.call('/api/admin/tokens','POST',{'name':'reporting','scope':'read','expires_days':1,'allowed_ips':['127.0.0.1/32']})
                readonly=Client(base,read['token'])
                assert readonly.call('/api/admin/system')['version']==spec['info']['version']
                assert readonly.call('/api/me')['csrf']==''
                assert 'api_token' not in readonly.call('/api/me')
                assert readonly.call('/api/users')
                readonly.call('/api/admin/system',method='HEAD')
                readonly.call('/api/v2/cron-preview','POST',{'schedule':'hourly'},status=403)
                readonly.call('/api/users/tenant_fixture','POST',{'enabled':False},status=403)
                readonly.call('/api/admin/tokens',status=403)
                full=admin.call('/api/admin/tokens','POST',{'name':'provisioning','scope':'admin','expires_days':2})
                writer=Client(base,full['token'])
                assert len(writer.call('/api/v2/cron-preview','POST',{'schedule':'0 3 * * *','timezone':'UTC'})['next'])==5
                writer.call('/api/users/tenant_fixture','POST',{'enabled':True,'quota':17,'allowed_ips':[]})
                assert next(u for u in writer.call('/api/users') if u['id']=='tenant_fixture')['quota']==17
                for path,method,body in [('/api/admin/tokens','POST',{'name':'denied'}),('/api/admin/tokens/'+read['id'],'DELETE',None),('/api/password','POST',{'current':password,'password':secrets.token_urlsafe(24)}),('/api/logout','POST',{})]:
                    writer.call(path,method,body,status=403)
                admin.call('/api/me',headers={'Authorization':'Bearer malformed'},status=401)
                assert admin.call('/api/me')['id']==uid
                iptoken=admin.call('/api/admin/tokens','POST',{'name':'wrong-ip','allowed_ips':['203.0.113.10/32']})
                Client(base,iptoken['token']).call('/api/me',status=403)
                with sqlite3.connect(database) as db:
                    rows=db.execute('SELECT token_hash,prefix FROM api_tokens').fetchall()
                    assert any(h==hashlib.sha256(read['token'].encode()).hexdigest() for h,p in rows)
                    assert all(len(h)==64 and len(p)==12 for h,p in rows)
                    db.execute('UPDATE users SET allowed_ips=? WHERE id=?',(json.dumps(['203.0.113.0/24']),uid))
                writer.call('/api/me',status=403)
                with sqlite3.connect(database) as db:db.execute("UPDATE users SET allowed_ips='[]',enabled=0 WHERE id=?",(uid,))
                writer.call('/api/me',status=401)
                with sqlite3.connect(database) as db:db.execute("UPDATE users SET enabled=1,role='user' WHERE id=?",(uid,))
                writer.call('/api/me',status=401)
                with sqlite3.connect(database) as db:db.execute("UPDATE users SET role='admin' WHERE id=?",(uid,))
                listing=admin.call('/api/admin/tokens')
                assert all('token' not in t and 'token_hash' not in t for t in listing)
                assert next(t for t in listing if t['id']==full['id'])['last_used'] is not None
                assert read['token'] not in json.dumps(listing)
                events=writer.call('/api/admin/audit?limit=2')
                following=writer.call('/api/admin/audit?limit=2&before='+str(events['next_before']))
                assert {x['id'] for x in events['events']}.isdisjoint(x['id'] for x in following['events'])
                writer.call('/api/admin/audit?limit=201',status=400)
                with sqlite3.connect(database) as db:
                    assert db.execute("SELECT count(*) FROM audit WHERE action LIKE ?",('api_token:'+full['id']+':%',)).fetchone()[0]>=2
                    db.execute('UPDATE api_tokens SET expires=unixepoch()-1 WHERE id=?',(read['id'],))
                readonly.call('/api/me',status=401)
                admin.call('/api/admin/tokens/'+full['id'],'DELETE')
                writer.call('/api/me',status=401)
                admin.call('/api/admin/tokens/'+full['id'],'DELETE')
                admin.call('/api/admin/tokens/missing','DELETE',status=404)
                survivor=admin.call('/api/admin/tokens','POST',{'name':'password-rotation'})
                new_password=secrets.token_urlsafe(24)
                admin.call('/api/password','POST',{'current':password,'password':new_password})
                Client(base,survivor['token']).call('/api/me',status=401)
                admin.call('/api/me',status=401)
                admin.login('admin',new_password)
                assert all(t['revoked'] for t in admin.call('/api/admin/tokens'))
                assert survivor['token'] not in log.read_text()
                print('PASS: isolated admin API scopes, IP rules, expiry, revocation, password rotation, CSRF, audit paging, tenant denial and bundled documentation',flush=True)
            finally:
                process.terminate()
                try:process.wait(timeout=10)
                except subprocess.TimeoutExpired:process.kill();process.wait()

if __name__=='__main__':main()
