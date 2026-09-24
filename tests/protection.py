#!/usr/bin/env python3
"""Isolated local challenge lifecycle and asset route regression tests."""
import json,os,secrets,socket,sqlite3,subprocess,tempfile,time,re,urllib.request,urllib.error
from pathlib import Path
from admin_api import Client,BINARY
with tempfile.TemporaryDirectory(prefix='cgp-guard-') as folder:
 folder=Path(folder);dbfile=folder/'db';password=secrets.token_urlsafe(24)
 with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
 env={**os.environ,'CGPANEL_DB':str(dbfile),'CGPANEL_ADMIN':'admin','CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':str(folder/'none')}
 subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True)
 env.pop('CGPANEL_ADMIN_PASSWORD')
 with (folder/'log').open('w') as log:
  proc=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
  try:
   c=Client(f'http://127.0.0.1:{port}')
   for _ in range(100):
    try:c.call('/healthz');break
    except OSError:time.sleep(.05)
   c.login('admin',password);owner=c.call('/api/me')['id'];rid='b'*32
   with sqlite3.connect(dbfile) as db:
    db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'domains','test.example','{}')",(rid,owner))
    db.execute('INSERT INTO website_protection(domain_id,config) VALUES(?,?)',(rid,json.dumps({'captcha':'local'})))
   class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self,*args,**kwargs):return None
   client=urllib.request.build_opener(NoRedirect())
   def request(path,data=None,extra=None):
    headers={'Host':'test.example','X-CGPanel-Client-IP':'203.0.113.20',**(extra or {})}
    req=urllib.request.Request(c.base+path,data=data,headers=headers)
    try:response=client.open(req,timeout=5)
    except urllib.error.HTTPError as e:response=e
    with response:return response.status,response.read().decode(),response.headers
   for asset in ['/app.js','/mail.js','/nav-icons.js','/workspace.js']:
    code,body,headers=request(asset);assert code==200 and 'javascript' in headers['Content-Type'],asset
   code,body,_=request('/');assert body.count('name="code"')==1 and 'method="post"' in body
   check=f'/guard/{rid}/verify';challenge=f'/guard/{rid}/challenge'
   assert request(check)[0]==401
   code,body,_=request(challenge);assert code==200
   a,b=map(int,re.search(r'What is (\d+) \+ (\d+)\?',body).groups());nonce=re.search(r'name="nonce" value="([a-f0-9]+)"',body)[1]
   form=f'nonce={nonce}&answer={a+b}'.encode();headers={'Origin':'https://test.example','Content-Type':'application/x-www-form-urlencoded','X-Forwarded-Proto':'https'}
   assert request(challenge,form,{**headers,'Origin':'https://foreign.example'})[0]==403
   code,_,result=request(challenge,form,headers);assert code==303,code
   cookie=result['Set-Cookie'];assert 'Secure' in cookie and 'HttpOnly' in cookie
   cookie=cookie.split(';')[0]
   assert request(check,extra={'Cookie':cookie})[0]==204
   assert request(check,extra={'Cookie':cookie,'X-CGPanel-Client-IP':'203.0.113.21'})[0]==401
   assert request(challenge,form,headers)[0]==400
   assert request(check,extra={'Cookie':cookie,'Host':'foreign.example'})[0]==403
   with sqlite3.connect(dbfile) as db:
    db.execute('UPDATE website_protection SET config=? WHERE domain_id=?',(json.dumps({'captcha':'local','sitemap':True,'sitemap_auto':True,'sitemap_paths':['/']}),rid))
    db.execute("INSERT INTO analytics_events(domain_id,day,at,visitor,path,kind) VALUES(?,date('now'),unixepoch(),'test','/observed','view')",(rid,))
   code,body,headers=request(f'/guard/{rid}/sitemap');assert code==200 and 'https://test.example/observed</loc>' in body and headers['Content-Type'].startswith('application/xml')
   print('PASS: JS asset routes, login fallback, challenge origin/IP/domain binding, single use and secure cookie')
  finally:proc.terminate();proc.wait(timeout=10)
