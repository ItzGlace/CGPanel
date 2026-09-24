#!/usr/bin/env python3
"""Live integration test for a dedicated development host. Creates two test tenants.
Use CGPANEL_TEST_PASSWORD; credentials are never printed. Requires host root to test DB ACLs.
"""
import http.cookiejar, json, os, secrets, ssl, subprocess, sys, urllib.error, urllib.request

BASE=os.getenv('CGPANEL_TEST_URL','https://127.0.0.1:2083')
class Client:
    def __init__(self):
        self.jar=http.cookiejar.CookieJar()
        self.opener=urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar),urllib.request.HTTPSHandler(context=ssl._create_unverified_context()))
        self.csrf=''
    def call(self,path,method='GET',data=None,status=200,csrf=True,headers=None):
        h={'Content-Type':'application/json'}
        if csrf:h['x-csrf-token']=self.csrf
        if headers:h.update(headers)
        req=urllib.request.Request(BASE+'/api'+path,data=None if data is None else json.dumps(data).encode(),headers=h,method=method)
        try:
            with self.opener.open(req,timeout=370) as response:code=response.status;result=json.load(response)
        except urllib.error.HTTPError as e:code=e.code;result=json.load(e)
        assert code==status,(path,code,{k:v for k,v in result.items() if k!='result'})
        return result
    def login(self,name,pw):
        self.call('/login','POST',{'username':name,'password':pw});self.csrf=self.call('/me')['csrf']

def db_check(engine,credentials,other=None):
    env=os.environ.copy();pw=credentials['password'];name=credentials['database']
    if engine=='mysql':
        env['MYSQL_PWD']=pw;command=['mariadb','-u',name,'--batch','--skip-column-names','-e',f'USE {other or name}; SELECT 1;']
    else:
        env['PGPASSWORD']=pw;command=['psql',f'host=127.0.0.1 dbname={other or name} user={name} sslmode=require','-tAc','SELECT 1']
    r=subprocess.run(command,env=env,capture_output=True,text=True,timeout=15)
    assert (r.returncode!=0) if other else (r.returncode==0 and '1' in r.stdout),(engine,'database ACL check failed')

def main():
    admin=Client();admin.login(os.getenv('CGPANEL_TEST_USER','admin'),os.environ['CGPANEL_TEST_PASSWORD'])
    anon=Client();anon.call('/overview',status=401)
    admin.call('/resources/apps','POST',{},status=403,csrf=False)
    suffix=secrets.token_hex(3);password=secrets.token_urlsafe(24)
    users=[];created=[]
    try:
        for prefix in ['qa_a_','qa_b_']:
            result=admin.call('/users','POST',{'username':prefix+suffix,'password':password,'quota':10,'allowed_ips':[]});users.append(result)
        a,b=Client(),Client();a.login(users[0]['username'],password);b.login(users[1]['username'],password)
        a.call('/users',status=403)
        a.call('/resources/domains','POST',{'name':'unassigned.invalid'},status=400)
        domain=admin.call('/resources/domains','POST',{'owner':users[0]['id'],'name':suffix+'.invalid','app_id':''})['id'];created.append(domain)
        a.call('/resources/domains','POST',{'name':'evil'+suffix+'.invalid'},status=400)
        sub=a.call('/resources/domains','POST',{'name':'api.'+suffix+'.invalid','app_id':''})['id'];created.append(sub)
        dns=a.call('/resources/dns','POST',{'name':'@','domain_id':domain,'type':'TXT','value':'cgpanel-test','ttl':300})['id'];created.append(dns)
        assert b.call('/resources/domains')==[]
        b.call('/resource/'+domain,'DELETE',status=404)
        app=a.call('/resources/apps','POST',{'name':'smoke','runtime':'python','mode':'web','command':'','env':{}})['id'];created.append(app)
        result=a.call('/resource/'+app+'/terminal','POST',{'command':'id; printf "CGPANEL_ISOLATION_OK\\n"; test ! -r /etc/shadow && echo SHADOW_DENIED'})
        assert 'uid=1000' in result['output'] and 'CGPANEL_ISOLATION_OK' in result['output'] and 'SHADOW_DENIED' in result['output'],result
        b.call('/resource/'+app+'/terminal','POST',{'command':'id'},status=404)
        a.call('/resource/'+app+'/write','POST',{'path':'../escape','content':'x'},status=400)
        a.call('/resource/'+app+'/write','POST',{'path':'test.txt','content':'tenant data'})
        assert a.call('/resource/'+app+'/read','POST',{'path':'test.txt'})['output']=='tenant data'
        snapshot=a.call('/resources/backups','POST',{'name':'smoke','app_id':app})['id'];created.append(snapshot)
        a.call('/resource/'+app+'/write','POST',{'path':'test.txt','content':'changed'})
        a.call('/resource/'+snapshot+'/restore','POST',{'confirm':'RESTORE'})
        assert a.call('/resource/'+app+'/read','POST',{'path':'test.txt'})['output']=='tenant data'
        job=a.call('/resources/schedules','POST',{'name':'smoke','app_id':app,'schedule':'hourly','command':'echo scheduled'})['id'];created.append(job)
        for engine in ['mysql','postgresql']:
            db1=a.call('/resources/databases','POST',{'name':'testdb','engine':engine,'allowed_ips':[]});created.append(db1['id'])
            db2=b.call('/resources/databases','POST',{'name':'testdb','engine':engine,'allowed_ips':[]});created.append(db2['id'])
            db_check(engine,db1['result']);db_check(engine,db2['result']);db_check(engine,db1['result'],db2['result']['database'])
            a.call('/resource/'+db1['id']+'/access','POST',{'allowed_ips':['203.0.113.7']})
            a.call('/resource/'+db1['id']+'/access','POST',{'allowed_ips':['0.0.0.0/0']},status=400)
            a.call('/resource/'+db1['id']+'/access','POST',{'allowed_ips':[]})
        a.call('/resources/blocks','POST',{'name':'203.0.113.1'},status=403)
        # The client cannot spoof a trusted proxy header through Nginx.
        admin.call('/users/'+users[0]['id'],'POST',{'allowed_ips':['192.0.2.0/24'],'enabled':True,'quota':10})
        a.call('/me',status=401)
        a.call('/login','POST',{'username':users[0]['username'],'password':password},status=403,headers={'X-CGPanel-Client-IP':'192.0.2.1'})
        print('PASS: login, CSRF, tenant boundaries, domain assignments, DNS, rootless runtime, file confinement, backups/restore, schedules, MySQL/PostgreSQL ACLs, IP filters, proxy spoof protection')
    finally:
        for rid in reversed(created):
            try:admin.call('/resource/'+rid,'DELETE')
            except Exception as e:print('Cleanup required:',rid,str(e),file=sys.stderr)
        for u in users:
            try:admin.call('/users/'+u['id'],'POST',{'enabled':False,'quota':1,'allowed_ips':[]})
            except Exception:pass
        print('Test accounts were suspended; application workspace files were retained for inspection.')
if __name__=='__main__':main()
