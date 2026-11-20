#!/usr/bin/env python3
"""Isolated tests for MCP/OAuth, tenant boundaries, login challenges and new telemetry."""
import base64,hashlib,json,os,re,secrets,socket,sqlite3,subprocess,tempfile,time,urllib.request,urllib.error
from pathlib import Path
from admin_api import Client,BINARY

def main():
 with tempfile.TemporaryDirectory(prefix='cgp-release-') as directory:
  folder=Path(directory);database=folder/'panel.db';password=secrets.token_urlsafe(24)
  with socket.socket() as sock:sock.bind(('127.0.0.1',0));port=sock.getsockname()[1]
  env={**os.environ,'CGPANEL_DB':str(database),'CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':str(folder/'none.sock'),'CGPANEL_PUBLIC_URL':'https://panel.example.com'}
  subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True);env.pop('CGPANEL_ADMIN_PASSWORD')
  with (folder/'log').open('w') as log:
   process=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
   try:
    admin=Client(f'http://127.0.0.1:{port}')
    for _ in range(100):
     try:admin.call('/healthz');break
     except OSError:time.sleep(.05)
    admin.login('admin',password);uid=admin.call('/api/me')['id'];tenant='1'*32;other='2'*32;site='a'*32;foreign='b'*32
    with sqlite3.connect(database) as db:
     for user in (tenant,other):db.execute("INSERT INTO users(id,username,password,role) SELECT ?,?,password,'user' FROM users WHERE id=?",(user,'user_'+user[:4],uid))
     for rid,owner,name in [(site,tenant,'site.example.com'),(foreign,other,'other.example.com')]:db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'domains',?,'{}')",(rid,owner,name))
     db.execute("INSERT INTO resources(id,owner,kind,name,data) VALUES(?,?,'apps','private_ai',?)",('c'*32,tenant,json.dumps({'mode':'private','runtime':'python','env':{'TOKEN':'HIDDEN_SECRET'},'port':20001})))
    user=Client(admin.base);user.login('user_1111',password)
    user.call('/api/login-security',status=403)
    credential=user.call('/api/mcp/tokens','POST',{'name':'Claude','scope':'read','expires_days':1,'allowed_ips':[]})
    token=credential['token'];assert len(token)==68
    assert token not in json.dumps(user.call('/api/mcp/tokens'))
    def rpc(method,params=None,token=token,status=200):
     body={'jsonrpc':'2.0','id':7,'method':method}
     if params is not None:body['params']=params
     return Client(admin.base,token).call('/mcp','POST',body,status=status)
    assert rpc('initialize')['result']['protocolVersion']=='2025-03-26'
    assert len(rpc('tools/list')['result']['tools'])==2
    resource=rpc('tools/call',{'name':'list_resources','arguments':{'kind':'apps'}})
    assert 'private_ai' in json.dumps(resource) and 'HIDDEN_SECRET' not in json.dumps(resource)
    assert rpc('tools/call',{'name':'website_analytics','arguments':{'id':foreign}})['result']['isError']
    assert rpc('tools/call',{'name':'restart_application','arguments':{'id':'c'*32}})['result']['isError']
    assert rpc('tools/call',{'name':'list_resources','arguments':{'kind':'users'}})['result']['isError']
    rpc('ping',token='invalid',status=401)
    user.call('/api/mcp/tokens/'+credential['id'],'DELETE');rpc('ping',status=401)
    scoped=user.call('/api/mcp/tokens','POST',{'name':'restricted','allowed_ips':['203.0.113.10'],'scope':'manage'})
    rpc('ping',token=scoped['token'],status=403)
    # Dynamic OAuth registration, PKCE enforcement and one-use authorization codes.
    anonymous=Client(admin.base)
    anonymous.call('/oauth/register','POST',{'redirect_uris':['http://evil.example/callback']},status=400)
    client=anonymous.call('/oauth/register','POST',{'redirect_uris':['https://client.example/callback']})
    verifier=secrets.token_urlsafe(48);challenge=base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip('=')
    request={'client_id':client['client_id'],'redirect_uri':'https://client.example/callback','response_type':'code','code_challenge':challenge,'code_challenge_method':'S256','scope':'read','state':'opaque'}
    user.call('/api/mcp/authorize','POST',{**request,'redirect_uri':'https://evil.example'},status=400)
    user.call('/api/mcp/authorize','POST',{**request,'scope':''})
    user.call('/api/mcp/authorize','POST',{**request,'scope':'read manage'})
    user.call('/api/mcp/authorize','POST',{**request,'scope':'root'},status=400)
    response=user.call('/api/mcp/authorize','POST',request)
    from urllib.parse import urlparse,parse_qs,urlencode
    code=parse_qs(urlparse(response['redirect']).query)['code'][0]
    def exchange(code,proof,status=200):
     data=urlencode({'client_id':client['client_id'],'redirect_uri':request['redirect_uri'],'code':code,'code_verifier':proof,'grant_type':'authorization_code'}).encode()
     req=urllib.request.Request(admin.base+'/oauth/token',data=data,headers={'Content-Type':'application/x-www-form-urlencoded'})
     try:
      with urllib.request.urlopen(req) as r:result=json.load(r);actual=r.status
     except urllib.error.HTTPError as error:actual=error.code;result={}
     assert actual==status,(actual,status)
     return result
    oauth=exchange(code,verifier);assert rpc('ping',token=oauth['access_token'])['result']=={}
    exchange(code,verifier,400)
    bad_code=parse_qs(urlparse(user.call('/api/mcp/authorize','POST',request)['redirect']).query)['code'][0]
    exchange(bad_code,'x'*43,400)
    # New pointer events; cross-origin and ownership controls remain enforced.
    user.call('/api/v2/monitor/'+site,'POST',{'scheme':'https','path':'/','analytics':True,'clicks':True})
    monitor=user.call('/api/v2/monitor/'+site);key=monitor['settings']['analytics_key']
    anonymous.call('/telemetry/collect/'+site,'POST',{'key':key,'path':'/new-page?secret=discard','kind':'move','x':900,'y':500},headers={'Origin':'https://site.example.com'},status=204)
    data=user.call('/api/v2/analytics/'+site+'?kind=move&path=/new-page');assert data['heatmap']['cells'][0]['count']==1
    user.call('/api/databases/'+foreign+'/query','POST',{'sql':'SELECT 1'},status=404)
    # Persisted panel policies and one-use/IP-bound challenges.
    admin.call('/api/login-security','POST',{'captcha':True,'attempts':3,'window_seconds':60})
    anonymous.call('/api/login','POST',{'username':'user_1111','password':password},status=400)
    challenge=anonymous.call('/api/login-challenge');numbers=[int(v) for v in re.findall(r'\d+',challenge['question'])]
    payload={'username':'user_1111','password':password,'challenge':challenge['nonce'],'answer':str(sum(numbers))}
    # Reset earlier legitimate login counts so this section tests the configured threshold independently.
    with sqlite3.connect(database) as db:db.execute('DELETE FROM login_attempts')
    anonymous.call('/api/login','POST',payload)
    anonymous.call('/api/login','POST',payload,status=400)
    anonymous.call('/api/login','POST',payload,status=400)
    anonymous.call('/api/login','POST',payload,status=429)
    print('PASS: MCP hashed credentials, scope/IP/revocation, tenant denial, OAuth PKCE/code replay, pointer telemetry, persistent login limits and one-use CAPTCHA')
   finally:process.terminate();process.wait(timeout=10)
if __name__=='__main__':main()
