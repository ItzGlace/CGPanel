#!/usr/bin/env python3
"""Dedicated-host integration tests. Uses disposable tenants and local protocol fixtures.
Run as root with CGPANEL_TEST_PASSWORD. No third-party credentials are needed.
Fixture listeners are on high ports; keep the host firewall enabled.
"""
import asyncio, hashlib, hmac, http.server, io, json, os, pathlib, secrets, socket
import ssl, sqlite3, subprocess, threading, time, urllib.parse, urllib.request, zipfile
from smoke import Client, BASE

IP=os.environ['CGPANEL_PUBLIC_IP']
ROOT=pathlib.Path('/var/tmp/cgpanel-v2-tests')
ROOT.mkdir(mode=0o700,exist_ok=True)
SOCKS_PORT=21880
S3_PORT=21881
ACCESS='cgpanel-fixture-access'
SECRET=secrets.token_hex(24)
objects={}; uploads={}; requests=[]; attempts={}; errors=[]

def run(args,**kwargs):
    return subprocess.run(args,check=True,capture_output=True,text=True,**kwargs).stdout

class S3(http.server.BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def respond(self,status,body=b'',headers=None):
        self.send_response(status)
        for k,v in (headers or {}).items():self.send_header(k,v)
        self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def process(self):
        body=self.rfile.read(int(self.headers.get('Content-Length','0')))
        url=urllib.parse.urlsplit(self.path);query=urllib.parse.parse_qs(url.query,keep_blank_values=True)
        amz=self.headers['x-amz-date'];payload=hashlib.sha256(body).hexdigest()
        canonical_query=urllib.parse.urlencode(sorted(urllib.parse.parse_qsl(url.query,keep_blank_values=True)),quote_via=urllib.parse.quote,safe='~')
        signed='host;x-amz-content-sha256;x-amz-date'
        canonical=f'{self.command}\n{url.path}\n{canonical_query}\nhost:{self.headers["Host"]}\nx-amz-content-sha256:{payload}\nx-amz-date:{amz}\n\n{signed}\n{payload}'
        scope=f'{amz[:8]}/us-east-1/s3/aws4_request'
        key=('AWS4'+SECRET).encode()
        for word in [amz[:8],'us-east-1','s3','aws4_request']:key=hmac.new(key,word.encode(),hashlib.sha256).digest()
        signature=hmac.new(key,f'AWS4-HMAC-SHA256\n{amz}\n{scope}\n{hashlib.sha256(canonical.encode()).hexdigest()}'.encode(),hashlib.sha256).hexdigest()
        expected=f'AWS4-HMAC-SHA256 Credential={ACCESS}/{scope}, SignedHeaders={signed}, Signature={signature}'
        if not hmac.compare_digest(expected,self.headers.get('Authorization','')):
            errors.append('Invalid AWS signature');return self.respond(403)
        if payload!=self.headers['x-amz-content-sha256']:return self.respond(400)
        if self.command=='POST' and 'uploads' in query:
            uid=secrets.token_hex(8);uploads[uid]={};return self.respond(200,f'<InitiateMultipartUploadResult><UploadId>{uid}</UploadId></InitiateMultipartUploadResult>'.encode())
        uid=query.get('uploadId',[''])[0]
        if self.command=='PUT':
            part=int(query['partNumber'][0]);key=(uid,part);attempts[key]=attempts.get(key,0)+1
            if attempts[key]==1:return self.respond(503,b'Retry fixture')
            uploads[uid][part]=body;return self.respond(200,headers={'ETag':'"'+hashlib.md5(body).hexdigest()+'"'})
        if self.command=='POST':
            objects[url.path]=b''.join(v for _,v in sorted(uploads.pop(uid).items()))
            return self.respond(200,b'<CompleteMultipartUploadResult><ETag>"complete"</ETag></CompleteMultipartUploadResult>')
        if self.command=='DELETE':uploads.pop(uid,None);return self.respond(204)
        self.respond(400)
    do_POST=do_PUT=do_DELETE=process

async def socks(reader,writer):
    try:
        version,n=await reader.readexactly(2);methods=await reader.readexactly(n)
        assert version==5 and 2 in methods
        writer.write(b'\x05\x02');await writer.drain()
        version,n=await reader.readexactly(2);user=await reader.readexactly(n)
        n=(await reader.readexactly(1))[0];password=await reader.readexactly(n)
        assert version==1 and user==b'fixture' and password==b'fixture-password'
        writer.write(b'\x01\x00');await writer.drain()
        header=await reader.readexactly(4);assert header[:3]==b'\x05\x01\x00'
        if header[3]==1:host=socket.inet_ntoa(await reader.readexactly(4))
        elif header[3]==3:host=(await reader.readexactly((await reader.readexactly(1))[0])).decode()
        else:raise ValueError('Unexpected address family')
        port=int.from_bytes(await reader.readexactly(2),'big');requests.append((host,port))
        writer.write(b'\x05\x00\x00\x01'+socket.inet_aton(IP)+b'\x00\x00');await writer.drain()
        if port==53:
            length=int.from_bytes(await reader.readexactly(2),'big');packet=await reader.readexactly(length)
            # Controlled DNS answer routes arbitrary test hostnames through this fixture.
            reply=packet[:2]+b'\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00'+packet[12:]+b'\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04'+socket.inet_aton('1.1.1.1')
            writer.write(len(reply).to_bytes(2,'big')+reply);await writer.drain();return
        if host=='1.1.1.1' and port==80:
            await reader.readuntil(b'\r\n\r\n');body=b'{"ip":"198.51.100.42","fixture":true}'
            writer.write(b'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: '+str(len(body)).encode()+b'\r\n\r\n'+body);await writer.drain();return
        assert host==IP and port in [S3_PORT,22],(host,port)
        remote_r,remote_w=await asyncio.open_connection(host,port)
        async def relay(r,w):
            try:
                while data:=await r.read(65536):w.write(data);await w.drain()
            finally:w.close()
        await asyncio.gather(relay(reader,remote_w),relay(remote_r,writer))
    except (asyncio.IncompleteReadError,ConnectionError):pass
    except Exception as e:errors.append(str(e))
    finally:writer.close()

def fixtures():
    cert=ROOT/'s3.crt';key=ROOT/'s3.key'
    ca=ROOT/'ca.crt';cakey=ROOT/'ca.key';csr=ROOT/'s3.csr';extensions=ROOT/'s3.ext'
    run(['openssl','req','-x509','-newkey','rsa:2048','-nodes','-days','1','-keyout',str(cakey),'-out',str(ca),'-subj','/CN=CGPanel fixture CA','-addext','basicConstraints=critical,CA:TRUE'])
    run(['openssl','req','-newkey','rsa:2048','-nodes','-keyout',str(key),'-out',str(csr),'-subj','/CN=CGPanel test fixture'])
    extensions.write_text(f'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:{IP}\n')
    run(['openssl','x509','-req','-in',str(csr),'-CA',str(ca),'-CAkey',str(cakey),'-CAcreateserial','-days','1','-out',str(cert),'-extfile',str(extensions)])
    server=http.server.ThreadingHTTPServer(('0.0.0.0',S3_PORT),S3)
    context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER);context.load_cert_chain(cert,key);server.socket=context.wrap_socket(server.socket,server_side=True)
    threading.Thread(target=server.serve_forever,daemon=True).start()
    loop=asyncio.new_event_loop();threading.Thread(target=loop.run_forever,daemon=True).start()
    proxy=asyncio.run_coroutine_threadsafe(asyncio.start_server(socks,'0.0.0.0',SOCKS_PORT),loop).result()
    return server,loop,proxy,ca.read_text()

def wait_job(client,rid,expected='succeeded',timeout=360):
    until=time.monotonic()+timeout
    while time.monotonic()<until:
        job=next(j for j in client.call('/v2/jobs') if j['id']==rid)
        if job['status'] not in ['queued','running']:
            assert job['status']==expected,(job['kind'],job['status'],job['error']);return job
        time.sleep(2)
    raise AssertionError('Job timeout: '+rid)

def job(client,kind,target,expected='succeeded',**data):
    rid=client.call('/v2/jobs','POST',{'kind':kind,'target':target,**data})['id']
    return wait_job(client,rid,expected)

def sql(db,statement):
    c=db['result'];env=os.environ.copy()
    if db['engine']=='mysql':env['MYSQL_PWD']=c['password'];args=['mariadb','-u',c['database'],c['database'],'--batch','--skip-column-names','-e',statement]
    else:env['PGPASSWORD']=c['password'];args=['psql','-h','127.0.0.1','-U',c['database'],'-d',c['database'],'-tAc',statement]
    return run(args,env=env)

def main():
    server,loop,proxy,ca=fixtures()
    admin=Client();admin.login('admin',os.environ['CGPANEL_TEST_PASSWORD'])
    suffix=secrets.token_hex(3);password=secrets.token_urlsafe(24);users=[];resources=[];integrations=[];plans=[]
    hosts=pathlib.Path('/etc/hosts');original_hosts=hosts.read_text();domain_name='v2-'+suffix+'.invalid'
    ssh_user='cgbqa_'+suffix;ssh_home=pathlib.Path('/home')/ssh_user
    try:
        for prefix in ['v2a_','v2b_']:
            users.append(admin.call('/users','POST',{'username':prefix+suffix,'password':password,'quota':30,'allowed_ips':[]}))
        a,b=Client(),Client();a.login(users[0]['username'],password);b.login(users[1]['username'],password)
        app=a.call('/resources/apps','POST',{'name':'v2fixture','runtime':'php','mode':'web','command':'','env':{'FIXTURE_ENV':'retained'}})['id'];resources.append(app)
        domain=admin.call('/resources/domains','POST',{'owner':users[0]['id'],'name':domain_name,'app_id':app})['id'];resources.append(domain)
        hosts.write_text(original_hosts+f'\n{IP} {domain_name}\n')
        dns=a.call('/resources/dns','POST',{'domain_id':domain,'name':'@','type':'TXT','value':'v2-fixture','ttl':300})['id'];resources.append(dns)
        a.call('/resource/'+app+'/write','POST',{'path':'index.php','content':'<!doctype html><html lang="en"><head><title>CGPanel monitoring fixture</title><meta name="description" content="A controlled website for release verification"><meta name="viewport" content="width=device-width,initial-scale=1"><meta property="og:title" content="Fixture"><link rel="canonical" href="http://'+domain_name+'/"></head><body><h1>Fixture website</h1><button data-cgp-label="signup">Sign up</button></body></html>'})
        cfg={'enabled':True,'scheme':'http','path':'/','interval':60,'analytics':True,'clicks':True,'retention_days':7,'telegram_id':''}
        a.call('/v2/monitor/'+domain,'POST',cfg);b.call('/v2/monitor/'+domain,status=404)
        monitor=a.call('/v2/monitor/'+domain);key=monitor['settings']['analytics_key']
        def event(ip,kind='view',origin=None,dnt=False):
            body={'key':key,'kind':kind,'path':'/pricing?secret=discarded#fragment','x':760,'y':180,'viewport':'desktop','target':'signup','referrer':'example.com'}
            request=urllib.request.Request('http://127.0.0.1:2082/telemetry/collect/'+domain,data=json.dumps(body).encode(),headers={'Content-Type':'application/json','Origin':origin or 'http://'+domain_name,'X-CGPanel-Client-IP':ip,'DNT':'1' if dnt else '0'})
            return urllib.request.urlopen(request).status
        for ip in ['1.1.1.1','1.1.1.1','8.8.8.8']:assert event(ip)==204
        assert event('1.1.1.1','click')==204
        assert event('9.9.9.9',dnt=True)==204
        try:event('1.1.1.1',origin='https://wrong.invalid');raise AssertionError('Foreign origin accepted')
        except urllib.error.HTTPError as e:assert e.code==403
        data=a.call('/v2/analytics/'+domain+'?path=%2Fpricing');assert data['daily'][0]['views']==3 and data['daily'][0]['unique_ips']==2,data
        assert data['heatmap']['cells']==[{'col':7,'row':3,'count':1}]
        assert data['pages'][0]['path']=='/pricing'
        seo=job(a,'seo',domain,scheme='http')['result'];assert seo['http_status']==200 and seo['checks'][0]['detail']=='CGPanel monitoring fixture'
        def check_once(path):
            previous=a.call('/v2/monitor/'+domain)['checks']
            at=previous[0]['at'] if previous else 0
            time.sleep(1)
            a.call('/v2/monitor/'+domain,'POST',{**cfg,'path':path})
            until=time.monotonic()+30
            while time.monotonic()<until:
                state=a.call('/v2/monitor/'+domain)
                if state['checks'] and state['checks'][0]['at']>at:return state
                time.sleep(2)
            raise AssertionError('Monitor failed to collect a due check')
        assert check_once('/')['settings']['state']=='up'
        a.call('/resource/'+app+'/write','POST',{'path':'unavailable.php','content':'<?php http_response_code(503); echo "fixture outage";'})
        assert check_once('/unavailable.php')['settings']['state']=='up'
        assert check_once('/unavailable.php')['settings']['state']=='down'
        assert check_once('/')['settings']['state']=='up'
        with a.opener.open(BASE+'/api/v2/domains/'+domain+'/zone') as response:zone=response.read().decode()
        assert '$ORIGIN '+domain_name in zone and 'v2-fixture' in zone
        a.call('/v2/domains/'+domain+'/cdn','POST',{'provider':'cloudflare'})
        a.call('/v2/domains/'+domain+'/cdn','POST',{'provider':'none'})
        assert a.call('/v2/domains/'+domain+'/tls')['renewal_timer_active']
        b.call('/v2/domains/'+domain+'/tls',status=404)
        job(a,'tls',domain,expected='failed',agree_tos=False,email='fixture@example.invalid',validation='http')
        a.call('/v2/jobs','POST',{'kind':'panel_tls','target':users[0]['id'],'agree_tos':True},status=403)
        challenge=ROOT/'challenge';challenge.write_text('controlled-acme-proof')
        webroot=pathlib.Path('/srv/cgpanel/acme/.well-known/acme-challenge');webroot.mkdir(parents=True,exist_ok=True)
        (webroot/suffix).write_text('controlled-acme-proof')
        try:
            assert urllib.request.urlopen('http://'+domain_name+'/.well-known/acme-challenge/'+suffix).read()==b'controlled-acme-proof'
        finally:(webroot/suffix).unlink()
        env=os.environ.copy();env.update(CERTBOT_DOMAIN=domain_name,CERTBOT_VALIDATION='controlled_acme_'+suffix)
        run(['/usr/local/bin/cgpanel-acme-hook','auth',domain],env=env)
        assert 'controlled_acme_'+suffix in run(['dig','@127.0.0.1','+short','TXT','_acme-challenge.'+domain_name])
        run(['/usr/local/bin/cgpanel-acme-hook','cleanup',domain],env=env)
        assert 'controlled_acme_'+suffix not in run(['dig','@127.0.0.1','+short','TXT','_acme-challenge.'+domain_name])
        assert 'v2-fixture' in run(['dig','@127.0.0.1','+short','TXT',domain_name])
        dates=a.call('/v2/cron-preview','POST',{'schedule':'*/5 8-17 * * 1-5','timezone':'Asia/Tehran'})['next'];assert len(dates)==5 and dates==sorted(dates)
        a.call('/v2/cron-preview','POST',{'schedule':'61 * * * *','timezone':'UTC'},status=400)
        cron=a.call('/resources/schedules','POST',{'name':'minute','app_id':app,'schedule':'* * * * *','timezone':'UTC','command':'printf cron-ok > /workspace/cron-result.txt'})['id'];resources.append(cron)
        for engine in ['mysql','postgresql']:
            db=a.call('/resources/databases','POST',{'name':'v2db','engine':engine,'allowed_ips':[]});db['engine']=engine;resources.append(db['id'])
            sql(db,"CREATE TABLE fixture (id integer PRIMARY KEY, value varchar(40)); INSERT INTO fixture VALUES (1,'original');")
            if engine=='mysql':databases=[db]
            else:databases.append(db)
        a.call('/resource/'+app+'/write','POST',{'path':'saved.txt','content':'archive original'})
        a.call('/resource/'+app+'/terminal','POST',{'command':'php -r \'file_put_contents("backup-payload.bin",random_bytes(10*1024*1024));\''})
        backup=job(a,'full_backup',app,database_ids=[d['id'] for d in databases],destinations=[],quiesce=True)['result']
        with a.opener.open(BASE+'/api/v2/backups/'+backup['id']+'/download') as response:archive=response.read()
        z=zipfile.ZipFile(io.BytesIO(archive));manifest=json.loads(z.read('manifest.json'))
        assert manifest['format']=='CGPanel' and len(manifest['files'])==3
        assert {r['kind'] for r in manifest['resources']}=={'apps','databases','domains','dns','schedules'}
        for f in manifest['files']:assert hashlib.sha256(z.read(f['path'])).hexdigest()==f['sha256']
        b.call('/v2/backups/'+backup['id']+'/download',status=400)
        for db in databases:sql(db,"UPDATE fixture SET value='changed';")
        a.call('/resource/'+app+'/write','POST',{'path':'saved.txt','content':'changed'})
        job(a,'restore_full',app,backup_id=backup['id'],confirm='RESTORE')
        assert a.call('/resource/'+app+'/read','POST',{'path':'saved.txt'})['output']=='archive original'
        for db in databases:assert 'original' in sql(db,'SELECT value FROM fixture;')
        print('PASS: ownership, analytics, IP hashing, heatmap, SEO, DNS export, cron parsing, full ZIP and both SQL restores',flush=True)
        # Public endpoint validation blocks loopback/metadata targets before any privileged request.
        a.call('/v2/integrations','POST',{'type':'proxy','name':'invalid','url':'socks5h://127.0.0.1:1080'},status=400)
        px=a.call('/v2/integrations','POST',{'type':'proxy','name':'fixture-proxy','url':f'socks5h://fixture:fixture-password@{IP}:{SOCKS_PORT}'})['id'];integrations.append(px)
        storage=a.call('/v2/integrations','POST',{'type':'s3','name':'fixture-s3','endpoint':f'https://{IP}:{S3_PORT}','bucket':'fixture','region':'us-east-1','prefix':'backups','access_key':ACCESS,'secret_key':SECRET,'ca_pem':ca,'proxy_id':px})['id'];integrations.append(storage)
        visible=json.dumps(a.call('/v2/integrations'));assert SECRET not in visible and 'fixture-password' not in visible and 'ca_pem' not in visible
        assert b.call('/v2/integrations')==[]
        job(a,'backup_deliver',backup['id'],destination=storage)
        assert objects['/fixture/backups/'+backup['id']+'.cgp']==archive
        assert any(n==2 for n in attempts.values()) and any(part==2 for _,part in attempts) and (IP,S3_PORT) in requests
        run(['useradd','-m','-s','/bin/sh',ssh_user]);sshdir=ssh_home/'.ssh';sshdir.mkdir(mode=0o700)
        sshkey=ROOT/('key_'+suffix);run(['ssh-keygen','-q','-t','ed25519','-N','','-f',str(sshkey)])
        (sshdir/'authorized_keys').write_text('restrict,command="internal-sftp" '+sshkey.with_suffix('.pub').read_text())
        (sshdir/'authorized_keys').chmod(0o600);(ssh_home/'backups').mkdir();run(['chown','-R',ssh_user+':'+ssh_user,str(ssh_home)])
        host_key=' '.join(pathlib.Path('/etc/ssh/ssh_host_ed25519_key.pub').read_text().split()[:2])
        ssh_config={'type':'ssh','name':'fixture-ssh','host':IP,'port':22,'username':ssh_user,'remote_dir':str(ssh_home/'backups'),'private_key':sshkey.read_text(),'host_key':host_key,'proxy_id':px}
        ssh=a.call('/v2/integrations','POST',{**ssh_config,'host_key':' '.join(sshkey.with_suffix('.pub').read_text().split()[:2])})['id'];integrations.append(ssh)
        job(a,'backup_deliver',backup['id'],expected='failed',destination=ssh)
        a.call('/v2/integrations','POST',{**ssh_config,'id':ssh})
        job(a,'backup_deliver',backup['id'],destination=ssh)
        assert (ssh_home/'backups'/(backup['id']+'.cgp')).read_bytes()==archive
        assert (IP,22) in requests, 'SFTP must use the selected SOCKS proxy'
        plan=a.call('/v2/backup-plans','POST',{'app_id':app,'database_ids':[d['id'] for d in databases],'destinations':[storage],'quiesce':True,'schedule':'* * * * *','timezone':'UTC','retention':2})['id'];plans.append(plan)
        print('PASS: secret redaction, S3 SigV4/multipart/retries through SOCKS, pinned SSH/SFTP through SOCKS',flush=True)
        for _ in range(3):
            before=next(p for p in a.call('/v2/backup-plans') if p['id']==plan)['last_job']
            # Advance only the disposable fixture's due time to exercise the real scheduler promptly.
            with sqlite3.connect('/var/lib/cgpanel/panel.db') as connection:
                connection.execute('UPDATE backup_plans SET next_run=? WHERE id=?',(int(time.time())-1,plan))
            until=time.monotonic()+40
            while time.monotonic()<until:
                last=next(p for p in a.call('/v2/backup-plans') if p['id']==plan)['last_job']
                if last and last!=before:break
                time.sleep(2)
            assert last and last!=before,'Scheduled backup was not dispatched'
            wait_job(a,last)
        scheduled=[b for b in a.call('/v2/backups') if b['plan_id']==plan]
        assert len(scheduled)==2 and all(b['delivery'][0]['success'] for b in scheduled),scheduled
        admin.call('/v2/backup-plans/'+plan,'DELETE');plans.clear()
        print('PASS: automatic backup dispatch, remote delivery and two-copy local retention',flush=True)
        job(admin,'egress',app,proxy_id=px,locked=True)
        job(a,'egress',app,expected='failed',proxy_id='')
        a.call('/v2/integrations','POST',{'id':px,'type':'proxy','name':'locked','url':f'socks5h://{IP}:1'},status=400)
        command='php -r \'echo file_get_contents("http://api.myip.com", false, stream_context_create(["http"=>["timeout"=>5]]));\''
        output=a.call('/resource/'+app+'/terminal','POST',{'command':command})['output'];assert '198.51.100.42' in output,output
        assert any(port==53 for _,port in requests) and ('1.1.1.1',80) in requests
        assert 'retained' in a.call('/resource/'+app+'/terminal','POST',{'command':'printenv FIXTURE_ENV'})['output']
        assert 'uid=1000' in a.call('/resource/'+app+'/terminal','POST',{'command':'id'})['output']
        proxy.close();asyncio.run_coroutine_threadsafe(proxy.wait_closed(),loop).result()
        blocked=a.call('/resource/'+app+'/terminal','POST',{'command':'php -r \'$x=@file_get_contents("http://1.1.1.1",false,stream_context_create(["http"=>["timeout"=>3]])); echo $x===false?"BLOCKED":"LEAKED";\''})['output'];assert 'BLOCKED' in blocked and 'LEAKED' not in blocked,blocked
        job(admin,'egress',app,proxy_id='',locked=False)
        print('PASS: unmodified PHP sockets and DNS use SOCKS; credentials retained; admin lock; proxy outage fails closed',flush=True)
        # Ensure an actual cron execution was recorded, rather than merely accepting syntax.
        until=time.monotonic()+90
        while time.monotonic()<until:
            history=a.call('/resource/'+cron+'/status','POST',{})
            if history.get('history'):break
            time.sleep(5)
        assert any(entry['success'] for entry in history['history']),history
        assert not errors,errors
        print('PASS: real container cron execution and bounded run history',flush=True)
        for rid in plans:admin.call('/v2/backup-plans/'+rid,'DELETE')
        plans.clear()
        hold=min(300,int(os.getenv('CGPANEL_TEST_HOLD_SECONDS','0')))
        if hold:
            print(f'Browser fixtures available for {hold} seconds',flush=True)
            time.sleep(hold)
    finally:
        if errors:print('Fixture protocol errors:',errors,flush=True)
        for rid in plans:
            try:admin.call('/v2/backup-plans/'+rid,'DELETE')
            except Exception:pass
        if users:
            try:
                for backup in admin.call('/v2/backups?owner='+users[0]['id']):
                    admin.call('/v2/backups/'+backup['id']+'?owner='+users[0]['id'],'DELETE')
            except Exception as e:print('Backup cleanup requires review:',str(e),flush=True)
        for rid in reversed(resources):
            try:admin.call('/resource/'+rid,'DELETE')
            except Exception as e:print('Resource cleanup requires review:',rid,str(e),flush=True)
        for rid in reversed(integrations):
            try:admin.call('/v2/integrations/'+rid+'?owner='+users[0]['id'],'DELETE')
            except Exception as e:print('Integration cleanup requires review:',rid,str(e),flush=True)
        for user in users:
            try:admin.call('/users/'+user['id'],'POST',{'enabled':False,'quota':1,'allowed_ips':[]})
            except Exception:pass
        hosts.write_text(original_hosts)
        if ssh_home.exists():subprocess.run(['userdel','-r',ssh_user],capture_output=True)
        server.shutdown();proxy.close();loop.call_soon_threadsafe(loop.stop)
        for path in ROOT.glob('key_'+suffix+'*'):path.unlink()

if __name__=='__main__':main()
