#!/usr/bin/env python3
"""Isolated MFA enrollment, required factor, replay and recovery tests."""
import base64,hashlib,hmac,os,secrets,socket,struct,subprocess,tempfile,time
from pathlib import Path
from admin_api import Client,BINARY
def code(secret,step):
    value=hmac.new(base64.b32decode(secret),struct.pack('>Q',step),hashlib.sha1).digest()
    offset=value[-1]&15
    return str((int.from_bytes(value[offset:offset+4],'big')&0x7fffffff)%1000000).zfill(6)
with tempfile.TemporaryDirectory(prefix='cgp-mfa-') as directory:
    folder=Path(directory);password=secrets.token_urlsafe(24)
    with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    env={**os.environ,'CGPANEL_DB':str(folder/'db.sqlite'),'CGPANEL_ADMIN':'admin','CGPANEL_ADMIN_PASSWORD':password,'CGPANEL_BIND':f'127.0.0.1:{port}','CGPANEL_INSECURE_LOCAL':'1','CGPANEL_AGENT_SOCKET':str(folder/'missing.sock')}
    subprocess.run([str(BINARY),'bootstrap'],env=env,check=True,capture_output=True)
    env.pop('CGPANEL_ADMIN_PASSWORD')
    with (folder/'log').open('w') as log:
        process=subprocess.Popen([str(BINARY)],env=env,stdout=log,stderr=log)
        try:
            client=Client(f'http://127.0.0.1:{port}')
            for _ in range(100):
                try:client.call('/healthz');break
                except OSError:time.sleep(.05)
            client.login('admin',password)
            secret=client.call('/api/v5/mfa/enroll','POST',{'password':password})['secret']
            step=int(time.time())//30;first=code(secret,step)
            result=client.call('/api/v5/mfa/confirm','POST',{'code':first})
            assert len(result['recovery_codes'])==10
            anonymous=Client(client.base)
            login={'username':'admin','password':password}
            anonymous.call('/api/login','POST',login,status=401)
            anonymous.call('/api/login','POST',{**login,'code':first},status=401)
            anonymous.call('/api/login','POST',{**login,'code':code(secret,step+1)})
            recovery=result['recovery_codes'][0]
            anonymous.call('/api/login','POST',{**login,'code':recovery})
            anonymous.call('/api/login','POST',{**login,'code':recovery},status=401)
            assert client.call('/api/v5/mfa')['recovery_codes_remaining']==9
            client.call('/api/v5/mfa/disable','POST',{'password':password,'code':result['recovery_codes'][1]})
            anonymous.call('/api/login','POST',login)
            assert not client.call('/api/v5/mfa')['enabled']
            print('PASS: MFA enrollment, mandatory second factor, TOTP replay rejection, single-use recovery, authenticated disable')
        finally:process.terminate();process.wait(timeout=10)
